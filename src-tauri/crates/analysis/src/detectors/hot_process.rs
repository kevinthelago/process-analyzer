use arrow::array::{Float64Array, StringArray, UInt32Array};
use query::{QueryEngine, Selection, TraceStore};
use tokio_util::sync::CancellationToken;

use crate::{
    detector::Detector,
    error::Result,
    finding::{Finding, FindingKind, Severity},
};

/// Detects processes that dominate CPU within the selection.
///
/// A process is flagged when it accounts for more than `threshold_pct`
/// percent of all CPU samples in the selected window.
/// Default threshold: 50%.
#[derive(Debug)]
pub struct HotProcessDetector {
    pub threshold_pct: f64,
    pub top_n: usize,
}

impl Default for HotProcessDetector {
    fn default() -> Self {
        Self { threshold_pct: 50.0, top_n: 5 }
    }
}

impl Detector for HotProcessDetector {
    fn name(&self) -> &str { "hot-process" }

    fn detect<'a>(
        &'a self,
        engine: &'a QueryEngine,
        store: &'a dyn TraceStore,
        selection: &'a Selection,
        token: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Finding>>> + Send + 'a>>
    {
        Box::pin(async move {
            let batch = match engine.top_n_by_cpu(store, selection, self.top_n, token).await {
                Ok(b) => b,
                Err(query::QueryError::EmptyResult | query::QueryError::Cancelled) => return Ok(vec![]),
                Err(e) => return Err(e.into()),
            };

            let name_col = batch
                .column_by_name("name")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let pid_col = batch
                .column_by_name("process_id")
                .and_then(|c| c.as_any().downcast_ref::<UInt32Array>());
            let pct_col = batch
                .column_by_name("cpu_pct")
                .and_then(|c| c.as_any().downcast_ref::<Float64Array>());

            let mut findings = Vec::new();
            for i in 0..batch.num_rows() {
                let pct = pct_col.map(|c| c.value(i)).unwrap_or(0.0);
                if pct < self.threshold_pct {
                    continue;
                }

                let name = name_col
                    .map(|c| c.value(i).to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                let pid = pid_col.map(|c| c.value(i)).unwrap_or(0);

                let severity = if pct >= 90.0 {
                    Severity::Critical
                } else if pct >= 75.0 {
                    Severity::High
                } else {
                    Severity::Medium
                };

                let mut zoom = selection.clone();
                zoom.pids = Some(vec![pid]);

                findings.push(Finding::new(
                    FindingKind::HotProcess,
                    severity,
                    format!("{name} (pid {pid}) is using {pct:.1}% CPU"),
                    format!(
                        "Process '{name}' (pid {pid}) consumed {pct:.1}% of all CPU samples in \
                         the selected window, exceeding the {:.0}% threshold.",
                        self.threshold_pct
                    ),
                    zoom,
                    pct,
                ));
            }
            Ok(findings)
        })
    }
}
