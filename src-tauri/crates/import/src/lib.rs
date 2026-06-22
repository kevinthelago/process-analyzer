// Contract stub for trace-core types lives here until that crate lands.
// When pa-trace-core ships, replace `pub mod contract` with:
//   use pa_trace_core::{RawEvent, RawEventKind};
pub mod contract;

pub mod detect;
pub mod error;
pub mod adapters;

pub use contract::{RawEvent, RawEventKind};
pub use detect::{detect_format, TraceFormat};
pub use error::ImportError;

use std::path::Path;

/// Result of importing a trace file.
pub struct ImportResult {
    /// Stream of events in ascending timestamp order.
    /// For corrupt files the stream emits partial data then returns `None`.
    pub events: std::pin::Pin<Box<dyn futures::Stream<Item = Result<RawEvent, ImportError>> + Send>>,
    /// `false` when the file was truncated or corrupt.
    pub is_complete: bool,
    /// For incomplete imports: byte offset where valid data ended.
    pub boundary_offset: Option<u64>,
}

/// Import a trace file, auto-detecting its format from extension and magic bytes.
///
/// Corrupt files return partial data with `ImportResult::is_complete == false`.
/// Unknown formats return `ImportError::UnknownFormat` listing supported formats.
pub async fn import_trace(path: &Path) -> Result<ImportResult, ImportError> {
    let header = read_header_bytes(path).await?;
    let format = detect_format(path, &header)?;
    tracing::debug!(?path, ?format, "importing trace");
    match format {
        TraceFormat::PerfData => adapters::perf::import(path).await,
        TraceFormat::Etl => adapters::etl::import(path).await,
        TraceFormat::PatraceContainer => adapters::patrace::import(path).await,
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
