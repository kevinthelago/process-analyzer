//! xctrace fallback recorder for when Mach task-port access is unavailable.
//!
//! Uses `xctrace record --template "Time Profiler" --target-pid <pid>` followed
//! by `xctrace export` to extract time-sample data. xctrace ships with Xcode
//! and does not require SIP to be disabled or root privileges.
//!
//! Stack samples parsed from xctrace are less precise than direct Mach sampling:
//! - Timestamps from xctrace use Instruments' internal clock (converted to ns).
//! - Return addresses are symbolicated strings, not raw pointers; we store 0 for
//!   each frame's address and put the symbol in a secondary field.
//!
//! The caller should prefer direct Mach sampling and use this only when
//! `task_for_pid` returns `PermissionDenied`.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;

use tracing::{debug, warn};

use crate::error::RecordError;
use crate::types::{StackSample, TraceData};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub(crate) struct XctraceHandle {
    join: thread::JoinHandle<Result<TraceData, RecordError>>,
}

impl XctraceHandle {
    /// Spawn xctrace in the background.
    ///
    /// Limitation: xctrace must know the duration upfront when targeting a pid.
    /// `stop()` blocks until xctrace finishes (up to `duration_secs` seconds).
    pub(crate) fn spawn(pid: u32, duration_secs: f64) -> Result<Self, RecordError> {
        find_xctrace()?;

        let output_path = std::env::temp_dir().join(format!("process_analyzer_{pid}.xctrace"));
        let join = thread::Builder::new()
            .name(format!("xctrace-{pid}"))
            .spawn(move || run_xctrace(pid, &output_path, duration_secs))
            .map_err(RecordError::Io)?;

        Ok(Self { join })
    }

    pub(crate) fn stop(self) -> Result<TraceData, RecordError> {
        self.join.join().unwrap_or_else(|_| {
            Err(RecordError::XctraceError("xctrace thread panicked".into()))
        })
    }
}

// ---------------------------------------------------------------------------
// xctrace invocation
// ---------------------------------------------------------------------------

fn run_xctrace(pid: u32, output_path: &PathBuf, duration_secs: f64) -> Result<TraceData, RecordError> {
    // Remove any stale trace from a previous run.
    let _ = std::fs::remove_dir_all(output_path);

    let xctrace = find_xctrace()?;
    let dur_str = format!("{}s", (duration_secs.ceil() as u64).max(1));

    let status = Command::new(&xctrace)
        .args([
            "record",
            "--template",
            "Time Profiler",
            "--target-pid",
            &pid.to_string(),
            "--time-limit",
            &dur_str,
            "--output",
            output_path.to_str().unwrap_or("/tmp/pa.xctrace"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .map_err(|e| RecordError::XctraceError(format!("failed to run xctrace: {e}")))?;

    if !status.success() {
        return Err(RecordError::XctraceError(format!(
            "xctrace record exited {:?}",
            status.code()
        )));
    }

    // Export time-sample table as JSON (requires xctrace from Xcode 13+).
    let export = Command::new(&xctrace)
        .args([
            "export",
            "--input",
            output_path.to_str().unwrap_or("/tmp/pa.xctrace"),
            "--xpath",
            "/trace-toc/run[@number='1']/data/table[@schema='time-sample']",
            "--format",
            "json",
            "--output",
            "-",
        ])
        .output()
        .map_err(|e| RecordError::XctraceError(format!("xctrace export failed: {e}")))?;

    if export.stdout.is_empty() {
        warn!("xctrace export produced no output; trace may be empty");
        return Ok(TraceData::default());
    }

    parse_xctrace_json(&export.stdout, pid)
}

// ---------------------------------------------------------------------------
// JSON parser for xctrace export
// ---------------------------------------------------------------------------

/// Best-effort parser for the xctrace JSON time-sample export.
///
/// xctrace's JSON structure (Xcode 13+):
/// ```json
/// {"rows": [
///   {"timestamp": 1234567890, "thread": 42, "backtrace": [
///     {"addr": "0x1a2b3c4d", "symbol": "foo"},
///     ...
///   ]},
///   ...
/// ]}
/// ```
///
/// We use a hand-written parser to avoid pulling in `serde_json` as a dep.
/// The format is regular enough for simple scan-based extraction.
fn parse_xctrace_json(json: &[u8], pid: u32) -> Result<TraceData, RecordError> {
    let text = std::str::from_utf8(json)
        .map_err(|e| RecordError::XctraceError(format!("xctrace JSON not UTF-8: {e}")))?;

    let mut samples = Vec::new();

    // Walk through the text finding each `"timestamp":` occurrence.
    // Use an offset cursor to avoid borrow conflicts.
    const TS_KEY: &str = "\"timestamp\":";
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find(TS_KEY) {
        let row_start = cursor + rel;
        let row_slice = &text[row_start..];

        let ts = extract_number_after(row_slice, TS_KEY);
        let tid = extract_number_after(row_slice, "\"thread\":");
        let frames = extract_backtrace_addrs(row_slice);

        if let (Some(ts), Some(tid)) = (ts, tid) {
            samples.push(StackSample { timestamp_ns: ts, pid, tid, frames });
        }

        // Advance past this key so we don't re-match it.
        cursor = row_start + TS_KEY.len();
    }

    debug!("xctrace: parsed {} time samples for pid {pid}", samples.len());

    Ok(TraceData {
        stack_samples: samples,
        ..Default::default()
    })
}

/// Extract the first integer value after `key` in `text`.
fn extract_number_after(text: &str, key: &str) -> Option<u64> {
    let pos = text.find(key)? + key.len();
    let rest = text[pos..].trim_start_matches([' ', '\t', '\n', '\r']);
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Extract raw instruction-pointer addresses from a `"backtrace"` JSON array.
///
/// xctrace emits addresses as hex strings (`"0x1a2b3c4d"`) under `"addr"` keys.
/// We collect only the addresses in order.
fn extract_backtrace_addrs(text: &str) -> Vec<u64> {
    let mut addrs = Vec::new();
    let bt_start = match text.find("\"backtrace\"") {
        Some(p) => p,
        None => return addrs,
    };
    // Find the next closing ']' to bound the search.
    let bt_region = &text[bt_start..];
    let bt_end = bt_region.find(']').map(|p| p + 1).unwrap_or(bt_region.len());
    let bt_region = &bt_region[..bt_end];

    const ADDR_KEY: &str = "\"addr\":";
    let mut cursor = 0usize;
    while let Some(rel) = bt_region[cursor..].find(ADDR_KEY) {
        let val_start = cursor + rel + ADDR_KEY.len();
        let rest = bt_region[val_start..].trim_start_matches(|c: char| c == ' ' || c == '\t' || c == '"');
        let hex_part = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X"));
        if let Some(hex_str) = hex_part {
            let end = hex_str.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(hex_str.len());
            if let Ok(addr) = u64::from_str_radix(&hex_str[..end], 16) {
                addrs.push(addr);
            }
        }
        cursor = cursor + rel + ADDR_KEY.len();
    }

    addrs
}

// ---------------------------------------------------------------------------
// Locate xctrace binary
// ---------------------------------------------------------------------------

fn find_xctrace() -> Result<PathBuf, RecordError> {
    // Common locations: Xcode.app bundle or the standalone CLT install.
    let candidates = [
        "/usr/bin/xctrace",
        "/Applications/Xcode.app/Contents/Developer/usr/bin/xctrace",
    ];
    for candidate in &candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    // Fall back: ask `xcrun` (handles multiple Xcode installations).
    let out = Command::new("xcrun")
        .args(["--find", "xctrace"])
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if !s.is_empty() {
                return Ok(PathBuf::from(s));
            }
        }
    }

    Err(RecordError::XctraceNotFound)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_number_basic() {
        assert_eq!(extract_number_after(r#""timestamp": 1234567"#, "\"timestamp\":"), Some(1234567));
    }

    #[test]
    fn extract_addrs_from_backtrace() {
        let json = r#""backtrace": [{"addr": "0x1000abcd"}, {"addr": "0x2000ef01"}]"#;
        let addrs = extract_backtrace_addrs(json);
        assert_eq!(addrs, vec![0x1000abcd_u64, 0x2000ef01_u64]);
    }

    #[test]
    fn parse_minimal_json() {
        let json = br#"{"rows": [{"timestamp": 1718000000000, "thread": 42,
            "backtrace": [{"addr": "0xdeadbeef"}]}]}"#;
        let data = parse_xctrace_json(json, 99).unwrap();
        assert_eq!(data.stack_samples.len(), 1);
        assert_eq!(data.stack_samples[0].tid, 42);
        assert_eq!(data.stack_samples[0].frames[0], 0xdeadbeef);
    }
}
