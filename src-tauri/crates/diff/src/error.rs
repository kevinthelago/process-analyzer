use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiffError {
    #[error("diff cancelled")]
    Cancelled,

    #[error("query error: {0}")]
    Query(#[from] query::QueryError),

    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("datafusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),
}

pub type Result<T> = std::result::Result<T, DiffError>;
