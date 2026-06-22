use thiserror::Error;
use windows::Win32::Foundation::WIN32_ERROR;

#[derive(Debug, Error)]
pub enum RecordError {
    #[error(
        "Process Analyzer requires Administrator privileges. \
         Please re-run as Administrator (right-click → Run as administrator)."
    )]
    NotElevated,

    /// ERROR_ALREADY_EXISTS (183) from StartTrace — another tool holds the NT Kernel Logger.
    #[error(
        "The NT Kernel Logger session is already running. \
         Another profiling tool (e.g. WPA, PerfView, xperf) may be active. \
         Stop that conflicting session and try again."
    )]
    KernelLoggerConflict,

    #[error("ETW session error (Win32 code {code:#010x}): {message}")]
    EtwSession { code: u32, message: String },

    #[error("ETW consumer thread error: {0}")]
    EtwConsumer(String),

    #[error("wpr.exe fallback failed: {0}")]
    WprFailed(String),

    #[error(
        "wpr.exe not found. Install the Windows Performance Toolkit \
         (part of the Windows SDK or ADK) to use the wpr.exe fallback."
    )]
    WprNotFound,

    #[error("A capture session is already active; call stop() before starting a new one")]
    AlreadyRunning,

    #[error("No capture session is currently active")]
    NotRunning,

    #[error("Internal error: {0}")]
    Internal(String),
}

impl RecordError {
    /// Convert a Win32 error code to the appropriate `RecordError` variant.
    pub(crate) fn from_win32(err: WIN32_ERROR) -> Self {
        // ERROR_ALREADY_EXISTS = 183
        if err.0 == 183 {
            return RecordError::KernelLoggerConflict;
        }
        // WIN32_ERROR::ok() returns Err(windows::core::Error) for non-zero codes,
        // which formats as the human-readable Win32 message string.
        let message = match err.ok() {
            Err(e) => e.to_string(),
            Ok(()) => format!("error code {:#010x}", err.0),
        };
        RecordError::EtwSession { code: err.0, message }
    }
}
