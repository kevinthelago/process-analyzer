use serde::{Deserialize, Serialize};

/// A time range expressed in nanoseconds since the trace epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start_ns: i64,
    pub end_ns: i64,
}

impl TimeRange {
    pub fn new(start_ns: i64, end_ns: i64) -> Self {
        debug_assert!(start_ns <= end_ns, "start_ns must be <= end_ns");
        Self { start_ns, end_ns }
    }

    pub fn duration_ns(&self) -> i64 {
        self.end_ns - self.start_ns
    }
}

/// The current user selection that scopes all queries.
///
/// `None` on any field means "no filter" (include all values for that dimension).
/// This mirrors the Zustand selection store on the frontend.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Selection {
    /// Restrict to events within this time range.
    pub time_range: Option<TimeRange>,

    /// Restrict to these process IDs.
    pub pids: Option<Vec<u32>>,

    /// Restrict to these thread IDs.
    pub tids: Option<Vec<u32>>,

    /// SQL LIKE pattern applied to `function_name` (e.g., `"my_crate::%"`).
    /// `%` and `_` are standard SQL wildcards.
    pub function_like: Option<String>,
}

impl Selection {
    /// Build the WHERE clause fragment (excluding the leading `WHERE`).
    /// Returns `None` if the selection imposes no filters.
    ///
    /// `prefix` qualifies pid/tid/function_name columns (e.g. `"s."` for
    /// queries that join tables and need unambiguous column references).
    /// Pass `""` for single-table queries.
    pub fn to_where_clause(&self, ts_col: &str) -> Option<String> {
        self.to_where_clause_prefixed(ts_col, "")
    }

    /// Like `to_where_clause` but qualifies `pid`, `tid`, and `function_name`
    /// with `prefix` (e.g. `"s."`) to resolve column ambiguity in JOIN queries.
    pub fn to_where_clause_prefixed(&self, ts_col: &str, prefix: &str) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();

        if let Some(tr) = &self.time_range {
            parts.push(format!(
                "{ts_col} BETWEEN {} AND {}",
                tr.start_ns, tr.end_ns
            ));
        }

        if let Some(pids) = &self.pids {
            if !pids.is_empty() {
                let list = pids
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                parts.push(format!("{prefix}process_id IN ({list})"));
            }
        }

        if let Some(tids) = &self.tids {
            if !tids.is_empty() {
                let list = tids
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                parts.push(format!("{prefix}thread_id IN ({list})"));
            }
        }

        if let Some(pattern) = &self.function_like {
            // Escape single quotes — function names are user-controlled strings.
            let escaped = pattern.replace('\'', "''");
            parts.push(format!("{prefix}function_name LIKE '{escaped}'"));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" AND "))
        }
    }
}
