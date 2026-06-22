use std::path::PathBuf;
use thiserror::Error;

use crate::detect::TraceFormat;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error(
        "unsupported trace format for '{path}'; supported: \
         ETL (.etl), Linux perf (perf.data / *.perf.data), \
         Process Analyzer container (.patrace directory)"
    )]
    UnknownFormat { path: PathBuf },

    #[error("'{path}' is detected as {format} but failed validation: {reason}")]
    InvalidFile {
        path: PathBuf,
        format: TraceFormat,
        reason: String,
    },

    /// The source was readable up to `boundary_offset`; events before that offset
    /// were already emitted. This is the terminal error from the event stream.
    #[error(
        "'{path}' is corrupt; partial data imported up to byte {boundary_offset}: {reason}"
    )]
    PartialCorruption {
        path: PathBuf,
        boundary_offset: u64,
        reason: String,
    },

    #[error("I/O error reading '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("trace-core error reading '{path}': {reason}")]
    TraceCore { path: PathBuf, reason: String },
}
