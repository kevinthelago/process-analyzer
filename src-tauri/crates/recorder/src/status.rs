use serde::{Deserialize, Serialize};

/// The high-level state of a `RecordingSession`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Session created but not yet started.
    Idle,
    /// Privilege preflight running; backend not yet live.
    Starting,
    /// Backend is active and emitting events.
    Recording,
    /// Stop requested; draining the event stream.
    Stopping,
    /// Writing and sealing the `.patrace` container.
    Finalizing,
    /// Container written successfully. Terminal state.
    Completed,
    /// An unrecoverable error occurred. Terminal state.
    Failed,
}

impl SessionState {
    pub fn is_terminal(self) -> bool {
        matches!(self, SessionState::Completed | SessionState::Failed)
    }
}

/// Real-time statistics about an active recording session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveStatus {
    pub state: SessionState,
    /// Elapsed capture time in milliseconds (0 if not yet started).
    pub elapsed_ms: u64,
    /// Events captured so far.
    pub event_count: u64,
    /// Bytes written to the temporary capture buffer.
    pub bytes_written: u64,
    /// Error message if `state == Failed`.
    pub error: Option<String>,
    /// Path of the completed `.patrace` file, set when `state == Completed`.
    pub output_path: Option<std::path::PathBuf>,
}

impl LiveStatus {
    pub fn idle() -> Self {
        Self {
            state: SessionState::Idle,
            elapsed_ms: 0,
            event_count: 0,
            bytes_written: 0,
            error: None,
            output_path: None,
        }
    }

    pub fn failed(err: impl std::fmt::Display) -> Self {
        Self {
            state: SessionState::Failed,
            error: Some(err.to_string()),
            ..Self::idle()
        }
    }
}
