use std::fmt;
use std::path::Path;

use crate::error::ImportError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceFormat {
    /// Windows ETW trace log (.etl)
    Etl,
    /// Linux perf recording (perf.data / *.perf.data)
    PerfData,
    /// Process Analyzer native container (.patrace)
    PatraceContainer,
}

impl fmt::Display for TraceFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraceFormat::Etl => write!(f, "ETL (Windows ETW trace)"),
            TraceFormat::PerfData => write!(f, "perf.data (Linux perf)"),
            TraceFormat::PatraceContainer => write!(f, "Process Analyzer container (.patrace)"),
        }
    }
}

// Magic bytes for format detection
const PERF_MAGIC_V2: &[u8] = b"PERFILE2";
const PERF_MAGIC_V1: &[u8] = b"PERFILE "; // older perf (note trailing space)
pub const PATRACE_MAGIC: &[u8] = b"PATRACE\0";

/// Identify the format of a trace file using magic bytes and/or extension.
///
/// `header_bytes` must be at least the first 8 bytes of the file (fewer is
/// tolerated but reduces confidence in magic-byte detection).
pub fn detect_format(path: &Path, header_bytes: &[u8]) -> Result<TraceFormat, ImportError> {
    // Magic bytes are authoritative — check before extension.
    if header_bytes.starts_with(PERF_MAGIC_V2) || header_bytes.starts_with(PERF_MAGIC_V1) {
        return Ok(TraceFormat::PerfData);
    }
    if header_bytes.starts_with(PATRACE_MAGIC) {
        return Ok(TraceFormat::PatraceContainer);
    }

    // Extension fallback (ETL has no universal magic we can check cross-platform).
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());

    match ext.as_deref() {
        Some("etl") => return Ok(TraceFormat::Etl),
        Some("patrace") => return Ok(TraceFormat::PatraceContainer),
        _ => {}
    }

    // perf.data matched by filename convention.
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_lowercase();
        if lower == "perf.data" || lower.ends_with(".perf.data") {
            return Ok(TraceFormat::PerfData);
        }
    }

    Err(ImportError::UnknownFormat {
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(name: &str) -> PathBuf { PathBuf::from(name) }

    #[test]
    fn detects_perf_by_magic_v2() {
        let r = detect_format(&p("unknown"), b"PERFILE2xxxxxxxx");
        assert_eq!(r.unwrap(), TraceFormat::PerfData);
    }

    #[test]
    fn detects_perf_by_magic_v1() {
        let r = detect_format(&p("unknown"), b"PERFILE xxxxxxxx");
        assert_eq!(r.unwrap(), TraceFormat::PerfData);
    }

    #[test]
    fn detects_patrace_by_magic() {
        let r = detect_format(&p("foo.etl"), PATRACE_MAGIC);
        // magic wins over extension
        assert_eq!(r.unwrap(), TraceFormat::PatraceContainer);
    }

    #[test]
    fn detects_etl_by_extension() {
        let r = detect_format(&p("trace.etl"), &[]);
        assert_eq!(r.unwrap(), TraceFormat::Etl);
    }

    #[test]
    fn detects_patrace_by_extension() {
        let r = detect_format(&p("session.patrace"), &[0, 0, 0, 0]);
        assert_eq!(r.unwrap(), TraceFormat::PatraceContainer);
    }

    #[test]
    fn detects_perf_data_by_filename() {
        let r = detect_format(&p("perf.data"), &[]);
        assert_eq!(r.unwrap(), TraceFormat::PerfData);
    }

    #[test]
    fn detects_perf_data_by_suffixed_filename() {
        let r = detect_format(&p("session.perf.data"), &[]);
        assert_eq!(r.unwrap(), TraceFormat::PerfData);
    }

    #[test]
    fn unknown_format_errors_with_description() {
        let err = detect_format(&p("mystery.xyz"), &[]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("mystery.xyz"), "path in error: {msg}");
        assert!(msg.contains("ETL"), "lists ETL: {msg}");
        assert!(msg.contains(".patrace"), "lists patrace: {msg}");
    }
}
