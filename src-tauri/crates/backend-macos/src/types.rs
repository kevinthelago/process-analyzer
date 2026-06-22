/// Configuration for a recording session.
#[derive(Debug, Clone)]
pub struct RecordConfig {
    /// Stack samples per second (default: 100).
    pub sample_rate_hz: u32,
    /// Run DTrace for I/O and scheduling events. Requires elevated privileges
    /// or SIP partially disabled. On failure, falls back to empty event sets.
    pub enable_dtrace: bool,
    /// Collect memory snapshots via task_info(TASK_VM_INFO) (default: true).
    pub enable_memory: bool,
    /// Override the built-in io+sched DTrace script. Must consume `$target`.
    pub dtrace_script: Option<String>,
    /// Fixed recording duration. `None` means record until `stop()` is called.
    pub duration_secs: Option<f64>,
}

impl Default for RecordConfig {
    fn default() -> Self {
        Self {
            sample_rate_hz: 100,
            enable_dtrace: true,
            enable_memory: true,
            dtrace_script: None,
            duration_secs: None,
        }
    }
}

/// A stack sample from one thread at one instant.
#[derive(Debug, Clone)]
pub struct StackSample {
    /// Nanoseconds since the Unix epoch (CLOCK_REALTIME).
    pub timestamp_ns: u64,
    pub pid: u32,
    /// OS-assigned thread identifier (from `thread_identifier_info`).
    pub tid: u64,
    /// Raw instruction-pointer addresses, outermost (current) frame first.
    /// Addresses are from the target process's virtual address space and have
    /// PAC bits stripped.
    pub frames: Vec<u64>,
}

/// A block-layer I/O event captured via DTrace `io:::start`.
#[derive(Debug, Clone)]
pub struct IoEvent {
    pub timestamp_ns: u64,
    pub pid: u32,
    pub tid: u64,
    pub kind: IoKind,
    pub bytes: u64,
    /// Filesystem path of the file involved, if available.
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoKind {
    Read,
    Write,
}

/// A CPU scheduling event captured via DTrace `sched:::on-cpu` / `off-cpu`.
#[derive(Debug, Clone)]
pub struct SchedEvent {
    pub timestamp_ns: u64,
    pub pid: u32,
    pub tid: u64,
    pub kind: SchedKind,
    pub cpu: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedKind {
    OnCpu,
    OffCpu,
}

/// Memory figures for one point in time, from `task_info(TASK_VM_INFO)`.
#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    pub timestamp_ns: u64,
    pub pid: u32,
    /// Resident set size (RSS) in bytes.
    pub resident_bytes: u64,
    /// Virtual memory footprint in bytes.
    pub virtual_bytes: u64,
    /// Physical memory footprint as reported by Activity Monitor (`phys_footprint`).
    pub footprint_bytes: u64,
}

/// A Mach-O image loaded in the target process, from the dyld image list.
#[derive(Debug, Clone)]
pub struct DyldImage {
    /// Base load address (start of the Mach-O header in the target VA space).
    pub load_address: u64,
    /// ASLR slide: `load_address - preferred_load_address`.
    pub slide: i64,
    /// Path to the binary on disk.
    pub path: String,
    /// LC_UUID bytes from the Mach-O header (16 bytes), or all-zeros if not found.
    pub uuid: [u8; 16],
}

/// All data collected during one recording session.
#[derive(Debug, Default)]
pub struct TraceData {
    pub stack_samples: Vec<StackSample>,
    pub io_events: Vec<IoEvent>,
    pub sched_events: Vec<SchedEvent>,
    pub memory_snapshots: Vec<MemorySnapshot>,
    pub dyld_images: Vec<DyldImage>,
}
