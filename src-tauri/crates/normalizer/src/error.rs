use thiserror::Error;

#[derive(Debug, Error)]
pub enum NormalizerError {
    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),

    #[error("symbolicator error: {0}")]
    Symbolicator(#[from] symbolicator::SymbolicatorError),
}
