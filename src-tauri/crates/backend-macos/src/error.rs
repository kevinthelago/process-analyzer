use thiserror::Error;

#[derive(Debug, Error)]
pub enum RecordError {
    /// `task_for_pid` was denied. Run as root or sign with
    /// `com.apple.security.cs.debugger` + `NSAppleEventsUsageDescription`.
    #[error(
        "permission denied acquiring task port for pid {pid}: {detail}\n\
         Remediation: run as root, or codesign with \
         com.apple.security.cs.debugger entitlement, or use xctrace fallback"
    )]
    PermissionDenied { pid: u32, detail: String },

    #[error("process not found: pid {0}")]
    ProcessNotFound(u32),

    #[error("mach kernel error {code:#010x} in {context}")]
    MachError { code: i32, context: &'static str },

    /// DTrace subprocess failed to start or produced an error.
    /// I/O and scheduling events will be absent; the caller should still
    /// return stack samples and memory snapshots.
    #[error(
        "DTrace unavailable: {0}\n\
         SIP may be restricting DTrace. Disable with \
         `csrutil enable --without dtrace` or use xctrace fallback."
    )]
    DTraceUnavailable(String),

    #[error("xctrace error: {0}")]
    XctraceError(String),

    #[error(
        "xctrace not found — install Xcode Command Line Tools: xcode-select --install"
    )]
    XctraceNotFound,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("already recording pid {0}")]
    AlreadyRecording(u32),

    #[error("not recording")]
    NotRecording,

    #[error("remote memory read failed at {address:#018x}: {detail}")]
    MemoryReadFailed { address: u64, detail: String },

    #[error("dyld image list unavailable: {0}")]
    DyldInfoUnavailable(String),
}

impl RecordError {
    /// True when the error is due to insufficient privilege, meaning xctrace
    /// is a viable fallback.
    pub fn is_permission_error(&self) -> bool {
        matches!(self, RecordError::PermissionDenied { .. })
    }
}
