use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("trace format version {found} is newer than supported {supported}")]
    FormatVersionTooNew { found: u32, supported: u32 },

    #[error("trace schema version {found} is newer than supported {supported}")]
    SchemaVersionTooNew { found: u32, supported: u32 },

    #[error("not a .patrace directory: {0}")]
    InvalidDirectory(String),

    #[error("unknown table name '{0}'")]
    UnknownTable(String),

    #[error("schema mismatch for table '{table}': {detail}")]
    SchemaMismatch { table: String, detail: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("manifest parse error: {0}")]
    ManifestParse(#[from] serde_json::Error),

    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
}

pub type Result<T> = std::result::Result<T, Error>;
