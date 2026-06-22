use serde::{Deserialize, Serialize};

/// Bumped only when the on-disk container layout changes (e.g. file naming).
pub const FORMAT_VERSION: u32 = 1;

/// Bumped when any table schema changes in a backward-incompatible way.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub schema_version: u32,

    /// Caller-supplied opaque ID (UUID v4 string by convention).
    pub trace_id: String,

    /// Unix timestamp in nanoseconds when recording began.
    pub created_at_ns: i64,

    /// Wall-clock duration of the captured window, nanoseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ns: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,

    /// One entry per non-empty domain table serialized on disk.
    #[serde(default)]
    pub tables: Vec<TableMeta>,
}

impl Manifest {
    pub fn new(trace_id: impl Into<String>, created_at_ns: i64) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            schema_version: SCHEMA_VERSION,
            trace_id: trace_id.into(),
            created_at_ns,
            duration_ns: None,
            hostname: None,
            os: None,
            arch: None,
            tables: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMeta {
    pub name: String,
    pub file: String,
    pub row_count: u64,
}
