use std::sync::Arc;

use arrow::array::{Int64Array, UInt32Array};
use datafusion::{datasource::MemTable, prelude::*};
use query::{QueryEngine, Selection, TraceStore};
use tokio_util::sync::CancellationToken;

use crate::{
    detector::Detector,
    error::Result,
    finding::{Finding, FindingKind, Severity},
};

/// Memory event_type constants (matches trace-core encoding).
/// 0 = alloc, 1 = free (UInt8 in the schema).
const MEM_ALLOC: u8 = 0;
const MEM_FREE:  u8 = 1;

/// Detects processes with monotonically growing heap allocation over time.
///
/// Algorithm:
///  1. Bucket `memory` events into `window_count` time windows.
///  2. Compute running net allocation (alloc bytes - free bytes) per window.
///  3. If net allocation increases monotonically for `min_consecutive` windows,
///     flag it as a potential memory leak.
///
/// Default: 10 windows, 5 consecutive growing windows required.
#[derive(Debug)]
pub struct MemoryGrowthDetector {
    pub window_count: usize,
    pub min_consecutive: usize,
}

impl Default for MemoryGrowthDetector {
    fn default() -> Self {
        Self { window_count: 10, min_consecutive: 5 }
    }
}

impl Detector for MemoryGrowthDetector {
    fn name(&self) -> &str { "memory-growth" }

    fn detect<'a>(
        &'a self,
        _engine: &'a QueryEngine,
        store: &'a dyn TraceStore,
        selection: &'a Selection,
        token: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Finding>>> + Send + 'a>>
    {
        Box::pin(async move {
            let ctx = build_session(store)?;

            let duration = store.duration_ns() as i64;
            let (range_start, range_end) = match &selection.time_range {
                Some(tr) => (tr.start_ns, tr.end_ns),
                None => (0, duration),
            };
            let span = range_end - range_start;
            if span <= 0 { return Ok(vec![]); }
            let bucket_size = (span / self.window_count as i64).max(1);

            let pid_filter = match &selection.pids {
                Some(pids) if !pids.is_empty() => {
                    let list = pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
                    format!("AND process_id IN ({list})")
                }
                _ => String::new(),
            };

            let sql = format!(
                "SELECT \
                   process_id, \
                   CAST(FLOOR(CAST(timestamp_ns - {range_start} AS DOUBLE) / {bucket_size}) AS BIGINT) AS bucket, \
                   SUM(CASE WHEN event_type = {MEM_ALLOC} THEN CAST(COALESCE(size_bytes, 0) AS BIGINT) ELSE 0 END) AS alloc_bytes, \
                   SUM(CASE WHEN event_type = {MEM_FREE}  THEN CAST(COALESCE(size_bytes, 0) AS BIGINT) ELSE 0 END) AS free_bytes \
                 FROM memory \
                 WHERE timestamp_ns BETWEEN {range_start} AND {range_end} \
                   {pid_filter} \
                 GROUP BY process_id, bucket \
                 ORDER BY process_id, bucket",
            );

            let df = tokio::select! {
                r = ctx.sql(&sql) => r?,
                _ = token.cancelled() => return Ok(vec![]),
            };
            let batches = tokio::select! {
                r = df.collect() => r?,
                _ = token.cancelled() => return Ok(vec![]),
            };
            if batches.is_empty() { return Ok(vec![]); }
            let batch = arrow::compute::concat_batches(&batches[0].schema(), &batches)?;

            let pid_col   = batch.column_by_name("process_id").and_then(|c| c.as_any().downcast_ref::<UInt32Array>());
            let alloc_col = batch.column_by_name("alloc_bytes").and_then(|c| c.as_any().downcast_ref::<Int64Array>());
            let free_col  = batch.column_by_name("free_bytes").and_then(|c| c.as_any().downcast_ref::<Int64Array>());
            let bucket_col = batch.column_by_name("bucket").and_then(|c| c.as_any().downcast_ref::<Int64Array>());

            let Some(pid_col) = pid_col else { return Ok(vec![]); };

            use std::collections::HashMap;
            let mut per_pid: HashMap<u32, Vec<(i64, i64)>> = HashMap::new();
            for i in 0..batch.num_rows() {
                let pid   = pid_col.value(i);
                let alloc = alloc_col.map(|c| c.value(i)).unwrap_or(0);
                let free  = free_col.map(|c| c.value(i)).unwrap_or(0);
                let bucket = bucket_col.map(|c| c.value(i)).unwrap_or(0);
                per_pid.entry(pid).or_default().push((bucket, alloc - free));
            }

            let mut findings = Vec::new();
            for (pid, mut windows) in per_pid {
                windows.sort_by_key(|(b, _)| *b);
                let net: Vec<i64> = windows.iter().map(|(_, n)| *n).collect();
                let max_run = longest_increasing_run(&net);
                if max_run < self.min_consecutive { continue; }

                let total_growth: i64 = net.iter().sum();
                let growth_mb = total_growth as f64 / (1024.0 * 1024.0);
                let severity = if max_run as f64 / self.window_count as f64 >= 0.9 {
                    Severity::High
                } else {
                    Severity::Medium
                };

                let mut zoom = selection.clone();
                zoom.pids = Some(vec![pid]);

                findings.push(Finding::new(
                    FindingKind::MemoryGrowth,
                    severity,
                    format!("pid {pid} shows monotonic memory growth ({growth_mb:.1} MB net)"),
                    format!(
                        "Process {pid} had {max_run} consecutive windows of increasing net \
                         allocation, totalling {growth_mb:.1} MB net growth. This pattern \
                         suggests a potential memory leak.",
                    ),
                    zoom,
                    max_run as f64,
                ));
            }
            Ok(findings)
        })
    }
}

fn longest_increasing_run(values: &[i64]) -> usize {
    if values.is_empty() { return 0; }
    let mut max_run = 1usize;
    let mut cur = 1usize;
    for i in 1..values.len() {
        if values[i] > values[i - 1] {
            cur += 1;
            max_run = max_run.max(cur);
        } else {
            cur = 1;
        }
    }
    max_run
}

fn build_session(store: &dyn TraceStore) -> crate::error::Result<SessionContext> {
    let ctx = SessionContext::new();
    let table = MemTable::try_new(query::memory_schema(), vec![store.memory().to_vec()])
        .map_err(|e| crate::error::AnalysisError::DataFusion(e))?;
    ctx.register_table("memory", Arc::new(table))
        .map_err(|e| crate::error::AnalysisError::DataFusion(e))?;
    Ok(ctx)
}

#[cfg(test)]
mod tests {
    use super::longest_increasing_run;
    #[test] fn run_all_increasing()  { assert_eq!(longest_increasing_run(&[1,2,3,4,5]), 5); }
    #[test] fn run_none_increasing() { assert_eq!(longest_increasing_run(&[5,4,3,2,1]), 1); }
    #[test] fn run_partial()         { assert_eq!(longest_increasing_run(&[1,2,1,2,3,4]), 4); }
    #[test] fn run_empty()           { assert_eq!(longest_increasing_run(&[]), 0); }
}
