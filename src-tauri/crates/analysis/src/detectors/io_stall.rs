use std::sync::Arc;

use arrow::array::{Int64Array, UInt32Array};
use datafusion::{datasource::MemTable, prelude::*};
use query::{QueryEngine, Selection, TimeRange, TraceStore};
use tokio_util::sync::CancellationToken;

use crate::{
    detector::Detector,
    error::Result,
    finding::{Finding, FindingKind, Severity},
};

/// Detects individual I/O operations that stalled for longer than `threshold_ns`.
///
/// Checks both `disk_io` and `file_io` tables.  Default threshold: 10 ms.
#[derive(Debug)]
pub struct IoStallDetector {
    pub threshold_ns: u64,
    pub max_findings: usize,
}

impl Default for IoStallDetector {
    fn default() -> Self {
        Self { threshold_ns: 10_000_000, max_findings: 50 }
    }
}

impl Detector for IoStallDetector {
    fn name(&self) -> &str { "io-stall" }

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
            let threshold = self.threshold_ns;

            let time_filter = match &selection.time_range {
                Some(tr) => format!("AND timestamp_ns BETWEEN {} AND {}", tr.start_ns, tr.end_ns),
                None => String::new(),
            };
            let pid_filter = match &selection.pids {
                Some(pids) if !pids.is_empty() => {
                    let list = pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
                    format!("AND process_id IN ({list})")
                }
                _ => String::new(),
            };

            // UNION disk_io and file_io, both have duration_ns and size_bytes.
            let sql = format!(
                "WITH io_union AS ( \
                   SELECT timestamp_ns, process_id, CAST(duration_ns AS BIGINT) AS dur_ns, \
                          CAST(size_bytes AS BIGINT) AS bytes, 'disk' AS io_type \
                   FROM disk_io  WHERE duration_ns >= {threshold} {time_filter} {pid_filter} \
                   UNION ALL \
                   SELECT timestamp_ns, process_id, CAST(duration_ns AS BIGINT) AS dur_ns, \
                          CAST(COALESCE(size_bytes, 0) AS BIGINT) AS bytes, 'file' AS io_type \
                   FROM file_io  WHERE duration_ns >= {threshold} {time_filter} {pid_filter} \
                 ) \
                 SELECT timestamp_ns, process_id, dur_ns, bytes, io_type \
                 FROM io_union \
                 ORDER BY dur_ns DESC \
                 LIMIT {}",
                self.max_findings
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

            let ts_col   = batch.column_by_name("timestamp_ns").and_then(|c| c.as_any().downcast_ref::<Int64Array>());
            let pid_col  = batch.column_by_name("process_id").and_then(|c| c.as_any().downcast_ref::<UInt32Array>());
            let dur_col  = batch.column_by_name("dur_ns").and_then(|c| c.as_any().downcast_ref::<Int64Array>());
            let type_col = batch.column_by_name("io_type").and_then(|c| c.as_any().downcast_ref::<arrow::array::StringArray>());

            let mut findings = Vec::new();
            for i in 0..batch.num_rows() {
                let dur = dur_col.map(|c| c.value(i)).unwrap_or(0);
                let ts  = ts_col.map(|c| c.value(i)).unwrap_or(0);
                let pid = pid_col.map(|c| c.value(i)).unwrap_or(0);
                let io_type = type_col.map(|c| c.value(i)).unwrap_or("io");
                let dur_ms = dur as f64 / 1_000_000.0;

                let severity = if dur_ms >= 100.0 { Severity::High }
                               else if dur_ms >= 50.0 { Severity::Medium }
                               else { Severity::Low };

                let window_ns = dur.max(1_000_000);
                let mut zoom = selection.clone();
                zoom.time_range = Some(TimeRange::new(ts - window_ns, ts + window_ns));
                zoom.pids = Some(vec![pid]);

                findings.push(Finding::new(
                    FindingKind::IoStall,
                    severity,
                    format!("{io_type} I/O stall {dur_ms:.1} ms (pid {pid})"),
                    format!(
                        "{io_type} I/O took {dur_ms:.1} ms — {:.1}× the stall threshold of {} ms.",
                        dur_ms / (threshold as f64 / 1_000_000.0),
                        threshold / 1_000_000,
                    ),
                    zoom,
                    dur as f64,
                ));
            }
            Ok(findings)
        })
    }
}

fn build_session(store: &dyn TraceStore) -> crate::error::Result<SessionContext> {
    let ctx = SessionContext::new();
    register(&ctx, "disk_io", store.disk_io(), query::disk_io_schema())?;
    register(&ctx, "file_io", store.file_io(), query::file_io_schema())?;
    Ok(ctx)
}

fn register(
    ctx: &SessionContext,
    name: &str,
    batches: &[arrow::record_batch::RecordBatch],
    schema: arrow::datatypes::SchemaRef,
) -> crate::error::Result<()> {
    let table = MemTable::try_new(schema, vec![batches.to_vec()])
        .map_err(|e| crate::error::AnalysisError::DataFusion(e))?;
    ctx.register_table(name, Arc::new(table))
        .map_err(|e| crate::error::AnalysisError::DataFusion(e))?;
    Ok(())
}
