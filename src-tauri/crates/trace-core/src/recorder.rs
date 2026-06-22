use std::sync::Arc;

use arrow::array::{
    ArrayRef, FixedSizeBinaryBuilder, Int32Builder, Int64Builder, StringBuilder,
    UInt16Builder, UInt32Builder, UInt64Builder, UInt8Builder,
};
use arrow::array::RecordBatch;

use crate::{
    error::Result,
    event::*,
    schema,
    store::{TableKind, TraceStore},
};

/// Number of rows buffered before a row-group is flushed to the store.
/// 64 Ki rows ≈ 1–4 MiB per domain — small enough not to spike memory even at
/// 1 M+ events, large enough to compress well in Parquet.
pub const DEFAULT_BATCH_SIZE: usize = 65_536;

// ── public trait ─────────────────────────────────────────────────────────────

pub trait Recorder: Send {
    fn record(&mut self, event: RawEvent) -> Result<()>;

    /// Flush any pending buffered rows into the backing store as a new batch.
    fn flush(&mut self) -> Result<()>;
}

// ── StandardRecorder ─────────────────────────────────────────────────────────

/// Buffers `RawEvent`s into Arrow batches and streams them into a [`TraceStore`].
pub struct StandardRecorder {
    store:      TraceStore,
    batch_size: usize,
    // per-domain builders
    cpu:        CpuSampleBuilder,
    sched:      SchedulingBuilder,
    disk:       DiskIoBuilder,
    file:       FileIoBuilder,
    mem:        MemoryBuilder,
    net:        NetworkBuilder,
    proc:       ProcessBuilder,
    thr:        ThreadBuilder,
    frame:      FrameBuilder,
    stack:      StackBuilder,
}

impl StandardRecorder {
    pub fn new(store: TraceStore, batch_size: usize) -> Self {
        Self {
            store,
            batch_size,
            cpu:   CpuSampleBuilder::new(),
            sched: SchedulingBuilder::new(),
            disk:  DiskIoBuilder::new(),
            file:  FileIoBuilder::new(),
            mem:   MemoryBuilder::new(),
            net:   NetworkBuilder::new(),
            proc:  ProcessBuilder::new(),
            thr:   ThreadBuilder::new(),
            frame: FrameBuilder::new(),
            stack: StackBuilder::new(),
        }
    }

    /// Flush remaining buffers and return the completed [`TraceStore`].
    pub fn finish(mut self) -> Result<TraceStore> {
        self.flush()?;
        Ok(self.store)
    }

    fn maybe_flush_cpu(&mut self) -> Result<()> {
        if self.cpu.len() >= self.batch_size {
            let b = self.cpu.finish()?;
            self.store.append_batch(TableKind::CpuSamples, b)?;
        }
        Ok(())
    }
    fn maybe_flush_sched(&mut self) -> Result<()> {
        if self.sched.len() >= self.batch_size {
            let b = self.sched.finish()?;
            self.store.append_batch(TableKind::Scheduling, b)?;
        }
        Ok(())
    }
    fn maybe_flush_disk(&mut self) -> Result<()> {
        if self.disk.len() >= self.batch_size {
            let b = self.disk.finish()?;
            self.store.append_batch(TableKind::DiskIo, b)?;
        }
        Ok(())
    }
    fn maybe_flush_file(&mut self) -> Result<()> {
        if self.file.len() >= self.batch_size {
            let b = self.file.finish()?;
            self.store.append_batch(TableKind::FileIo, b)?;
        }
        Ok(())
    }
    fn maybe_flush_mem(&mut self) -> Result<()> {
        if self.mem.len() >= self.batch_size {
            let b = self.mem.finish()?;
            self.store.append_batch(TableKind::Memory, b)?;
        }
        Ok(())
    }
    fn maybe_flush_net(&mut self) -> Result<()> {
        if self.net.len() >= self.batch_size {
            let b = self.net.finish()?;
            self.store.append_batch(TableKind::Network, b)?;
        }
        Ok(())
    }
    fn maybe_flush_proc(&mut self) -> Result<()> {
        if self.proc.len() >= self.batch_size {
            let b = self.proc.finish()?;
            self.store.append_batch(TableKind::Processes, b)?;
        }
        Ok(())
    }
    fn maybe_flush_thr(&mut self) -> Result<()> {
        if self.thr.len() >= self.batch_size {
            let b = self.thr.finish()?;
            self.store.append_batch(TableKind::Threads, b)?;
        }
        Ok(())
    }
    fn maybe_flush_frame(&mut self) -> Result<()> {
        if self.frame.len() >= self.batch_size {
            let b = self.frame.finish()?;
            self.store.append_batch(TableKind::Frames, b)?;
        }
        Ok(())
    }
    fn maybe_flush_stack(&mut self) -> Result<()> {
        if self.stack.len() >= self.batch_size {
            let b = self.stack.finish()?;
            self.store.append_batch(TableKind::Stacks, b)?;
        }
        Ok(())
    }
}

impl Recorder for StandardRecorder {
    fn record(&mut self, event: RawEvent) -> Result<()> {
        match event {
            RawEvent::CpuSample(e)  => { self.cpu.push(&e);   self.maybe_flush_cpu()?; }
            RawEvent::Scheduling(e) => { self.sched.push(&e); self.maybe_flush_sched()?; }
            RawEvent::DiskIo(e)     => { self.disk.push(&e);  self.maybe_flush_disk()?; }
            RawEvent::FileIo(e)     => { self.file.push(&e);  self.maybe_flush_file()?; }
            RawEvent::Memory(e)     => { self.mem.push(&e);   self.maybe_flush_mem()?; }
            RawEvent::Network(e)    => { self.net.push(&e)?;  self.maybe_flush_net()?; }
            RawEvent::Process(e)    => { self.proc.push(&e);  self.maybe_flush_proc()?; }
            RawEvent::Thread(e)     => { self.thr.push(&e);   self.maybe_flush_thr()?; }
            RawEvent::Frame(e)      => { self.frame.push(&e); self.maybe_flush_frame()?; }
            RawEvent::StackEntry(e) => { self.stack.push(&e); self.maybe_flush_stack()?; }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.cpu.len() > 0 {
            let b = self.cpu.finish()?;
            self.store.append_batch(TableKind::CpuSamples, b)?;
        }
        if self.sched.len() > 0 {
            let b = self.sched.finish()?;
            self.store.append_batch(TableKind::Scheduling, b)?;
        }
        if self.disk.len() > 0 {
            let b = self.disk.finish()?;
            self.store.append_batch(TableKind::DiskIo, b)?;
        }
        if self.file.len() > 0 {
            let b = self.file.finish()?;
            self.store.append_batch(TableKind::FileIo, b)?;
        }
        if self.mem.len() > 0 {
            let b = self.mem.finish()?;
            self.store.append_batch(TableKind::Memory, b)?;
        }
        if self.net.len() > 0 {
            let b = self.net.finish()?;
            self.store.append_batch(TableKind::Network, b)?;
        }
        if self.proc.len() > 0 {
            let b = self.proc.finish()?;
            self.store.append_batch(TableKind::Processes, b)?;
        }
        if self.thr.len() > 0 {
            let b = self.thr.finish()?;
            self.store.append_batch(TableKind::Threads, b)?;
        }
        if self.frame.len() > 0 {
            let b = self.frame.finish()?;
            self.store.append_batch(TableKind::Frames, b)?;
        }
        if self.stack.len() > 0 {
            let b = self.stack.finish()?;
            self.store.append_batch(TableKind::Stacks, b)?;
        }
        Ok(())
    }
}

// ── per-domain batch builders ─────────────────────────────────────────────────

macro_rules! finish_batch {
    ($schema:expr, $( $col:expr ),+ $(,)?) => {{
        let arrays: Vec<ArrayRef> = vec![$( Arc::new($col) as ArrayRef ),+];
        RecordBatch::try_new($schema, arrays).map_err(crate::error::Error::Arrow)
    }};
}

// ── CpuSampleBuilder ─────────────────────────────────────────────────────────

struct CpuSampleBuilder {
    count:         usize,
    timestamp_ns:  Int64Builder,
    process_id:    UInt32Builder,
    thread_id:     UInt32Builder,
    cpu_id:        UInt32Builder,
    sample_weight: UInt64Builder,
    stack_id:      UInt64Builder,
}

impl CpuSampleBuilder {
    fn new() -> Self {
        Self {
            count: 0,
            timestamp_ns:  Int64Builder::new(),
            process_id:    UInt32Builder::new(),
            thread_id:     UInt32Builder::new(),
            cpu_id:        UInt32Builder::new(),
            sample_weight: UInt64Builder::new(),
            stack_id:      UInt64Builder::new(),
        }
    }

    fn len(&self) -> usize { self.count }

    fn push(&mut self, e: &CpuSampleEvent) {
        self.count += 1;
        self.timestamp_ns.append_value(e.timestamp_ns);
        self.process_id.append_value(e.process_id);
        self.thread_id.append_value(e.thread_id);
        self.cpu_id.append_value(e.cpu_id);
        self.sample_weight.append_value(e.sample_weight);
        self.stack_id.append_option(e.stack_id);
    }

    fn finish(&mut self) -> Result<RecordBatch> {
        self.count = 0;
        finish_batch!(
            schema::cpu_samples_schema(),
            self.timestamp_ns.finish(),
            self.process_id.finish(),
            self.thread_id.finish(),
            self.cpu_id.finish(),
            self.sample_weight.finish(),
            self.stack_id.finish(),
        )
    }
}

// ── SchedulingBuilder ────────────────────────────────────────────────────────

struct SchedulingBuilder {
    count:          usize,
    timestamp_ns:   Int64Builder,
    process_id:     UInt32Builder,
    thread_id:      UInt32Builder,
    cpu_id:         UInt32Builder,
    event_type:     UInt8Builder,
    prev_state:     UInt8Builder,
    next_process_id:UInt32Builder,
    next_thread_id: UInt32Builder,
    duration_ns:    UInt64Builder,
}

impl SchedulingBuilder {
    fn new() -> Self {
        Self {
            count: 0,
            timestamp_ns:    Int64Builder::new(),
            process_id:      UInt32Builder::new(),
            thread_id:       UInt32Builder::new(),
            cpu_id:          UInt32Builder::new(),
            event_type:      UInt8Builder::new(),
            prev_state:      UInt8Builder::new(),
            next_process_id: UInt32Builder::new(),
            next_thread_id:  UInt32Builder::new(),
            duration_ns:     UInt64Builder::new(),
        }
    }

    fn len(&self) -> usize { self.count }

    fn push(&mut self, e: &SchedulingEvent) {
        self.count += 1;
        self.timestamp_ns.append_value(e.timestamp_ns);
        self.process_id.append_value(e.process_id);
        self.thread_id.append_value(e.thread_id);
        self.cpu_id.append_value(e.cpu_id);
        self.event_type.append_value(e.event_type as u8);
        self.prev_state.append_option(e.prev_state);
        self.next_process_id.append_option(e.next_process_id);
        self.next_thread_id.append_option(e.next_thread_id);
        self.duration_ns.append_option(e.duration_ns);
    }

    fn finish(&mut self) -> Result<RecordBatch> {
        self.count = 0;
        finish_batch!(
            schema::scheduling_schema(),
            self.timestamp_ns.finish(),
            self.process_id.finish(),
            self.thread_id.finish(),
            self.cpu_id.finish(),
            self.event_type.finish(),
            self.prev_state.finish(),
            self.next_process_id.finish(),
            self.next_thread_id.finish(),
            self.duration_ns.finish(),
        )
    }
}

// ── DiskIoBuilder ────────────────────────────────────────────────────────────

struct DiskIoBuilder {
    count:        usize,
    timestamp_ns: Int64Builder,
    process_id:   UInt32Builder,
    thread_id:    UInt32Builder,
    operation:    UInt8Builder,
    device_major: UInt32Builder,
    device_minor: UInt32Builder,
    sector:       UInt64Builder,
    size_bytes:   UInt64Builder,
    duration_ns:  UInt64Builder,
}

impl DiskIoBuilder {
    fn new() -> Self {
        Self {
            count: 0,
            timestamp_ns: Int64Builder::new(),
            process_id:   UInt32Builder::new(),
            thread_id:    UInt32Builder::new(),
            operation:    UInt8Builder::new(),
            device_major: UInt32Builder::new(),
            device_minor: UInt32Builder::new(),
            sector:       UInt64Builder::new(),
            size_bytes:   UInt64Builder::new(),
            duration_ns:  UInt64Builder::new(),
        }
    }

    fn len(&self) -> usize { self.count }

    fn push(&mut self, e: &DiskIoEvent) {
        self.count += 1;
        self.timestamp_ns.append_value(e.timestamp_ns);
        self.process_id.append_value(e.process_id);
        self.thread_id.append_value(e.thread_id);
        self.operation.append_value(e.operation as u8);
        self.device_major.append_value(e.device_major);
        self.device_minor.append_value(e.device_minor);
        self.sector.append_value(e.sector);
        self.size_bytes.append_value(e.size_bytes);
        self.duration_ns.append_option(e.duration_ns);
    }

    fn finish(&mut self) -> Result<RecordBatch> {
        self.count = 0;
        finish_batch!(
            schema::disk_io_schema(),
            self.timestamp_ns.finish(),
            self.process_id.finish(),
            self.thread_id.finish(),
            self.operation.finish(),
            self.device_major.finish(),
            self.device_minor.finish(),
            self.sector.finish(),
            self.size_bytes.finish(),
            self.duration_ns.finish(),
        )
    }
}

// ── FileIoBuilder ────────────────────────────────────────────────────────────

struct FileIoBuilder {
    count:        usize,
    timestamp_ns: Int64Builder,
    process_id:   UInt32Builder,
    thread_id:    UInt32Builder,
    operation:    UInt8Builder,
    fd:           Int32Builder,
    path_hash:    UInt64Builder,
    offset_bytes: Int64Builder,
    size_bytes:   UInt64Builder,
    duration_ns:  UInt64Builder,
    return_value: Int64Builder,
}

impl FileIoBuilder {
    fn new() -> Self {
        Self {
            count: 0,
            timestamp_ns: Int64Builder::new(),
            process_id:   UInt32Builder::new(),
            thread_id:    UInt32Builder::new(),
            operation:    UInt8Builder::new(),
            fd:           Int32Builder::new(),
            path_hash:    UInt64Builder::new(),
            offset_bytes: Int64Builder::new(),
            size_bytes:   UInt64Builder::new(),
            duration_ns:  UInt64Builder::new(),
            return_value: Int64Builder::new(),
        }
    }

    fn len(&self) -> usize { self.count }

    fn push(&mut self, e: &FileIoEvent) {
        self.count += 1;
        self.timestamp_ns.append_value(e.timestamp_ns);
        self.process_id.append_value(e.process_id);
        self.thread_id.append_value(e.thread_id);
        self.operation.append_value(e.operation as u8);
        self.fd.append_option(e.fd);
        self.path_hash.append_option(e.path_hash);
        self.offset_bytes.append_option(e.offset_bytes);
        self.size_bytes.append_option(e.size_bytes);
        self.duration_ns.append_option(e.duration_ns);
        self.return_value.append_option(e.return_value);
    }

    fn finish(&mut self) -> Result<RecordBatch> {
        self.count = 0;
        finish_batch!(
            schema::file_io_schema(),
            self.timestamp_ns.finish(),
            self.process_id.finish(),
            self.thread_id.finish(),
            self.operation.finish(),
            self.fd.finish(),
            self.path_hash.finish(),
            self.offset_bytes.finish(),
            self.size_bytes.finish(),
            self.duration_ns.finish(),
            self.return_value.finish(),
        )
    }
}

// ── MemoryBuilder ────────────────────────────────────────────────────────────

struct MemoryBuilder {
    count:        usize,
    timestamp_ns: Int64Builder,
    process_id:   UInt32Builder,
    thread_id:    UInt32Builder,
    event_type:   UInt8Builder,
    address:      UInt64Builder,
    size_bytes:   UInt64Builder,
    numa_node:    UInt32Builder,
}

impl MemoryBuilder {
    fn new() -> Self {
        Self {
            count: 0,
            timestamp_ns: Int64Builder::new(),
            process_id:   UInt32Builder::new(),
            thread_id:    UInt32Builder::new(),
            event_type:   UInt8Builder::new(),
            address:      UInt64Builder::new(),
            size_bytes:   UInt64Builder::new(),
            numa_node:    UInt32Builder::new(),
        }
    }

    fn len(&self) -> usize { self.count }

    fn push(&mut self, e: &MemoryEvent) {
        self.count += 1;
        self.timestamp_ns.append_value(e.timestamp_ns);
        self.process_id.append_value(e.process_id);
        self.thread_id.append_option(e.thread_id);
        self.event_type.append_value(e.event_type as u8);
        self.address.append_option(e.address);
        self.size_bytes.append_option(e.size_bytes);
        self.numa_node.append_option(e.numa_node);
    }

    fn finish(&mut self) -> Result<RecordBatch> {
        self.count = 0;
        finish_batch!(
            schema::memory_schema(),
            self.timestamp_ns.finish(),
            self.process_id.finish(),
            self.thread_id.finish(),
            self.event_type.finish(),
            self.address.finish(),
            self.size_bytes.finish(),
            self.numa_node.finish(),
        )
    }
}

// ── NetworkBuilder ───────────────────────────────────────────────────────────

struct NetworkBuilder {
    count:        usize,
    timestamp_ns: Int64Builder,
    process_id:   UInt32Builder,
    thread_id:    UInt32Builder,
    operation:    UInt8Builder,
    protocol:     UInt8Builder,
    fd:           Int32Builder,
    local_addr:   FixedSizeBinaryBuilder,
    remote_addr:  FixedSizeBinaryBuilder,
    local_port:   UInt16Builder,
    remote_port:  UInt16Builder,
    size_bytes:   UInt64Builder,
    duration_ns:  UInt64Builder,
}

impl NetworkBuilder {
    fn new() -> Self {
        Self {
            count: 0,
            timestamp_ns: Int64Builder::new(),
            process_id:   UInt32Builder::new(),
            thread_id:    UInt32Builder::new(),
            operation:    UInt8Builder::new(),
            protocol:     UInt8Builder::new(),
            fd:           Int32Builder::new(),
            local_addr:   FixedSizeBinaryBuilder::with_capacity(0, 16),
            remote_addr:  FixedSizeBinaryBuilder::with_capacity(0, 16),
            local_port:   UInt16Builder::new(),
            remote_port:  UInt16Builder::new(),
            size_bytes:   UInt64Builder::new(),
            duration_ns:  UInt64Builder::new(),
        }
    }

    fn len(&self) -> usize { self.count }

    fn push(&mut self, e: &NetworkEvent) -> Result<()> {
        self.count += 1;
        self.timestamp_ns.append_value(e.timestamp_ns);
        self.process_id.append_value(e.process_id);
        self.thread_id.append_option(e.thread_id);
        self.operation.append_value(e.operation as u8);
        self.protocol.append_value(e.protocol as u8);
        self.fd.append_option(e.fd);
        match &e.local_addr {
            Some(a) => self.local_addr.append_value(a)?,
            None    => self.local_addr.append_null(),
        }
        match &e.remote_addr {
            Some(a) => self.remote_addr.append_value(a)?,
            None    => self.remote_addr.append_null(),
        }
        self.local_port.append_option(e.local_port);
        self.remote_port.append_option(e.remote_port);
        self.size_bytes.append_option(e.size_bytes);
        self.duration_ns.append_option(e.duration_ns);
        Ok(())
    }

    fn finish(&mut self) -> Result<RecordBatch> {
        self.count = 0;
        finish_batch!(
            schema::network_schema(),
            self.timestamp_ns.finish(),
            self.process_id.finish(),
            self.thread_id.finish(),
            self.operation.finish(),
            self.protocol.finish(),
            self.fd.finish(),
            self.local_addr.finish(),
            self.remote_addr.finish(),
            self.local_port.finish(),
            self.remote_port.finish(),
            self.size_bytes.finish(),
            self.duration_ns.finish(),
        )
    }
}

// ── ProcessBuilder ───────────────────────────────────────────────────────────

struct ProcessBuilder {
    count:             usize,
    process_id:        UInt32Builder,
    parent_process_id: UInt32Builder,
    name:              StringBuilder,
    cmdline:           StringBuilder,
    start_time_ns:     Int64Builder,
    exit_time_ns:      Int64Builder,
    exit_code:         Int32Builder,
}

impl ProcessBuilder {
    fn new() -> Self {
        Self {
            count: 0,
            process_id:        UInt32Builder::new(),
            parent_process_id: UInt32Builder::new(),
            name:              StringBuilder::new(),
            cmdline:           StringBuilder::new(),
            start_time_ns:     Int64Builder::new(),
            exit_time_ns:      Int64Builder::new(),
            exit_code:         Int32Builder::new(),
        }
    }

    fn len(&self) -> usize { self.count }

    fn push(&mut self, e: &ProcessInfoEvent) {
        self.count += 1;
        self.process_id.append_value(e.process_id);
        self.parent_process_id.append_option(e.parent_process_id);
        self.name.append_value(&e.name);
        self.cmdline.append_option(e.cmdline.as_deref());
        self.start_time_ns.append_value(e.start_time_ns);
        self.exit_time_ns.append_option(e.exit_time_ns);
        self.exit_code.append_option(e.exit_code);
    }

    fn finish(&mut self) -> Result<RecordBatch> {
        self.count = 0;
        finish_batch!(
            schema::processes_schema(),
            self.process_id.finish(),
            self.parent_process_id.finish(),
            self.name.finish(),
            self.cmdline.finish(),
            self.start_time_ns.finish(),
            self.exit_time_ns.finish(),
            self.exit_code.finish(),
        )
    }
}

// ── ThreadBuilder ────────────────────────────────────────────────────────────

struct ThreadBuilder {
    count:         usize,
    thread_id:     UInt32Builder,
    process_id:    UInt32Builder,
    name:          StringBuilder,
    start_time_ns: Int64Builder,
    exit_time_ns:  Int64Builder,
}

impl ThreadBuilder {
    fn new() -> Self {
        Self {
            count: 0,
            thread_id:     UInt32Builder::new(),
            process_id:    UInt32Builder::new(),
            name:          StringBuilder::new(),
            start_time_ns: Int64Builder::new(),
            exit_time_ns:  Int64Builder::new(),
        }
    }

    fn len(&self) -> usize { self.count }

    fn push(&mut self, e: &ThreadInfoEvent) {
        self.count += 1;
        self.thread_id.append_value(e.thread_id);
        self.process_id.append_value(e.process_id);
        self.name.append_option(e.name.as_deref());
        self.start_time_ns.append_value(e.start_time_ns);
        self.exit_time_ns.append_option(e.exit_time_ns);
    }

    fn finish(&mut self) -> Result<RecordBatch> {
        self.count = 0;
        finish_batch!(
            schema::threads_schema(),
            self.thread_id.finish(),
            self.process_id.finish(),
            self.name.finish(),
            self.start_time_ns.finish(),
            self.exit_time_ns.finish(),
        )
    }
}

// ── FrameBuilder ─────────────────────────────────────────────────────────────

struct FrameBuilder {
    count:       usize,
    frame_id:    UInt64Builder,
    address:     UInt64Builder,
    symbol_name: StringBuilder,
    module_name: StringBuilder,
    file_path:   StringBuilder,
    line_number: UInt32Builder,
}

impl FrameBuilder {
    fn new() -> Self {
        Self {
            count: 0,
            frame_id:    UInt64Builder::new(),
            address:     UInt64Builder::new(),
            symbol_name: StringBuilder::new(),
            module_name: StringBuilder::new(),
            file_path:   StringBuilder::new(),
            line_number: UInt32Builder::new(),
        }
    }

    fn len(&self) -> usize { self.count }

    fn push(&mut self, e: &FrameInfoEvent) {
        self.count += 1;
        self.frame_id.append_value(e.frame_id);
        self.address.append_value(e.address);
        self.symbol_name.append_option(e.symbol_name.as_deref());
        self.module_name.append_option(e.module_name.as_deref());
        self.file_path.append_option(e.file_path.as_deref());
        self.line_number.append_option(e.line_number);
    }

    fn finish(&mut self) -> Result<RecordBatch> {
        self.count = 0;
        finish_batch!(
            schema::frames_schema(),
            self.frame_id.finish(),
            self.address.finish(),
            self.symbol_name.finish(),
            self.module_name.finish(),
            self.file_path.finish(),
            self.line_number.finish(),
        )
    }
}

// ── StackBuilder ─────────────────────────────────────────────────────────────

struct StackBuilder {
    count:    usize,
    stack_id: UInt64Builder,
    depth:    UInt32Builder,
    frame_id: UInt64Builder,
}

impl StackBuilder {
    fn new() -> Self {
        Self {
            count: 0,
            stack_id: UInt64Builder::new(),
            depth:    UInt32Builder::new(),
            frame_id: UInt64Builder::new(),
        }
    }

    fn len(&self) -> usize { self.count }

    fn push(&mut self, e: &StackEntryEvent) {
        self.count += 1;
        self.stack_id.append_value(e.stack_id);
        self.depth.append_value(e.depth);
        self.frame_id.append_value(e.frame_id);
    }

    fn finish(&mut self) -> Result<RecordBatch> {
        self.count = 0;
        finish_batch!(
            schema::stacks_schema(),
            self.stack_id.finish(),
            self.depth.finish(),
            self.frame_id.finish(),
        )
    }
}
