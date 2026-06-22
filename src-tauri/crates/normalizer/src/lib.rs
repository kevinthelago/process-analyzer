mod batch_builder;
mod clock;
mod error;
mod intern;
pub mod raw_event;
pub mod schema;

pub use error::NormalizerError;
pub use raw_event::RawEvent;
pub use schema::trace_schema;

use batch_builder::BatchBuilder;
use clock::ClockCorrector;
use intern::StackInterner;
use schema::event_type;
use symbolicator::{ModuleEntry, Symbolicator};

use arrow_array::RecordBatch;
use std::path::PathBuf;
use std::sync::Arc;

/// Default number of rows per emitted RecordBatch.
pub const DEFAULT_BATCH_SIZE: usize = 4_096;

/// Statistics accumulated since the last reset.
#[derive(Debug, Default, Clone)]
pub struct NormalizerStats {
    pub events_processed: u64,
    pub events_dropped: u64,
    pub batches_emitted: u64,
    pub clock_corrections: u64,
    pub stacks_interned: u64,
}

/// Streaming normalizer: converts `RawEvent`s into Arrow `RecordBatch`es.
///
/// Call `push()` for each event; it returns a batch when the internal buffer
/// fills.  Call `flush()` at end-of-stream to emit any remaining rows.
///
/// One normalizer instance is per-process (or per-capture-session): it owns a
/// `Symbolicator` and a clock corrector per session, so timestamps and module
/// maps are coherent within the session.
pub struct Normalizer {
    symbolicator: Symbolicator,
    clock: ClockCorrector,
    stacks: StackInterner,
    builder: BatchBuilder,
    stats: NormalizerStats,
}

impl Normalizer {
    pub fn new() -> Self {
        Self::with_batch_size(DEFAULT_BATCH_SIZE)
    }

    pub fn with_batch_size(batch_size: usize) -> Self {
        let schema = Arc::new(trace_schema().as_ref().clone());
        Self {
            symbolicator: Symbolicator::new(),
            clock: ClockCorrector::new(),
            stacks: StackInterner::new(),
            builder: BatchBuilder::new(schema, batch_size),
            stats: NormalizerStats::default(),
        }
    }

    /// Feed one event.  Returns `Some(batch)` when the internal buffer is full.
    pub fn push(&mut self, event: RawEvent) -> Result<Option<RecordBatch>, NormalizerError> {
        self.stats.events_processed += 1;
        let batch = self.dispatch(event)?;
        if batch.is_some() {
            self.stats.batches_emitted += 1;
            self.stats.clock_corrections = self.clock.corrections;
        }
        Ok(batch)
    }

    /// Flush any buffered rows.  Returns `Some(batch)` if there were any.
    pub fn flush(&mut self) -> Result<Option<RecordBatch>, NormalizerError> {
        let batch = self.builder.flush()?;
        if batch.is_some() {
            self.stats.batches_emitted += 1;
        }
        self.stats.clock_corrections = self.clock.corrections;
        Ok(batch)
    }

    pub fn stats(&self) -> NormalizerStats {
        let mut s = self.stats.clone();
        s.clock_corrections = self.clock.corrections;
        s.stacks_interned = self.stacks.len() as u64;
        s
    }

    // ──────────────────────────────────────────────────────────────────────
    // Internal dispatch
    // ──────────────────────────────────────────────────────────────────────

    fn dispatch(&mut self, event: RawEvent) -> Result<Option<RecordBatch>, NormalizerError> {
        use RawEvent::*;
        match event {
            ProcessCreate { pid, ppid: _, name, timestamp_ns } => {
                let ts = self.correct(timestamp_ns);
                self.append(ts, event_type::PROCESS_CREATE, pid, 0, 0, None, None, None, Some(&name))
            }
            ProcessExit { pid, exit_code, timestamp_ns } => {
                let ts = self.correct(timestamp_ns);
                self.append(ts, event_type::PROCESS_EXIT, pid, 0, 0, None, Some(exit_code as i64), None, None)
            }
            ThreadCreate { pid, tid, name, timestamp_ns } => {
                let ts = self.correct(timestamp_ns);
                self.append(ts, event_type::THREAD_CREATE, pid, tid, 0, None, None, None, name.as_deref())
            }
            ThreadExit { pid, tid, timestamp_ns } => {
                let ts = self.correct(timestamp_ns);
                self.append(ts, event_type::THREAD_EXIT, pid, tid, 0, None, None, None, None)
            }
            StackSample { pid, tid, frames, timestamp_ns, cpu } => {
                let ts = self.correct(timestamp_ns);
                let stack_id = self.stacks.intern(&frames);
                self.append(ts, event_type::STACK_SAMPLE, pid, tid, stack_id, None, None, cpu, None)
            }
            ContextSwitch { prev_pid, prev_tid, next_pid: _, next_tid: _, timestamp_ns, cpu } => {
                let ts = self.correct(timestamp_ns);
                self.append(ts, event_type::CONTEXT_SWITCH, prev_pid, prev_tid, 0, None, None, cpu, None)
            }
            FileRead { pid, tid, bytes, timestamp_ns } => {
                let ts = self.correct(timestamp_ns);
                self.append(ts, event_type::FILE_READ, pid, tid, 0, Some(bytes), None, None, None)
            }
            FileWrite { pid, tid, bytes, timestamp_ns } => {
                let ts = self.correct(timestamp_ns);
                self.append(ts, event_type::FILE_WRITE, pid, tid, 0, Some(bytes), None, None, None)
            }
            SyscallEnter { pid, tid, nr, timestamp_ns } => {
                let ts = self.correct(timestamp_ns);
                self.append(ts, event_type::SYSCALL_ENTER, pid, tid, 0, None, None, Some(nr), None)
            }
            SyscallExit { pid, tid, nr, ret, timestamp_ns } => {
                let ts = self.correct(timestamp_ns);
                self.append(ts, event_type::SYSCALL_EXIT, pid, tid, 0, None, Some(ret), Some(nr), None)
            }
            ModuleLoad { pid, base, size, path, build_id, timestamp_ns } => {
                let ts = self.correct(timestamp_ns);
                // Register with the symbolicator.
                self.symbolicator.add_module(ModuleEntry {
                    base,
                    size,
                    path: PathBuf::from(&path),
                    build_id,
                });
                self.append(ts, event_type::MODULE_LOAD, pid, 0, 0, Some(base), None, None, Some(&path))
            }
            ModuleUnload { pid, base, timestamp_ns } => {
                let ts = self.correct(timestamp_ns);
                self.symbolicator.remove_module(base);
                self.append(ts, event_type::MODULE_UNLOAD, pid, 0, 0, Some(base), None, None, None)
            }
            Unknown { .. } => {
                self.stats.events_dropped += 1;
                Ok(None)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn append(
        &mut self,
        ts: i64,
        etype: u8,
        pid: u32,
        tid: u32,
        stack_id: u32,
        aux_u64: Option<u64>,
        aux_i64: Option<i64>,
        aux_u32: Option<u32>,
        name: Option<&str>,
    ) -> Result<Option<RecordBatch>, NormalizerError> {
        self.builder.append(ts, etype, pid, tid, stack_id, aux_u64, aux_i64, aux_u32, name)
    }

    fn correct(&mut self, ts: u64) -> i64 {
        self.clock.correct(ts) as i64
    }
}

impl Default for Normalizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stack_sample(pid: u32, tid: u32, ts: u64) -> RawEvent {
        RawEvent::StackSample {
            pid,
            tid,
            frames: vec![0x1000, 0x2000],
            timestamp_ns: ts,
            cpu: Some(0),
        }
    }

    #[test]
    fn unknown_events_are_dropped() {
        let mut n = Normalizer::new();
        n.push(RawEvent::Unknown {
            event_type: 99,
            timestamp_ns: 1000,
            payload: vec![],
        })
        .unwrap();
        assert_eq!(n.stats().events_dropped, 1);
        assert_eq!(n.stats().events_processed, 1);
    }

    #[test]
    fn flush_empty_returns_none() {
        let mut n = Normalizer::new();
        assert!(n.flush().unwrap().is_none());
    }

    #[test]
    fn flush_returns_batch_when_rows_buffered() {
        let mut n = Normalizer::new();
        n.push(make_stack_sample(1, 2, 1000)).unwrap();
        let batch = n.flush().unwrap();
        assert!(batch.is_some());
        assert_eq!(batch.unwrap().num_rows(), 1);
    }

    #[test]
    fn batch_emitted_at_capacity() {
        let mut n = Normalizer::with_batch_size(4);
        for i in 0..4 {
            let result = n.push(make_stack_sample(1, 1, 1000 + i as u64)).unwrap();
            if i < 3 {
                assert!(result.is_none(), "batch should not emit before capacity");
            } else {
                assert!(result.is_some(), "batch should emit at capacity");
            }
        }
    }

    #[test]
    fn clock_correction_counted() {
        let mut n = Normalizer::new();
        n.push(make_stack_sample(1, 1, 2000)).unwrap();
        n.push(make_stack_sample(1, 1, 1000)).unwrap(); // backwards
        assert_eq!(n.stats().clock_corrections, 1);
    }

    #[test]
    fn stacks_interned() {
        let mut n = Normalizer::new();
        n.push(make_stack_sample(1, 1, 1000)).unwrap();
        n.push(make_stack_sample(1, 1, 2000)).unwrap(); // same frames
        assert_eq!(n.stats().stacks_interned, 1); // should be interned as one
    }

    #[test]
    fn module_load_registers_with_symbolicator() {
        let mut n = Normalizer::new();
        n.push(RawEvent::ModuleLoad {
            pid: 1,
            base: 0x40_0000,
            size: 0x10_0000,
            path: "/nonexistent/libtest.so".to_owned(),
            build_id: None,
            timestamp_ns: 1000,
        })
        .unwrap();
        // The module is registered; it degrades gracefully since the file doesn't exist.
        // Just verify no panic/error.
        assert_eq!(n.stats().events_processed, 1);
    }

    #[test]
    fn stats_batch_count() {
        let mut n = Normalizer::with_batch_size(2);
        n.push(make_stack_sample(1, 1, 1000)).unwrap();
        n.push(make_stack_sample(1, 1, 2000)).unwrap(); // triggers batch
        assert_eq!(n.stats().batches_emitted, 1);
        n.push(make_stack_sample(1, 1, 3000)).unwrap();
        n.flush().unwrap(); // flush the remaining row
        assert_eq!(n.stats().batches_emitted, 2);
    }
}
