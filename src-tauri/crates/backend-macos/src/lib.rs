#![warn(rust_2018_idioms, clippy::unwrap_used)]

pub mod contracts;
pub mod error;
pub mod types;

#[cfg(target_os = "macos")]
mod ffi;

#[cfg(target_os = "macos")]
mod dyld;

#[cfg(target_os = "macos")]
mod memory;

#[cfg(target_os = "macos")]
mod sampler;

#[cfg(target_os = "macos")]
mod dtrace;

#[cfg(target_os = "macos")]
mod xctrace;

#[cfg(target_os = "macos")]
pub mod recorder;

#[cfg(target_os = "macos")]
pub use recorder::MacosRecorder;

pub use contracts::Recorder;

#[cfg(test)]
mod tests;
pub use error::RecordError;
pub use types::{
    DyldImage, IoEvent, IoKind, MemorySnapshot, RecordConfig, SchedEvent, SchedKind,
    StackSample, TraceData,
};
