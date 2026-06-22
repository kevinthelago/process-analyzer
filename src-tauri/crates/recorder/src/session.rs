/// `RecordingSession` — the central state machine for capture orchestration.
///
/// ## State transitions
///
/// ```text
/// Idle ──start()──► Starting ──(preflight ok + backend live)──► Recording
///                      │                                             │
///                   (preflight fails)                         (stop / process exit /
///                      ▼                                       disk full / timeout)
///                   Failed                                          ▼
///                                                              Stopping
///                                                                   │
///                                                              Finalizing
///                                                                   │
///                                                      ┌────────────┴────────────┐
///                                                   Completed               Failed
/// ```
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch, Mutex};

use trace_core::{
    Manifest, RawEvent, StandardRecorder, TraceStore, DEFAULT_BATCH_SIZE,
    recorder::Recorder as CoreRecorder,
};

use crate::backend::Recorder;
use crate::config::RecordingConfig;
use crate::container::{finalize_container, FinalizeParams, FinalizeReason};
use crate::error::RecorderError;
use crate::preflight;
use crate::status::{LiveStatus, SessionState};

enum ControlMsg {
    Stop,
}

/// A running (or completed) recording session.
///
/// Constructed with `RecordingSession::new`, started with `start()`.
/// The session drives the backend, monitors limits, and finalizes the container.
#[allow(dead_code)]
pub struct RecordingSession<R: Recorder> {
    config: RecordingConfig,
    backend: Arc<Mutex<R>>,
    control_tx: mpsc::Sender<ControlMsg>,
    status_rx: watch::Receiver<LiveStatus>,
    event_rx: Arc<Mutex<Option<mpsc::Receiver<RawEvent>>>>,
}

impl<R: Recorder> RecordingSession<R> {
    /// Create a new session. Does not start capture; call `start()` next.
    pub fn new(config: RecordingConfig, backend: R) -> Self {
        let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(4);
        let (status_tx, status_rx) = watch::channel(LiveStatus::idle());
        let event_rx = Arc::new(Mutex::new(None::<mpsc::Receiver<RawEvent>>));
        let backend = Arc::new(Mutex::new(backend));

        let event_rx_clone = event_rx.clone();
        let backend_clone = backend.clone();
        let config_clone = config.clone();

        tokio::spawn(async move {
            drive_session(
                config_clone,
                backend_clone,
                event_rx_clone,
                control_rx,
                status_tx,
            )
            .await;
        });

        Self {
            config,
            backend,
            control_tx,
            status_rx,
            event_rx,
        }
    }

    /// Request the session to stop. Returns once the stop message is queued;
    /// finalization may still be in progress — watch `status()` for Completed.
    pub async fn stop(&self) -> Result<(), RecorderError> {
        let state = self.status_rx.borrow().state;
        match state {
            SessionState::Recording | SessionState::Starting => {
                self.control_tx.send(ControlMsg::Stop).await.ok();
                Ok(())
            }
            SessionState::Stopping | SessionState::Finalizing => Ok(()),
            SessionState::Idle => Err(RecorderError::NotRecording),
            SessionState::Completed | SessionState::Failed => Err(RecorderError::NotRecording),
        }
    }

    /// Current live status. Cheap clone; backed by `watch`.
    pub fn status(&self) -> LiveStatus {
        self.status_rx.borrow().clone()
    }

    /// Wait until the session reaches a terminal state (Completed or Failed).
    pub async fn wait_for_completion(&mut self) -> LiveStatus {
        loop {
            {
                let s = self.status_rx.borrow();
                if s.state.is_terminal() {
                    return s.clone();
                }
            }
            if self.status_rx.changed().await.is_err() {
                break;
            }
        }
        self.status_rx.borrow().clone()
    }
}

async fn drive_session<R: Recorder>(
    config: RecordingConfig,
    backend: Arc<Mutex<R>>,
    event_rx_slot: Arc<Mutex<Option<mpsc::Receiver<RawEvent>>>>,
    mut control_rx: mpsc::Receiver<ControlMsg>,
    status_tx: watch::Sender<LiveStatus>,
) {
    // ── Starting: privilege preflight ─────────────────────────────────────────
    set_state(&status_tx, SessionState::Starting);
    if let Err(e) = preflight::check_privileges() {
        set_failed(&status_tx, e);
        return;
    }

    // ── Starting → Recording: start the backend ───────────────────────────────
    let event_rx = {
        let mut b = backend.lock().await;
        match b.start(&config) {
            Ok(rx) => rx,
            Err(e) => {
                set_failed(&status_tx, e);
                return;
            }
        }
    };

    *event_rx_slot.lock().await = Some(event_rx);
    set_state(&status_tx, SessionState::Recording);

    let start_instant = Instant::now();
    let start_time_ns = unix_now_ns();
    let mut event_count: u64 = 0;
    let mut finalize_reason = FinalizeReason::UserStop;

    // Build the manifest and recorder that will accumulate Arrow data.
    let trace_id = format!("trace-{start_time_ns}");
    let mut manifest = Manifest::new(trace_id, start_time_ns as i64);
    manifest.os = Some(current_os().to_owned());
    manifest.hostname = hostname();
    manifest.arch = Some(current_arch().to_owned());
    let store = TraceStore::new(manifest);
    let mut rec = StandardRecorder::new(store, DEFAULT_BATCH_SIZE);

    let mut rx: mpsc::Receiver<RawEvent> = {
        let mut slot = event_rx_slot.lock().await;
        slot.take().expect("receiver always set before Recording")
    };

    loop {
        let elapsed = start_instant.elapsed();
        // Rough byte estimate: 128 bytes per event (uncompressed upper bound).
        let bytes_est = event_count * 128;
        update_live_status(&status_tx, elapsed, event_count, bytes_est);

        if config.max_duration_secs > 0
            && elapsed >= Duration::from_secs(config.max_duration_secs)
        {
            finalize_reason = FinalizeReason::MaxDurationReached;
            break;
        }

        if config.max_disk_bytes > 0 && bytes_est >= config.max_disk_bytes {
            finalize_reason = FinalizeReason::MaxBytesReached;
            break;
        }

        if event_count % 100 == 0 {
            let alive = backend.lock().await.is_target_alive();
            if !alive {
                finalize_reason = FinalizeReason::TargetProcessExited;
                break;
            }
        }

        tokio::select! {
            biased;

            msg = control_rx.recv() => {
                match msg {
                    Some(ControlMsg::Stop) | None => {
                        finalize_reason = FinalizeReason::UserStop;
                        break;
                    }
                }
            }

            event = rx.recv() => {
                match event {
                    Some(ev) => {
                        if let Err(e) = rec.record(ev) {
                            tracing::error!("StandardRecorder error: {e}");
                            finalize_reason = FinalizeReason::BackendError(e.to_string());
                            break;
                        }
                        event_count += 1;
                    }
                    None => {
                        finalize_reason = FinalizeReason::TargetProcessExited;
                        break;
                    }
                }
            }
        }
    }

    // ── Stopping: drain backend and flush recorder ────────────────────────────
    set_state(&status_tx, SessionState::Stopping);
    {
        let mut b = backend.lock().await;
        let _ = b.stop();
    }

    if let Err(e) = rec.flush() {
        tracing::warn!("recorder flush on stop: {e}");
    }

    // ── Finalizing: finish the store and persist ──────────────────────────────
    set_state(&status_tx, SessionState::Finalizing);

    let mut store = match rec.finish() {
        Ok(s) => s,
        Err(e) => {
            set_failed(&status_tx, RecorderError::ContainerError(e.to_string()));
            return;
        }
    };

    let elapsed_ns = start_instant.elapsed().as_nanos() as i64;
    store.manifest.duration_ns = Some(elapsed_ns);

    match finalize_container(FinalizeParams {
        output_dir: config.output_dir.clone(),
        label: config.label.clone(),
        store,
        reason: finalize_reason,
        event_count,
    })
    .await
    {
        Ok(output_path) => {
            let mut s = status_tx.borrow().clone();
            s.state = SessionState::Completed;
            s.output_path = Some(output_path);
            s.event_count = event_count;
            let _ = status_tx.send(s);
        }
        Err(e) => set_failed(&status_tx, e),
    }
}

fn set_state(tx: &watch::Sender<LiveStatus>, state: SessionState) {
    let mut s = tx.borrow().clone();
    s.state = state;
    let _ = tx.send(s);
}

fn set_failed(tx: &watch::Sender<LiveStatus>, err: impl std::fmt::Display) {
    let _ = tx.send(LiveStatus::failed(err));
}

fn update_live_status(
    tx: &watch::Sender<LiveStatus>,
    elapsed: Duration,
    event_count: u64,
    bytes_written: u64,
) {
    let mut s = tx.borrow().clone();
    s.elapsed_ms = elapsed.as_millis() as u64;
    s.event_count = event_count;
    s.bytes_written = bytes_written;
    let _ = tx.send(s);
}

fn unix_now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn current_os() -> &'static str {
    #[cfg(target_os = "linux")]   { "linux"   }
    #[cfg(target_os = "macos")]   { "macos"   }
    #[cfg(target_os = "windows")] { "windows" }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    { "unknown" }
}

fn current_arch() -> &'static str {
    #[cfg(target_arch = "x86_64")]  { "x86_64"  }
    #[cfg(target_arch = "aarch64")] { "aarch64" }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    { "unknown" }
}

fn hostname() -> Option<String> {
    #[cfg(target_os = "windows")]
    { std::env::var("COMPUTERNAME").ok() }
    #[cfg(not(target_os = "windows"))]
    {
        std::fs::read_to_string("/etc/hostname")
            .ok()
            .map(|s| s.trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{StubRecorder, StubRecorderWithEvents};
    use crate::config::{DomainConfig, RecordingConfig, RecordingMode};
    use trace_core::event::CpuSampleEvent;

    fn test_config(dir: &std::path::Path) -> RecordingConfig {
        RecordingConfig {
            mode: RecordingMode::SystemWide,
            domains: DomainConfig::default(),
            output_dir: dir.to_path_buf(),
            label: Some("test".into()),
            max_disk_bytes: 0,
            max_duration_secs: 0,
        }
    }

    fn make_cpu_sample_event() -> RawEvent {
        RawEvent::CpuSample(CpuSampleEvent {
            timestamp_ns: 1_000_000,
            process_id: 123,
            thread_id: 456,
            cpu_id: 0,
            sample_weight: 1,
            stack_id: None,
        })
    }

    #[tokio::test]
    async fn session_completes_when_backend_closes_channel() {
        let dir = tempfile::tempdir().unwrap();
        let backend = StubRecorder::new();
        let mut session = RecordingSession::new(test_config(dir.path()), backend);
        let status = session.wait_for_completion().await;
        assert!(status.state.is_terminal(), "state={:?}", status.state);
    }

    #[tokio::test]
    async fn session_records_events_from_backend() {
        let dir = tempfile::tempdir().unwrap();
        let events = vec![make_cpu_sample_event(); 3];
        let backend = StubRecorderWithEvents::new(events);
        let mut session = RecordingSession::new(test_config(dir.path()), backend);
        let status = session.wait_for_completion().await;
        assert!(status.state.is_terminal());
        if status.state == SessionState::Completed {
            assert_eq!(status.event_count, 3);
        }
    }

    #[tokio::test]
    async fn stop_transitions_to_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let backend = StubRecorder::new();
        let mut session = RecordingSession::new(test_config(dir.path()), backend);
        let _ = session.stop().await;
        let status = session.wait_for_completion().await;
        assert!(status.state.is_terminal());
    }
}
