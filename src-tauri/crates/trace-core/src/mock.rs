use crate::{error::Result, event::RawEvent, recorder::Recorder};

/// A [`Recorder`] that accepts every event and discards it immediately.
///
/// Intended for unit tests in crates that depend on `trace-core` and need a
/// backend without setting up a real [`crate::store::TraceStore`].
pub struct NoOpRecorder;

impl Recorder for NoOpRecorder {
    #[inline]
    fn record(&mut self, _event: RawEvent) -> Result<()> { Ok(()) }

    #[inline]
    fn flush(&mut self) -> Result<()> { Ok(()) }
}
