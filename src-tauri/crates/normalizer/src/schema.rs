use arrow_schema::{DataType, Field, Schema, TimeUnit};
use std::sync::Arc;

/// Event-type constants stored in the `event_type` column.
pub mod event_type {
    pub const PROCESS_CREATE: u8 = 1;
    pub const PROCESS_EXIT: u8 = 2;
    pub const THREAD_CREATE: u8 = 3;
    pub const THREAD_EXIT: u8 = 4;
    pub const STACK_SAMPLE: u8 = 5;
    pub const CONTEXT_SWITCH: u8 = 6;
    pub const FILE_READ: u8 = 7;
    pub const FILE_WRITE: u8 = 8;
    pub const SYSCALL_ENTER: u8 = 9;
    pub const SYSCALL_EXIT: u8 = 10;
    pub const MODULE_LOAD: u8 = 11;
    pub const MODULE_UNLOAD: u8 = 12;
}

/// Canonical Arrow schema for normalized trace events.
///
/// All events share these columns; event-specific fields are nullable and only
/// populated for the relevant event types.
///
/// Column layout:
///   timestamp_ns  – corrected monotonic nanosecond timestamp (i64 nanos since epoch 0)
///   event_type    – u8 discriminant (see event_type consts above)
///   pid           – u32 process ID
///   tid           – u32 thread ID (0 if not applicable)
///   stack_id      – u32 interned stack ID (0 = no stack)
///   aux_u64       – u64 general auxiliary field (bytes for I/O, RVA for modules, etc.)
///   aux_i64       – i64 general auxiliary field (exit code, syscall ret, etc.)
///   aux_u32       – u32 general auxiliary field (syscall nr, cpu, etc.)
///   name          – utf8 optional string (process/thread name, module path)
pub fn trace_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new(
            "timestamp_ns",
            DataType::Timestamp(TimeUnit::Nanosecond, None),
            false,
        ),
        Field::new("event_type", DataType::UInt8, false),
        Field::new("pid", DataType::UInt32, false),
        Field::new("tid", DataType::UInt32, false),
        Field::new("stack_id", DataType::UInt32, false),
        Field::new("aux_u64", DataType::UInt64, true),
        Field::new("aux_i64", DataType::Int64, true),
        Field::new("aux_u32", DataType::UInt32, true),
        Field::new("name", DataType::Utf8, true),
    ]))
}
