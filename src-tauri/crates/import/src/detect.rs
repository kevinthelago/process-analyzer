use std::fmt;
use std::path::Path;

use crate::error::ImportError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceFormat {
    /// Windows ETW trace log (.etl)
    Etl,
    /// Linux perf recording (perf.data / *.perf.data)
    PerfData,
    /// Process Analyzer native container directory (.patrace or directory with manifest.json)
    PatraceDirectory,
}

impl fmt::Display for TraceFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraceFormat::Etl => write!(f, "ETL (Windows ETW trace)"),
            TraceFormat::PerfData => write!(f, "perf.data (Linux perf)"),
            TraceFormat::PatraceDirectory => write!(f, "Process Analyzer container (.patrace directory)"),
        }
    }
}

const PERF_MAGIC_V2: &[u8] = b"PERFILE2";
const PERF_MAGIC_V1: &[u8] = b"PERFILE "; // older perf (note trailing space)

/// Identify the format of a trace *file* using magic bytes and/or extension.
///
/// For directory inputs, check `path.join("manifest.json").exists()` before calling
/// this function — `detect_format` is only for file inputs.
pub fn detect_format(path: &Path, header_bytes: &[u8]) -> Result<TraceFormat, ImportError> {
    // Magic bytes are authoritative.
    if header_bytes.starts_with(PERF_MAGIC_V2) || header_bytes.starts_with(PERF_MAGIC_V1) {
        return Ok(TraceFormat::PerfData);
    }

    // Extension fallback.
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());

    match ext.as_deref() {
        Some("etl") => return Ok(TraceFormat::Etl),
        Some("patrace") => return Ok(TraceFormat::PatraceDirectory),
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
        assert_eq!(detect_format(&p("x"), b"PERFILE2xxx").unwrap(), TraceFormat::PerfData);
    }

    #[test]
    fn detects_perf_by_magic_v1() {
        assert_eq!(detect_format(&p("x"), b"PERFILE xxx").unwrap(), TraceFormat::PerfData);
    }

    #[test]
    fn detects_etl_by_extension() {
        assert_eq!(detect_format(&p("trace.etl"), &[]).unwrap(), TraceFormat::Etl);
    }

    #[test]
    fn detects_patrace_by_extension() {
        assert_eq!(detect_format(&p("s.patrace"), &[]).unwrap(), TraceFormat::PatraceDirectory);
    }

    #[test]
    fn detects_perf_data_by_filename() {
        assert_eq!(detect_format(&p("perf.data"), &[]).unwrap(), TraceFormat::PerfData);
    }

    #[test]
    fn detects_perf_data_by_suffix() {
        assert_eq!(detect_format(&p("sess.perf.data"), &[]).unwrap(), TraceFormat::PerfData);
    }

    #[test]
    fn unknown_format_lists_supported_types() {
        let err = detect_format(&p("mystery.xyz"), &[]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("mystery.xyz"), "{msg}");
        assert!(msg.contains("ETL"), "{msg}");
    }
}
