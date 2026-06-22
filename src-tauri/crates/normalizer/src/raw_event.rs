/// Contract definition for events produced by trace-core capture backends.
///
/// This is the canonical interface that normalizer consumes.  When trace-core
/// lands its implementation it must match this definition (or a migration will
/// be required and coordinated through the director).
///
/// All timestamps are in nanoseconds since an arbitrary epoch established by
/// the capture backend.  The normalizer corrects them to a single monotonic base.
#[derive(Debug, Clone)]
pub enum RawEvent {
    // ── Process / thread lifecycle ─────────────────────────────────────────
    ProcessCreate {
        pid: u32,
        ppid: u32,
        name: String,
        timestamp_ns: u64,
    },
    ProcessExit {
        pid: u32,
        exit_code: i32,
        timestamp_ns: u64,
    },
    ThreadCreate {
        pid: u32,
        tid: u32,
        name: Option<String>,
        timestamp_ns: u64,
    },
    ThreadExit {
        pid: u32,
        tid: u32,
        timestamp_ns: u64,
    },

    // ── CPU sampling / stack walks ─────────────────────────────────────────
    StackSample {
        pid: u32,
        tid: u32,
        /// Raw virtual addresses, innermost frame first.
        frames: Vec<u64>,
        timestamp_ns: u64,
        /// CPU index the sample was taken on.
        cpu: Option<u32>,
    },

    // ── Context switches ───────────────────────────────────────────────────
    ContextSwitch {
        /// Thread being switched off.
        prev_pid: u32,
        prev_tid: u32,
        /// Thread being switched on.
        next_pid: u32,
        next_tid: u32,
        timestamp_ns: u64,
        cpu: Option<u32>,
    },

    // ── I/O ────────────────────────────────────────────────────────────────
    FileRead {
        pid: u32,
        tid: u32,
        bytes: u64,
        timestamp_ns: u64,
    },
    FileWrite {
        pid: u32,
        tid: u32,
        bytes: u64,
        timestamp_ns: u64,
    },

    // ── System calls ───────────────────────────────────────────────────────
    SyscallEnter {
        pid: u32,
        tid: u32,
        /// Platform-specific syscall number.
        nr: u32,
        timestamp_ns: u64,
    },
    SyscallExit {
        pid: u32,
        tid: u32,
        nr: u32,
        ret: i64,
        timestamp_ns: u64,
    },

    // ── Module load / unload ───────────────────────────────────────────────
    ModuleLoad {
        pid: u32,
        base: u64,
        size: u64,
        path: String,
        /// GNU build-id or PE TimeDateStamp, if available.
        build_id: Option<Vec<u8>>,
        timestamp_ns: u64,
    },
    ModuleUnload {
        pid: u32,
        base: u64,
        timestamp_ns: u64,
    },

    // ── Fallthrough ────────────────────────────────────────────────────────
    /// Any event type the normalizer doesn't recognise; counted and dropped.
    Unknown {
        event_type: u32,
        timestamp_ns: u64,
        payload: Vec<u8>,
    },
}

impl RawEvent {
    /// Extract the timestamp from any event variant.
    pub fn timestamp_ns(&self) -> u64 {
        match self {
            RawEvent::ProcessCreate { timestamp_ns, .. }
            | RawEvent::ProcessExit { timestamp_ns, .. }
            | RawEvent::ThreadCreate { timestamp_ns, .. }
            | RawEvent::ThreadExit { timestamp_ns, .. }
            | RawEvent::StackSample { timestamp_ns, .. }
            | RawEvent::ContextSwitch { timestamp_ns, .. }
            | RawEvent::FileRead { timestamp_ns, .. }
            | RawEvent::FileWrite { timestamp_ns, .. }
            | RawEvent::SyscallEnter { timestamp_ns, .. }
            | RawEvent::SyscallExit { timestamp_ns, .. }
            | RawEvent::ModuleLoad { timestamp_ns, .. }
            | RawEvent::ModuleUnload { timestamp_ns, .. }
            | RawEvent::Unknown { timestamp_ns, .. } => *timestamp_ns,
        }
    }
}
