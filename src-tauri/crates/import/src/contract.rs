/// A single raw event emitted by a capture backend or import adapter.
///
/// NOTE: This is a contract stub owned by this crate until `pa-trace-core` lands.
/// When that crate ships, both `pa-import` and `pa-recorder` switch to:
///   `use pa_trace_core::{RawEvent, RawEventKind};`
/// and this module is removed. Keep this in sync with the trace-core stream's contract file.
#[derive(Debug, Clone)]
pub struct RawEvent {
    /// Nanoseconds since Unix epoch (CLOCK_REALTIME).
    /// For ETL files, converted from Windows FILETIME (100-ns ticks since 1601-01-01).
    pub timestamp_ns: u64,
    pub pid: u32,
    pub tid: u32,
    /// CPU index; None when not recorded by the source format.
    pub cpu: Option<u16>,
    pub kind: RawEventKind,
    /// Raw payload bytes. Interpretation is format-specific;
    /// the normalizer (trace-core) decodes these into Arrow columns.
    pub payload: bytes::Bytes,
}

#[derive(Debug, Clone)]
pub enum RawEventKind {
    /// Scheduler switched another thread onto this CPU.
    ContextSwitchIn { next_pid: u32, next_tid: u32 },
    /// Scheduler preempted or blocked the running thread.
    ContextSwitchOut { prev_pid: u32, prev_tid: u32 },
    /// CPU performance counter sample.
    PerfSample {
        /// Instruction pointer at sample time.
        ip: u64,
        /// Return-address call stack (innermost first), empty if not captured.
        call_stack: Vec<u64>,
    },
    IoBegin {
        file_path: Option<String>,
        size_bytes: u64,
    },
    IoEnd {
        bytes_transferred: u64,
        latency_ns: u64,
    },
    ProcessCreate {
        parent_pid: u32,
        name: String,
    },
    ProcessExit {
        exit_code: i32,
    },
    ThreadCreate {
        name: Option<String>,
    },
    ThreadExit,
    /// ETW-sourced event (Windows ETL import).
    EtwEvent {
        /// Provider GUID (16 bytes, little-endian field order).
        provider: [u8; 16],
        opcode: u8,
        version: u8,
        event_id: u16,
        level: u8,
    },
    /// Unclassified event from a backend; normalizer decides how to handle it.
    Raw {
        provider_tag: u32,
        opcode: u32,
    },
}
