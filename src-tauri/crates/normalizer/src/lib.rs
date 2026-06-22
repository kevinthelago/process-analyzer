mod clock;
mod error;
pub mod intern;

pub use error::NormalizerError;

use clock::ClockCorrector;
use symbolicator::{LookupResult, ModuleEntry, Symbolicator};
use trace_core::{
    event::{CpuSampleEvent, FrameInfoEvent, ProcessInfoEvent},
    Recorder, RawEvent,
};

/// Statistics accumulated since the normalizer was created.
#[derive(Debug, Default, Clone)]
pub struct NormalizerStats {
    pub events_processed: u64,
    pub frames_symbolized: u64,
    pub clock_corrections: u64,
}

/// A [`Recorder`] decorator that applies clock correction and frame symbolication
/// before forwarding events to an inner [`Recorder`].
///
/// Timestamps are corrected to be monotonically non-decreasing across the session.
/// Frame events with `symbol_name: None` are enriched via the embedded [`Symbolicator`].
/// Call [`Self::register_module`] when the capture backend sees a module load.
pub struct NormalizingRecorder<R: Recorder> {
    inner: R,
    sym: Symbolicator,
    clock: ClockCorrector,
    stats: NormalizerStats,
}

impl<R: Recorder> NormalizingRecorder<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            sym: Symbolicator::new(),
            clock: ClockCorrector::new(),
            stats: NormalizerStats::default(),
        }
    }

    pub fn register_module(&mut self, entry: ModuleEntry) {
        self.sym.add_module(entry);
    }

    pub fn unregister_module(&mut self, base: u64) {
        self.sym.remove_module(base);
    }

    pub fn stats(&self) -> NormalizerStats {
        let mut s = self.stats.clone();
        s.clock_corrections = self.clock.corrections;
        s
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Recorder> Recorder for NormalizingRecorder<R> {
    fn record(&mut self, event: RawEvent) -> trace_core::Result<()> {
        self.stats.events_processed += 1;
        let event = match event {
            RawEvent::CpuSample(mut e) => {
                e.timestamp_ns = self.clock.correct(e.timestamp_ns);
                RawEvent::CpuSample(e)
            }
            RawEvent::Scheduling(mut e) => {
                e.timestamp_ns = self.clock.correct(e.timestamp_ns);
                RawEvent::Scheduling(e)
            }
            RawEvent::DiskIo(mut e) => {
                e.timestamp_ns = self.clock.correct(e.timestamp_ns);
                RawEvent::DiskIo(e)
            }
            RawEvent::FileIo(mut e) => {
                e.timestamp_ns = self.clock.correct(e.timestamp_ns);
                RawEvent::FileIo(e)
            }
            RawEvent::Memory(mut e) => {
                e.timestamp_ns = self.clock.correct(e.timestamp_ns);
                RawEvent::Memory(e)
            }
            RawEvent::Network(mut e) => {
                e.timestamp_ns = self.clock.correct(e.timestamp_ns);
                RawEvent::Network(e)
            }
            RawEvent::Process(mut e) => {
                e.start_time_ns = self.clock.correct(e.start_time_ns);
                if let Some(t) = e.exit_time_ns {
                    e.exit_time_ns = Some(self.clock.correct(t));
                }
                RawEvent::Process(e)
            }
            RawEvent::Thread(mut e) => {
                e.start_time_ns = self.clock.correct(e.start_time_ns);
                if let Some(t) = e.exit_time_ns {
                    e.exit_time_ns = Some(self.clock.correct(t));
                }
                RawEvent::Thread(e)
            }
            RawEvent::Frame(e) => RawEvent::Frame(self.symbolicate_frame(e)),
            // StackEntry has no timestamp.
            // TODO: add RawEvent::ModuleLoad / ModuleUnload arms here once
            // trace-core adds those variants (contracts/trace_core.md §A).
            // They should call register_module/unregister_module and return
            // early (not forwarded to the inner recorder).
            other => other,
        };
        self.inner.record(event)
    }

    fn flush(&mut self) -> trace_core::Result<()> {
        self.inner.flush()
    }
}

impl<R: Recorder> NormalizingRecorder<R> {
    fn symbolicate_frame(&mut self, mut e: FrameInfoEvent) -> FrameInfoEvent {
        if e.symbol_name.is_some() {
            return e;
        }
        match self.sym.lookup(e.address) {
            LookupResult::Resolved(frames) => {
                if let Some(f) = frames.into_iter().next() {
                    e.symbol_name = Some(f.function);
                    e.module_name = Some(f.module);
                    e.file_path = f.file;
                    e.line_number = f.line;
                    self.stats.frames_symbolized += 1;
                }
            }
            LookupResult::ModuleOffset { module, .. } => {
                e.module_name = Some(module);
            }
            LookupResult::Unknown { .. } => {}
        }
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trace_core::NoOpRecorder;

    fn cpu(ts: i64) -> RawEvent {
        RawEvent::CpuSample(CpuSampleEvent {
            timestamp_ns: ts,
            process_id: 1,
            thread_id: 1,
            cpu_id: 0,
            sample_weight: 1,
            stack_id: None,
        })
    }

    fn frame(addr: u64) -> RawEvent {
        RawEvent::Frame(FrameInfoEvent {
            frame_id: addr,
            address: addr,
            symbol_name: None,
            module_name: None,
            file_path: None,
            line_number: None,
        })
    }

    #[test]
    fn pass_through_counts_events() {
        let mut r = NormalizingRecorder::new(NoOpRecorder);
        r.record(cpu(1000)).unwrap();
        assert_eq!(r.stats().events_processed, 1);
    }

    #[test]
    fn clock_backwards_counted() {
        let mut r = NormalizingRecorder::new(NoOpRecorder);
        r.record(cpu(2000)).unwrap();
        r.record(cpu(1000)).unwrap();
        assert_eq!(r.stats().clock_corrections, 1);
    }

    #[test]
    fn frame_unresolved_without_module() {
        let mut r = NormalizingRecorder::new(NoOpRecorder);
        r.record(frame(0xdead_beef)).unwrap();
        assert_eq!(r.stats().frames_symbolized, 0);
    }

    #[test]
    fn frame_existing_symbol_preserved() {
        let mut r = NormalizingRecorder::new(NoOpRecorder);
        r.record(RawEvent::Frame(FrameInfoEvent {
            frame_id: 1,
            address: 0x1234,
            symbol_name: Some("known".to_owned()),
            module_name: None,
            file_path: None,
            line_number: None,
        }))
        .unwrap();
        assert_eq!(r.stats().frames_symbolized, 0);
    }

    #[test]
    fn flush_ok() {
        let mut r = NormalizingRecorder::new(NoOpRecorder);
        r.flush().unwrap();
    }

    #[test]
    fn process_backwards_exit_corrected() {
        let mut r = NormalizingRecorder::new(NoOpRecorder);
        r.record(RawEvent::Process(ProcessInfoEvent {
            process_id: 1,
            parent_process_id: None,
            name: "t".to_owned(),
            cmdline: None,
            start_time_ns: 5000,
            exit_time_ns: Some(3000),
            exit_code: Some(0),
        }))
        .unwrap();
        assert_eq!(r.stats().clock_corrections, 1);
    }
}
