//! Local mirror of the `trace-core::Recorder` integration contract.
//!
//! When the `trace-core` crate is published, replace the body of this file with:
//!
//! ```ignore
//! pub use trace_core::Recorder;
//! ```
//!
//! and remove the local definition below. The method signatures here are the
//! authoritative contract; if they need to change, coordinate with the director
//! before modifying them.

use crate::{RecordConfig, RecordError, TraceData};

/// Records a running process.
///
/// Implementations are OS-specific capture backends. `trace-core` consumes
/// this trait to drive capture, flush data, and convert raw events to Arrow.
pub trait Recorder: Send + 'static {
    /// Attach to `pid` and begin capturing.
    ///
    /// Must not be called while already recording; returns
    /// [`RecordError::AlreadyRecording`] if so.
    fn start(&mut self, pid: u32, config: RecordConfig) -> Result<(), RecordError>;

    /// Stop capturing and return all collected data.
    ///
    /// Blocks until all in-flight data is flushed. Returns
    /// [`RecordError::NotRecording`] if not currently active.
    fn stop(&mut self) -> Result<TraceData, RecordError>;

    /// Whether a recording session is currently active.
    fn is_recording(&self) -> bool;
}
