use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("query cancelled")]
    Cancelled,

    #[error("DataFusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),

    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("empty result: no data for this selection")]
    EmptyResult,
}

pub type Result<T> = std::result::Result<T, QueryError>;
