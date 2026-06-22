//! Windows ETW capture backend.
//!
//! Implements the `Recorder` trait using the NT Kernel Logger via ETW.
//! Requires Administrator privileges. Falls back to `wpr.exe` when available.

#![cfg(windows)]

pub mod error;
pub mod etw;
pub mod recorder;
pub mod wpr;

mod elevation;

pub use error::RecordError;
pub use recorder::{KernelFlags, RecordConfig, RecordSummary, RecorderStats, WindowsEtwRecorder};

mod tests;

/// Capture backend contract — mirrors the trace-core `Recorder` trait.
///
/// This local definition will be replaced by `trace-core::Recorder` at workspace
/// integration. The method signatures must remain compatible.
pub trait Recorder: Send + 'static {
    /// Start capturing events with the given configuration.
    fn start(&mut self, config: RecordConfig) -> Result<(), RecordError>;

    /// Stop the active capture and return session statistics.
    fn stop(&mut self) -> Result<RecordSummary, RecordError>;

    /// Return live session statistics without stopping.
    fn stats(&self) -> Option<RecorderStats>;
}
