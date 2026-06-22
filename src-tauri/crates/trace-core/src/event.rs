//! Raw normalized events produced by capture backends.
//!
//! These types are the bridge between OS-specific capture code and the
//! domain-agnostic Arrow tables.  Each variant maps 1-to-1 with a table schema
//! defined in [`crate::schema`].

// ── enum codes ────────────────────────────────────────────────────────────────

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingEventType {
    ContextSwitchOut = 0,
    ContextSwitchIn  = 1,
    Wakeup           = 2,
    Migration        = 3,
}

/// Disk I/O operation.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskOp {
    Read    = 0,
    Write   = 1,
    Flush   = 2,
    Discard = 3,
}

/// File I/O operation (syscall level).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOp {
    Read  = 0,
    Write = 1,
    Open  = 2,
    Close = 3,
    Seek  = 4,
    Mmap  = 5,
}

/// Memory event type.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryEventType {
    Alloc      = 0,
    Free       = 1,
    PageFault  = 2,
    Mmap       = 3,
    Munmap     = 4,
}

/// Network socket operation.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkOp {
    Send    = 0,
    Recv    = 1,
    Connect = 2,
    Accept  = 3,
    Close   = 4,
}

/// Network protocol.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp   = 0,
    Udp   = 1,
    Other = 2,
}

// ── event structs ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CpuSampleEvent {
    pub timestamp_ns:  i64,
    pub process_id:    u32,
    pub thread_id:     u32,
    pub cpu_id:        u32,
    /// Sampling weight — 1 for uniform sampling, >1 for weighted/frequency-scaled.
    pub sample_weight: u64,
    /// FK into the `stacks` table; None if no stack was captured.
    pub stack_id:      Option<u64>,
}

#[derive(Debug, Clone)]
pub struct SchedulingEvent {
    pub timestamp_ns:    i64,
    pub process_id:      u32,
    pub thread_id:       u32,
    pub cpu_id:          u32,
    pub event_type:      SchedulingEventType,
    /// Linux task_struct state bits before the event.
    pub prev_state:      Option<u8>,
    pub next_process_id: Option<u32>,
    pub next_thread_id:  Option<u32>,
    pub duration_ns:     Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DiskIoEvent {
    pub timestamp_ns: i64,
    pub process_id:   u32,
    pub thread_id:    u32,
    pub operation:    DiskOp,
    pub device_major: u32,
    pub device_minor: u32,
    pub sector:       u64,
    pub size_bytes:   u64,
    pub duration_ns:  Option<u64>,
}

#[derive(Debug, Clone)]
pub struct FileIoEvent {
    pub timestamp_ns: i64,
    pub process_id:   u32,
    pub thread_id:    u32,
    pub operation:    FileOp,
    pub fd:           Option<i32>,
    /// Hash of the file path string — stable grouping key.
    pub path_hash:    Option<u64>,
    pub offset_bytes: Option<i64>,
    pub size_bytes:   Option<u64>,
    pub duration_ns:  Option<u64>,
    pub return_value: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct MemoryEvent {
    pub timestamp_ns: i64,
    pub process_id:   u32,
    pub thread_id:    Option<u32>,
    pub event_type:   MemoryEventType,
    pub address:      Option<u64>,
    pub size_bytes:   Option<u64>,
    pub numa_node:    Option<u32>,
}

#[derive(Debug, Clone)]
pub struct NetworkEvent {
    pub timestamp_ns: i64,
    pub process_id:   u32,
    pub thread_id:    Option<u32>,
    pub operation:    NetworkOp,
    pub protocol:     Protocol,
    pub fd:           Option<i32>,
    /// 16-byte IPv6 representation; IPv4 stored as IPv4-in-IPv6.
    pub local_addr:   Option<[u8; 16]>,
    pub remote_addr:  Option<[u8; 16]>,
    pub local_port:   Option<u16>,
    pub remote_port:  Option<u16>,
    pub size_bytes:   Option<u64>,
    pub duration_ns:  Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ProcessInfoEvent {
    pub process_id:        u32,
    pub parent_process_id: Option<u32>,
    pub name:              String,
    pub cmdline:           Option<String>,
    pub start_time_ns:     i64,
    pub exit_time_ns:      Option<i64>,
    pub exit_code:         Option<i32>,
}

#[derive(Debug, Clone)]
pub struct ThreadInfoEvent {
    pub thread_id:     u32,
    pub process_id:    u32,
    pub name:          Option<String>,
    pub start_time_ns: i64,
    pub exit_time_ns:  Option<i64>,
}

#[derive(Debug, Clone)]
pub struct FrameInfoEvent {
    pub frame_id:    u64,
    pub address:     u64,
    pub symbol_name: Option<String>,
    pub module_name: Option<String>,
    pub file_path:   Option<String>,
    pub line_number: Option<u32>,
}

/// A single exploded call-stack entry.
#[derive(Debug, Clone)]
pub struct StackEntryEvent {
    pub stack_id: u64,
    pub depth:    u32,
    pub frame_id: u64,
}

// ── top-level dispatch type ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum RawEvent {
    CpuSample(CpuSampleEvent),
    Scheduling(SchedulingEvent),
    DiskIo(DiskIoEvent),
    FileIo(FileIoEvent),
    Memory(MemoryEvent),
    Network(NetworkEvent),
    Process(ProcessInfoEvent),
    Thread(ThreadInfoEvent),
    Frame(FrameInfoEvent),
    StackEntry(StackEntryEvent),
}
