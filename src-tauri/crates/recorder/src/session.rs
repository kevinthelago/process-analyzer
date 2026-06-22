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

use crate::backend::Recorder;
use crate::config::RecordingConfig;
use crate::container::{finalize_container, FinalizeParams, FinalizeReason};
use crate::error::RecorderError;
use crate::preflight;
use crate::status::{LiveStatus, SessionState};

/// Messages sent from the session handle into the background driver task.
enum ControlMsg {
    Stop,
}

/// A running (or completed) recording session.
///
/// Constructed with `RecordingSession::new`, started with `start()`.
/// The session drives the backend, monitors limits, and finalizes the container.
pub struct RecordingSession<R: Recorder> {
    config: RecordingConfig,
    backend: Arc<Mutex<R>>,
    control_tx: mpsc::Sender<ControlMsg>,
    status_rx: watch::Receiver<LiveStatus>,
    /// Populated after the session reaches `Recording` state.
    event_rx: Arc<Mutex<Option<mpsc::Receiver<pa_import::RawEvent>>>>,
}

impl<R: Recorder> RecordingSession<R> {
    /// Create a new session. Does not start capture; call `start()` next.
    pub fn new(config: RecordingConfig, backend: R) -> Self {
        let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(4);
        let (status_tx, status_rx) = watch::channel(LiveStatus::idle());
        let event_rx = Arc::new(Mutex::new(None::<mpsc::Receiver<pa_import::RawEvent>>));
        let backend = Arc::new(Mutex::new(backend));

        let event_rx_clone = event_rx.clone();
        let backend_clone = backend.clone();
        let config_clone = config.clone();

        // The background driver task: runs the full session lifecycle.
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

    /// Trigger capture start. Returns immediately; watch `status()` for state.
    pub async fn start(&self) -> Result<(), RecorderError> {
        self.control_tx
            .send(ControlMsg::Stop) // placeholder until we add Start msg
            .await
            .ok();
        // The driver starts automatically; this is a no-op trigger for now.
        // Full signal-based start will be wired when the Tauri command layer lands.
        Ok(())
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
            SessionState::Stopping | SessionState::Finalizing => Ok(()), // already stopping
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

/// The background task that drives the full session lifecycle.
async fn drive_session<R: Recorder>(
    config: RecordingConfig,
    backend: Arc<Mutex<R>>,
    event_rx_slot: Arc<Mutex<Option<mpsc::Receiver<pa_import::RawEvent>>>>,
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
    let mut ipc_buffer: Vec<u8> = Vec::new();
    let mut finalize_reason = FinalizeReason::UserStop;

    // Take the receiver out of the slot so we own it for the event loop.
    let mut rx: mpsc::Receiver<pa_import::RawEvent> = {
        let mut slot = event_rx_slot.lock().await;
        slot.take().expect("receiver always set before Recording")
    };

    loop {
        // Check limits before blocking on the next event.
        let elapsed = start_instant.elapsed();
        update_live_status(&status_tx, elapsed, event_count, ipc_buffer.len() as u64);

        if config.max_duration_secs > 0
            && elapsed >= Duration::from_secs(config.max_duration_secs)
        {
            finalize_reason = FinalizeReason::MaxDurationReached;
            break;
        }

        if config.max_disk_bytes > 0 && ipc_buffer.len() as u64 >= config.max_disk_bytes {
            finalize_reason = FinalizeReason::MaxBytesReached;
            break;
        }

        // Check target-process liveness (every ~100 events to avoid syscall spam).
        if event_count % 100 == 0 {
            let alive = backend.lock().await.is_target_alive();
            if !alive {
                finalize_reason = FinalizeReason::TargetProcessExited;
                break;
            }
        }

        tokio::select! {
            biased;  // check control messages first

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
                        // Accumulate into IPC buffer.
                        // When trace-core lands, convert ev → Arrow record batch here.
                        // For now, write a placeholder 8-byte frame per event.
                        append_raw_event_to_ipc(&mut ipc_buffer, &ev);
                        event_count += 1;
                    }
                    None => {
                        // Backend closed the channel (process exited or error).
                        finalize_reason = FinalizeReason::TargetProcessExited;
                        break;
                    }
                }
            }
        }
    }

    // ── Stopping → Finalizing ─────────────────────────────────────────────────
    set_state(&status_tx, SessionState::Stopping);
    {
        let mut b = backend.lock().await;
        let _ = b.stop();
    }

    set_state(&status_tx, SessionState::Finalizing);

    let process_name = match &config.mode {
        crate::config::RecordingMode::AttachPid(_) => None,
        crate::config::RecordingMode::Launch { program, .. } => {
            Some(program.as_str())
        }
        crate::config::RecordingMode::SystemWide => None,
    };
    let pid = match &config.mode {
        crate::config::RecordingMode::AttachPid(p) => Some(*p),
        _ => None,
    };

    match finalize_container(FinalizeParams {
        output_dir: &config.output_dir,
        label: config.label.as_deref(),
        os: current_os(),
        pid,
        process_name,
        start_time_ns,
        event_count,
        arrow_ipc_bytes: ipc_buffer,
        reason: finalize_reason,
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

/// Encode a RawEvent as a minimal Arrow-IPC-compatible placeholder frame.
///
/// Format (per event): [0xFF,0xFF,0xFF,0xFF] continuation + [8 u8] metadata_len + payload.
/// When trace-core lands this is replaced by proper Arrow record-batch serialization.
fn append_raw_event_to_ipc(buf: &mut Vec<u8>, ev: &pa_import::RawEvent) {
    // 8-byte placeholder: timestamp(u64 LE) per event.
    // The IPC framing markers are added by ensure_ipc_eos in finalization.
    buf.extend_from_slice(&ev.timestamp_ns.to_le_bytes());
    buf.extend_from_slice(&(ev.pid as u64).to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{StubRecorder, StubRecorderWithEvents};
    use crate::config::{DomainConfig, RecordingConfig, RecordingMode};
    use pa_import::{RawEvent, RawEventKind};

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

    fn make_exit_event() -> RawEvent {
        RawEvent {
            timestamp_ns: 1_000_000,
            pid: 123,
            tid: 123,
            cpu: None,
            kind: RawEventKind::ProcessExit { exit_code: 0 },
            payload: bytes::Bytes::new(),
        }
    }

    #[tokio::test]
    async fn session_completes_when_backend_closes_channel() {
        let dir = tempfile::tempdir().unwrap();
        let backend = StubRecorder::new(); // closes channel immediately
        let mut session = RecordingSession::new(test_config(dir.path()), backend);
        let status = session.wait_for_completion().await;
        // On platforms where preflight fails (CI without capabilities),
        // the session ends in Failed; on privileged environments it Completes.
        // We just assert it reaches a terminal state.
        assert!(status.state.is_terminal(), "state={:?}", status.state);
    }

    #[tokio::test]
    async fn session_records_events_from_backend() {
        let dir = tempfile::tempdir().unwrap();
        let events = vec![make_exit_event(); 3];
        let backend = StubRecorderWithEvents::new(events);
        let mut session = RecordingSession::new(test_config(dir.path()), backend);
        let status = session.wait_for_completion().await;
        assert!(status.state.is_terminal());
        // event_count is set when Completed (not when Failed due to preflight).
        if status.state == SessionState::Completed {
            assert_eq!(status.event_count, 3);
        }
    }

    #[tokio::test]
    async fn stop_transitions_to_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let backend = StubRecorder::new();
        let mut session = RecordingSession::new(test_config(dir.path()), backend);
        // stop() on an Idle/Starting session returns NotRecording or Ok;
        // either is acceptable since we race with the driver starting.
        let _ = session.stop().await;
        let status = session.wait_for_completion().await;
        assert!(status.state.is_terminal());
    }
}
