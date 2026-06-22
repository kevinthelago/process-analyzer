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

/// Detects scheduling events where a thread waited longer than `threshold_ns`
/// before getting CPU time.
///
/// Default threshold: 5 ms (5_000_000 ns).
#[derive(Debug)]
pub struct SchedWaitDetector {
    pub threshold_ns: u64,
    pub max_findings: usize,
}

impl Default for SchedWaitDetector {
    fn default() -> Self {
        Self { threshold_ns: 5_000_000, max_findings: 50 }
    }
}

impl Detector for SchedWaitDetector {
    fn name(&self) -> &str { "sched-wait" }

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

            let sql = format!(
                "SELECT timestamp_ns, process_id, thread_id, \
                        CAST(duration_ns AS BIGINT) AS dur_ns \
                 FROM scheduling \
                 WHERE duration_ns >= {threshold} \
                   {time_filter} \
                   {pid_filter} \
                 ORDER BY duration_ns DESC \
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

            let ts_col  = batch.column_by_name("timestamp_ns").and_then(|c| c.as_any().downcast_ref::<Int64Array>());
            let pid_col = batch.column_by_name("process_id").and_then(|c| c.as_any().downcast_ref::<UInt32Array>());
            let tid_col = batch.column_by_name("thread_id").and_then(|c| c.as_any().downcast_ref::<UInt32Array>());
            let dur_col = batch.column_by_name("dur_ns").and_then(|c| c.as_any().downcast_ref::<Int64Array>());

            let mut findings = Vec::new();
            for i in 0..batch.num_rows() {
                let dur = dur_col.map(|c| c.value(i)).unwrap_or(0);
                let ts  = ts_col.map(|c| c.value(i)).unwrap_or(0);
                let pid = pid_col.map(|c| c.value(i)).unwrap_or(0);
                let tid = tid_col.map(|c| c.value(i)).unwrap_or(0);
                let dur_ms = dur as f64 / 1_000_000.0;

                let severity = if dur_ms >= 50.0 { Severity::High }
                               else if dur_ms >= 20.0 { Severity::Medium }
                               else { Severity::Low };

                let window_ns = dur.max(1_000_000);
                let mut zoom = selection.clone();
                zoom.time_range = Some(TimeRange::new(ts - window_ns, ts + window_ns));
                zoom.pids = Some(vec![pid]);
                zoom.tids = Some(vec![tid]);

                findings.push(Finding::new(
                    FindingKind::SchedWait,
                    severity,
                    format!("Thread {tid} (pid {pid}) waited {dur_ms:.1} ms for CPU"),
                    format!(
                        "Scheduling latency of {dur_ms:.1} ms exceeded the {:.0} ms threshold. \
                         The thread was descheduled at t={ts}ns.",
                        threshold as f64 / 1_000_000.0,
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
    let table = MemTable::try_new(
        query::scheduling_schema(),
        vec![store.scheduling().to_vec()],
    ).map_err(|e| crate::error::AnalysisError::DataFusion(e))?;
    ctx.register_table("scheduling", Arc::new(table))
        .map_err(|e| crate::error::AnalysisError::DataFusion(e))?;
    Ok(ctx)
}
