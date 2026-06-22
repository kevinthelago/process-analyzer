use std::fmt;

use query::{QueryEngine, Selection, TraceStore};
use tokio_util::sync::CancellationToken;

use crate::{error::Result, finding::Finding};

/// A pluggable analysis detector.
///
/// Each detector runs one or more DataFusion queries against the store and
/// returns zero or more `Finding`s.  A detector that finds nothing on a clean
/// trace must return an empty `Vec` — never an error.
///
/// Detectors are registered with `AnalysisEngine` and run in parallel.  Each
/// receives its own `CancellationToken` cloned from the engine-level token.
pub trait Detector: Send + Sync + fmt::Debug {
    /// Short unique identifier shown in log output (e.g. `"hot-process"`).
    fn name(&self) -> &str;

    /// Run the detector and return ranked findings (or an empty vec).
    fn detect<'a>(
        &'a self,
        engine: &'a QueryEngine,
        store: &'a dyn TraceStore,
        selection: &'a Selection,
        token: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Finding>>> + Send + 'a>>;
}
