pub mod delta;
pub mod engine;
pub mod error;

pub use delta::{
    ChangeKind, DiffSummary, Entity, EntityKind, FunctionDelta, IoDelta, MemoryDelta, TraceDiff,
};
pub use engine::DiffEngine;
pub use error::{DiffError, Result};
