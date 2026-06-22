use thiserror::Error;

#[derive(Debug, Error)]
pub enum RecorderError {
    // ── Privilege / preflight errors ─────────────────────────────────────────

    #[cfg(target_os = "linux")]
    #[error(
        "Insufficient privileges for kernel-level tracing.\n\
         Fix (choose one):\n  \
           sudo setcap cap_perfmon,cap_bpf=+eip <binary>   # grant capabilities\n  \
           sudo process-analyzer                            # run as root\n  \
           echo 1 | sudo tee /proc/sys/kernel/perf_event_paranoid  # relax globally"
    )]
    InsufficientPrivilegesLinux,

    #[cfg(target_os = "macos")]
    #[error(
        "DTrace / Instruments tracing requires elevated privileges.\n\
         Fix (choose one):\n  \
           sudo process-analyzer                            # run as root\n  \
           grant the 'com.apple.security.get-task-allow' entitlement and re-sign\n  \
         Note: System Integrity Protection (SIP) restricts DTrace on protected paths."
    )]
    InsufficientPrivilegesMacos,

    #[cfg(target_os = "windows")]
    #[error(
        "ETW kernel-mode logging requires Administrator privileges.\n\
         Fix: right-click process-analyzer.exe → Run as administrator"
    )]
    InsufficientPrivilegesWindows,

    // ── Session state errors ──────────────────────────────────────────────────

    #[error("recording already in progress; call stop() before starting a new session")]
    AlreadyRecording,

    #[error("no active recording to stop")]
    NotRecording,

    #[error("target process {pid} is not running")]
    ProcessNotFound { pid: u32 },

    // ── Disk / resource errors ────────────────────────────────────────────────

    #[error("disk full: output directory '{path}' has less than {min_bytes} bytes free")]
    DiskFull {
        path: std::path::PathBuf,
        min_bytes: u64,
    },

    #[error("output directory '{path}' does not exist or is not writable")]
    OutputDirUnusable { path: std::path::PathBuf },

    // ── Backend / I/O errors ──────────────────────────────────────────────────

    #[error("backend returned an error: {0}")]
    BackendError(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("container finalization failed: {0}")]
    ContainerError(String),
}
