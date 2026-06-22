use arrow::array::Int64Array;
use query::{QueryEngine, Selection, TimeRange, TraceStore};
use tokio_util::sync::CancellationToken;

use crate::{
    detector::Detector,
    error::Result,
    finding::{Finding, FindingKind, Severity},
};

/// Detects sudden CPU spikes — windows where CPU usage exceeds `spike_factor`
/// times the trace-wide baseline.
///
/// Default spike_factor: 3.0×, window_count: 20.
#[derive(Debug)]
pub struct CpuSpikeDetector {
    pub spike_factor: f64,
    pub window_count: usize,
}

impl Default for CpuSpikeDetector {
    fn default() -> Self {
        Self { spike_factor: 3.0, window_count: 20 }
    }
}

impl Detector for CpuSpikeDetector {
    fn name(&self) -> &str { "cpu-spike" }

    fn detect<'a>(
        &'a self,
        engine: &'a QueryEngine,
        store: &'a dyn TraceStore,
        selection: &'a Selection,
        token: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Finding>>> + Send + 'a>>
    {
        Box::pin(async move {
            let histogram = match engine
                .time_histogram(store, selection, self.window_count, token.clone())
                .await
            {
                Ok(b) => b,
                Err(query::QueryError::EmptyResult | query::QueryError::Cancelled) => return Ok(vec![]),
                Err(e) => return Err(e.into()),
            };

            let cpu_col = histogram
                .column_by_name("cpu_samples")
                .and_then(|c| c.as_any().downcast_ref::<Int64Array>());
            let start_col = histogram
                .column_by_name("bucket_start_ns")
                .and_then(|c| c.as_any().downcast_ref::<Int64Array>());

            let Some(cpu_col) = cpu_col else {
                return Ok(vec![]);
            };
            let n = cpu_col.len();
            if n == 0 { return Ok(vec![]); }

            let total: i64 = (0..n).map(|i| cpu_col.value(i)).sum();
            let baseline = total as f64 / n as f64;
            let threshold = baseline * self.spike_factor;

            let mut findings = Vec::new();
            for i in 0..n {
                let count = cpu_col.value(i) as f64;
                if count < threshold { continue; }

                let ratio = count / baseline;
                let start_ns = start_col.map(|c| c.value(i)).unwrap_or(0);
                let bucket_ns = store.duration_ns() as i64 / self.window_count as i64;
                let end_ns = start_ns + bucket_ns;

                let severity = if ratio >= 10.0 {
                    Severity::Critical
                } else if ratio >= 5.0 {
                    Severity::High
                } else {
                    Severity::Medium
                };

                let mut zoom = selection.clone();
                zoom.time_range = Some(TimeRange::new(start_ns, end_ns));

                findings.push(Finding::new(
                    FindingKind::CpuSpike,
                    severity,
                    format!("CPU spike {ratio:.1}× baseline at t={start_ns}ns"),
                    format!(
                        "CPU sample count of {count:.0} in this window is {ratio:.1}× the \
                         trace-wide baseline of {baseline:.0} samples/window \
                         (spike threshold: {:.1}×).",
                        self.spike_factor
                    ),
                    zoom,
                    ratio,
                ));
            }
            Ok(findings)
        })
    }
}
