use serde::{Deserialize, Serialize};

/// Change status of an entity in the diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind {
    /// Present in both traces with changed metrics.
    Changed,
    /// Present only in trace A.
    Removed,
    /// Present only in trace B.
    Added,
}

/// Per-function CPU time delta between two traces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDelta {
    pub function_name: String,
    pub binary_name: Option<String>,
    pub kind: ChangeKind,

    /// Absolute change in self-sample count (positive = more samples in B).
    pub self_samples_delta: i64,
    /// Absolute change in total-sample count.
    pub total_samples_delta: i64,

    /// Duration-normalized self-sample delta as a fraction of trace A's duration.
    /// Allows meaningful comparison between traces of different lengths.
    pub normalized_self_delta: f64,
    pub normalized_total_delta: f64,
}

/// Per-process I/O delta between two traces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoDelta {
    pub pid: u32,
    pub process_name: Option<String>,
    pub kind: ChangeKind,

    /// Absolute change in I/O bytes (positive = more in B).
    pub io_bytes_delta: i64,
    /// Absolute change in I/O operation count.
    pub io_ops_delta: i64,

    pub normalized_io_bytes_delta: f64,
}

/// Per-process memory allocation delta between two traces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDelta {
    pub pid: u32,
    pub process_name: Option<String>,
    pub kind: ChangeKind,

    /// Net allocation bytes change (alloc - free) between traces.
    pub net_alloc_delta: i64,
    pub normalized_net_alloc_delta: f64,
}

/// A process or thread observed in only one of the two traces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub kind: EntityKind,
    pub pid: u32,
    pub tid: Option<u32>,
    pub name: Option<String>,
    pub change: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityKind {
    Process,
    Thread,
}

/// Summary statistics for the diff.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffSummary {
    /// Duration of trace A in nanoseconds.
    pub duration_a_ns: u64,
    /// Duration of trace B in nanoseconds.
    pub duration_b_ns: u64,

    pub functions_changed: usize,
    pub functions_added: usize,
    pub functions_removed: usize,

    pub processes_added: usize,
    pub processes_removed: usize,
}

/// The complete diff between two trace stores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceDiff {
    pub summary: DiffSummary,
    pub function_deltas: Vec<FunctionDelta>,
    pub io_deltas: Vec<IoDelta>,
    pub memory_deltas: Vec<MemoryDelta>,
    pub added_entities: Vec<Entity>,
    pub removed_entities: Vec<Entity>,
}
