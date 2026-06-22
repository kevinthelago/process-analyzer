use std::sync::Arc;

use query::{QueryEngine, Selection, TraceStore};
use tokio_util::sync::CancellationToken;

use crate::{
    detector::Detector,
    detectors::{
        CpuSpikeDetector, HotProcessDetector, IoStallDetector, MemoryGrowthDetector,
        SchedWaitDetector,
    },
    error::{AnalysisError, Result},
    finding::{rank_findings, Finding},
};

/// Runs all registered detectors against a `TraceStore` and returns ranked
/// `Finding`s.
///
/// Detectors are run sequentially; each is async and yields during DataFusion
/// query execution so the runtime remains responsive.  Cancelling `token` stops
/// after the current detector finishes its first cancellable await point.
///
/// A clean trace (no issues found) returns `Ok(vec![])` — never an error.
pub struct AnalysisEngine {
    detectors: Vec<Arc<dyn Detector>>,
    query_engine: QueryEngine,
}

impl Default for AnalysisEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisEngine {
    /// Creates the engine with all five built-in detectors at default thresholds.
    pub fn new() -> Self {
        let detectors: Vec<Arc<dyn Detector>> = vec![
            Arc::new(HotProcessDetector::default()),
            Arc::new(CpuSpikeDetector::default()),
            Arc::new(IoStallDetector::default()),
            Arc::new(SchedWaitDetector::default()),
            Arc::new(MemoryGrowthDetector::default()),
        ];
        Self {
            detectors,
            query_engine: QueryEngine::new(),
        }
    }

    /// Creates the engine with a custom detector set.
    pub fn with_detectors(detectors: Vec<Arc<dyn Detector>>) -> Self {
        Self {
            detectors,
            query_engine: QueryEngine::new(),
        }
    }

    /// Runs all detectors and returns findings sorted by severity then score.
    pub async fn run_all(
        &self,
        store: &dyn TraceStore,
        selection: &Selection,
        token: CancellationToken,
    ) -> Result<Vec<Finding>> {
        let mut all_findings = Vec::new();

        for detector in &self.detectors {
            if token.is_cancelled() {
                break;
            }

            let child_token = token.child_token();
            let name = detector.name();

            match detector
                .detect(&self.query_engine, store, selection, child_token)
                .await
            {
                Ok(findings) => {
                    tracing::debug!(detector = %name, count = findings.len(), "detector complete");
                    all_findings.extend(findings);
                }
                Err(AnalysisError::Cancelled) => {
                    tracing::debug!(detector = %name, "detector cancelled");
                    break;
                }
                Err(e) => {
                    // Log and continue — one failing detector should not suppress
                    // results from the others.
                    tracing::warn!(detector = %name, error = %e, "detector error (skipped)");
                }
            }
        }

        rank_findings(&mut all_findings);
        Ok(all_findings)
    }
}
