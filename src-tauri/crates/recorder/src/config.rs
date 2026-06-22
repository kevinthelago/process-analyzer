use serde::{Deserialize, Serialize};

/// What to record within a capture domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainConfig {
    /// Capture CPU scheduling / context switches.
    pub scheduler: bool,
    /// Capture CPU performance-counter samples.
    pub cpu_samples: bool,
    /// Capture file-system I/O events.
    pub file_io: bool,
    /// Capture network I/O events.
    pub network_io: bool,
    /// Capture memory allocation events.
    pub memory: bool,
    /// Target sample rate in Hz (for CPU sampling). 0 → backend default (~1000 Hz).
    pub sample_rate_hz: u32,
}

impl Default for DomainConfig {
    fn default() -> Self {
        Self {
            scheduler: true,
            cpu_samples: true,
            file_io: true,
            network_io: false,
            memory: false,
            sample_rate_hz: 0,
        }
    }
}

/// How to bind the recording to a process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecordingMode {
    /// Start capturing immediately, system-wide (all processes).
    SystemWide,
    /// Record only events belonging to the given process ID (and its children).
    AttachPid(u32),
    /// Launch the given command, record it from the start.
    Launch { program: String, args: Vec<String> },
}

/// Full configuration for a recording session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingConfig {
    pub mode: RecordingMode,
    pub domains: DomainConfig,
    /// Directory where the output `.patrace` file will be written.
    pub output_dir: std::path::PathBuf,
    /// Optional human-readable label embedded in the container manifest.
    pub label: Option<String>,
    /// Maximum disk use in bytes before capture is force-stopped.
    /// 0 means no limit.
    pub max_disk_bytes: u64,
    /// Maximum duration in seconds before capture is force-stopped.
    /// 0 means no limit.
    pub max_duration_secs: u64,
}

impl RecordingConfig {
    pub fn attach(pid: u32, output_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            mode: RecordingMode::AttachPid(pid),
            domains: DomainConfig::default(),
            output_dir: output_dir.into(),
            label: None,
            max_disk_bytes: 0,
            max_duration_secs: 0,
        }
    }

    pub fn system_wide(output_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            mode: RecordingMode::SystemWide,
            domains: DomainConfig::default(),
            output_dir: output_dir.into(),
            label: None,
            max_disk_bytes: 0,
            max_duration_secs: 0,
        }
    }
}
