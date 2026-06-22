use thiserror::Error;

#[derive(Debug, Error)]
pub enum NormalizerError {
    #[error("trace-core error: {0}")]
    TraceCore(#[from] trace_core::Error),
}
