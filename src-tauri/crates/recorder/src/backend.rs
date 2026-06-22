/// Contract stub for the platform capture backends.
///
/// The three backend crates (linux-backend, macos-backend, windows-backend)
/// each implement `Recorder` for their platform. This trait is the seam:
/// pa-recorder drives them through it; the backends implement it.
///
/// NOTE: This trait definition must stay in sync with the contract file
/// in the contracts directory. When the backend streams land, they import
/// this trait from pa-recorder (or a shared contract crate if the director
/// decides to introduce one).
use tokio::sync::mpsc;

use pa_import::RawEvent;

use crate::config::RecordingConfig;
use crate::error::RecorderError;

/// A platform-specific capture backend.
///
/// All methods are synchronous: backends manage their own threads/tasks
/// internally and communicate through the event channel returned by `start`.
pub trait Recorder: Send + 'static {
    /// Start capturing according to `config`. Returns a receiver through which
    /// the backend sends `RawEvent`s. The backend keeps sending until it is
    /// stopped, the target process exits, or an unrecoverable error occurs
    /// (in which case it closes the sender side so the receiver sees `None`).
    ///
    /// Must only be called once per session; call `stop` before reusing.
    fn start(
        &mut self,
        config: &RecordingConfig,
    ) -> Result<mpsc::Receiver<RawEvent>, RecorderError>;

    /// Signal the backend to stop capturing. Blocks until capture is fully
    /// drained and the event sender is closed.
    fn stop(&mut self) -> Result<(), RecorderError>;

    /// Returns `true` if the target process (for `AttachPid`/`Launch` modes)
    /// is still alive. Always returns `true` for `SystemWide` mode.
    fn is_target_alive(&self) -> bool;

    /// Platform name for logging / manifest embedding.
    fn platform_name(&self) -> &'static str;
}

/// A no-op backend used in tests and CI environments where a real backend is
/// not available. Emits a configurable burst of synthetic events then closes.
pub struct StubRecorder {
    target_alive: bool,
}

impl StubRecorder {
    pub fn new() -> Self {
        Self { target_alive: true }
    }
}

impl Default for StubRecorder {
    fn default() -> Self { Self::new() }
}

impl Recorder for StubRecorder {
    fn start(
        &mut self,
        _config: &RecordingConfig,
    ) -> Result<mpsc::Receiver<RawEvent>, RecorderError> {
        let (tx, rx) = mpsc::channel(64);
        // The sender is dropped immediately, so the stream closes right away.
        // Tests that need events should use `StubRecorder` with a custom impl.
        drop(tx);
        Ok(rx)
    }

    fn stop(&mut self) -> Result<(), RecorderError> {
        self.target_alive = false;
        Ok(())
    }

    fn is_target_alive(&self) -> bool {
        self.target_alive
    }

    fn platform_name(&self) -> &'static str {
        "stub"
    }
}

/// An emitting stub that sends a fixed set of events before closing.
pub struct StubRecorderWithEvents {
    events: Vec<RawEvent>,
}

impl StubRecorderWithEvents {
    pub fn new(events: Vec<RawEvent>) -> Self {
        Self { events }
    }
}

impl Recorder for StubRecorderWithEvents {
    fn start(
        &mut self,
        _config: &RecordingConfig,
    ) -> Result<mpsc::Receiver<RawEvent>, RecorderError> {
        let (tx, rx) = mpsc::channel(self.events.len() + 1);
        for ev in self.events.drain(..) {
            // Channel is large enough; send won't block.
            let _ = tx.try_send(ev);
        }
        // Drop tx to close the channel when events are consumed.
        drop(tx);
        Ok(rx)
    }

    fn stop(&mut self) -> Result<(), RecorderError> {
        Ok(())
    }

    fn is_target_alive(&self) -> bool {
        false
    }

    fn platform_name(&self) -> &'static str {
        "stub-with-events"
    }
}
