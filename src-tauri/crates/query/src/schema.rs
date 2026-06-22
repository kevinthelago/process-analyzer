use std::sync::{Arc, LazyLock};

use arrow::{
    datatypes::{DataType, Field, Schema, SchemaRef},
    record_batch::RecordBatch,
};

// ── Canonical schemas (mirror trace-core) ────────────────────────────────────
// These must remain in sync with trace-core::schema.  If trace-core changes a
// schema, update here too and update the affected SQL queries.

static CPU_SAMPLES: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("timestamp_ns",  DataType::Int64,  false),
        Field::new("process_id",    DataType::UInt32, false),
        Field::new("thread_id",     DataType::UInt32, false),
        Field::new("cpu_id",        DataType::UInt32, false),
        Field::new("sample_weight", DataType::UInt64, false),
        Field::new("stack_id",      DataType::UInt64, true),
    ]))
});

static STACKS: LazyLock<SchemaRef> = LazyLock::new(|| {
    // Exploded call stacks: (stack_id, depth, frame_id).
    // depth 0 = leaf (innermost / currently executing frame).
    Arc::new(Schema::new(vec![
        Field::new("stack_id", DataType::UInt64, false),
        Field::new("depth",    DataType::UInt32, false),
        Field::new("frame_id", DataType::UInt64, false),
    ]))
});

static FRAMES: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("frame_id",    DataType::UInt64, false),
        Field::new("address",     DataType::UInt64, false),
        Field::new("symbol_name", DataType::Utf8,   true),
        Field::new("module_name", DataType::Utf8,   true),
        Field::new("file_path",   DataType::Utf8,   true),
        Field::new("line_number", DataType::UInt32, true),
    ]))
});

static SCHEDULING: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("timestamp_ns",    DataType::Int64,  false),
        Field::new("process_id",      DataType::UInt32, false),
        Field::new("thread_id",       DataType::UInt32, false),
        Field::new("cpu_id",          DataType::UInt32, false),
        Field::new("event_type",      DataType::UInt8,  false),
        Field::new("prev_state",      DataType::UInt8,  true),
        Field::new("next_process_id", DataType::UInt32, true),
        Field::new("next_thread_id",  DataType::UInt32, true),
        Field::new("duration_ns",     DataType::UInt64, true),
    ]))
});

static DISK_IO: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("timestamp_ns", DataType::Int64,  false),
        Field::new("process_id",   DataType::UInt32, false),
        Field::new("thread_id",    DataType::UInt32, false),
        Field::new("operation",    DataType::UInt8,  false),
        Field::new("device_major", DataType::UInt32, false),
        Field::new("device_minor", DataType::UInt32, false),
        Field::new("sector",       DataType::UInt64, false),
        Field::new("size_bytes",   DataType::UInt64, false),
        Field::new("duration_ns",  DataType::UInt64, true),
    ]))
});

static FILE_IO: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("timestamp_ns",  DataType::Int64,  false),
        Field::new("process_id",    DataType::UInt32, false),
        Field::new("thread_id",     DataType::UInt32, false),
        Field::new("operation",     DataType::UInt8,  false),
        Field::new("fd",            DataType::Int32,  true),
        Field::new("path_hash",     DataType::UInt64, true),
        Field::new("offset_bytes",  DataType::Int64,  true),
        Field::new("size_bytes",    DataType::UInt64, true),
        Field::new("duration_ns",   DataType::UInt64, true),
        Field::new("return_value",  DataType::Int64,  true),
    ]))
});

static MEMORY: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("timestamp_ns", DataType::Int64,  false),
        Field::new("process_id",   DataType::UInt32, false),
        Field::new("thread_id",    DataType::UInt32, true),
        Field::new("event_type",   DataType::UInt8,  false),
        Field::new("address",      DataType::UInt64, true),
        Field::new("size_bytes",   DataType::UInt64, true),
        Field::new("numa_node",    DataType::UInt32, true),
    ]))
});

static PROCESSES: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("process_id",        DataType::UInt32, false),
        Field::new("parent_process_id", DataType::UInt32, true),
        Field::new("name",              DataType::Utf8,   false),
        Field::new("cmdline",           DataType::Utf8,   true),
        Field::new("start_time_ns",     DataType::Int64,  false),
        Field::new("exit_time_ns",      DataType::Int64,  true),
        Field::new("exit_code",         DataType::Int32,  true),
    ]))
});

static THREADS: LazyLock<SchemaRef> = LazyLock::new(|| {
    Arc::new(Schema::new(vec![
        Field::new("thread_id",     DataType::UInt32, false),
        Field::new("process_id",    DataType::UInt32, false),
        Field::new("name",          DataType::Utf8,   true),
        Field::new("start_time_ns", DataType::Int64,  false),
        Field::new("exit_time_ns",  DataType::Int64,  true),
    ]))
});

// ── Public schema accessors ───────────────────────────────────────────────────

pub fn cpu_samples_schema() -> SchemaRef { CPU_SAMPLES.clone() }
pub fn stacks_schema()       -> SchemaRef { STACKS.clone() }
pub fn frames_schema()       -> SchemaRef { FRAMES.clone() }
pub fn scheduling_schema()   -> SchemaRef { SCHEDULING.clone() }
pub fn disk_io_schema()      -> SchemaRef { DISK_IO.clone() }
pub fn file_io_schema()      -> SchemaRef { FILE_IO.clone() }
pub fn memory_schema()       -> SchemaRef { MEMORY.clone() }
pub fn processes_schema()    -> SchemaRef { PROCESSES.clone() }
pub fn threads_schema()      -> SchemaRef { THREADS.clone() }

// ── TraceStore trait ──────────────────────────────────────────────────────────

/// The contract that `trace-core::TraceStore` must satisfy.
///
/// Defined here so the `query`, `analysis`, and `diff` crates can be built and
/// tested in parallel with the `trace-core` stream.  At integration time,
/// `trace-core` adds an `impl query::TraceStore for trace_core::TraceStore`
/// (or a thin newtype wrapper) that delegates to `batches(TableKind)`.
pub trait TraceStore: Send + Sync {
    fn cpu_samples(&self) -> &[RecordBatch];
    fn stacks(&self) -> &[RecordBatch];
    fn frames(&self) -> &[RecordBatch];
    fn scheduling(&self) -> &[RecordBatch];
    fn disk_io(&self) -> &[RecordBatch];
    fn file_io(&self) -> &[RecordBatch];
    fn memory(&self) -> &[RecordBatch];
    fn processes(&self) -> &[RecordBatch];
    fn threads(&self) -> &[RecordBatch];

    /// Wall-clock duration of the recorded trace in nanoseconds.
    fn duration_ns(&self) -> u64;
}
