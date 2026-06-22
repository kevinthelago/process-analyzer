// Re-export the canonical RawEvent type from trace-core so callers
// only need one import for the full import pipeline.
pub use trace_core::RawEvent;

pub mod detect;
pub mod error;
pub mod adapters;

pub use detect::{detect_format, TraceFormat};
pub use error::ImportError;

use std::path::Path;

/// Result of importing a trace file or directory.
pub struct ImportResult {
    /// Stream of events in ascending timestamp order.
    /// For corrupt files the stream emits partial data then terminates.
    pub events: std::pin::Pin<Box<dyn futures::Stream<Item = Result<RawEvent, ImportError>> + Send>>,
    /// `false` when the source was truncated or corrupt.
    pub is_complete: bool,
    /// For incomplete imports: byte offset (or row index) where valid data ended.
    pub boundary_offset: Option<u64>,
}

/// Import a trace file or directory, auto-detecting its format.
///
/// Supported inputs:
/// - `.etl` files (Windows ETW)
/// - `perf.data` files (Linux perf)
/// - `.patrace` directories (Process Analyzer native container)
///
/// Unknown formats return `ImportError::UnknownFormat` listing what is supported.
/// Corrupt files return partial data with `ImportResult::is_complete == false`.
pub async fn import_trace(path: &Path) -> Result<ImportResult, ImportError> {
    // Directory inputs: only .patrace (manifest.json inside) is recognised.
    if path.is_dir() {
        if path.join("manifest.json").exists() {
            tracing::debug!(?path, "importing .patrace directory");
            return adapters::patrace::import(path).await;
        }
        return Err(ImportError::UnknownFormat {
            path: path.to_path_buf(),
        });
    }

    let header = read_header_bytes(path).await?;
    let format = detect_format(path, &header)?;
    tracing::debug!(?path, ?format, "importing trace file");
    match format {
        TraceFormat::PerfData => adapters::perf::import(path).await,
        TraceFormat::Etl => adapters::etl::import(path).await,
        TraceFormat::PatraceDirectory => {
            // Single-file .patrace: should have been caught as a directory above;
            // this path handles the extension-matched-but-is-file case.
            Err(ImportError::InvalidFile {
                path: path.to_path_buf(),
                format: TraceFormat::PatraceDirectory,
                reason: "expected a directory, got a file".into(),
            })
        }
    }
}

async fn read_header_bytes(path: &Path) -> Result<Vec<u8>, ImportError> {
    use tokio::io::AsyncReadExt as _;
    let mut f = tokio::fs::File::open(path).await.map_err(|e| ImportError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut buf = vec![0u8; 16];
    let n = f.read(&mut buf).await.map_err(|e| ImportError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    buf.truncate(n);
    Ok(buf)
}
