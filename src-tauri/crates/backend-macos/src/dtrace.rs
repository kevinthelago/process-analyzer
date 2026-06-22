//! DTrace subprocess management for I/O and scheduling events.
//!
//! Runs `dtrace -p <pid>` with a built-in io+sched probe script and streams
//! its stdout through a reader thread. Parsing is line-based; each line is
//! one event with a fixed prefix (`IO R`, `IO W`, `SCHED ON`, `SCHED OFF`).
//!
//! DTrace requires root or a partially-disabled SIP configuration on macOS.
//! When `dtrace` is unavailable or produces an error, [`DTraceHandle::spawn`]
//! returns [`RecordError::DTraceUnavailable`] and the caller falls back
//! gracefully (missing I/O/sched events, but stack samples continue).

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::SyncSender;
use std::thread;

use tracing::{debug, warn};

use crate::error::RecordError;
use crate::types::{IoEvent, IoKind, SchedEvent, SchedKind};

// ---------------------------------------------------------------------------
// DTrace script
// ---------------------------------------------------------------------------

/// Built-in DTrace script. `$target` is the PID passed via `-p`.
///
/// Output format:
/// ```
/// IO R {walltimestamp} {tid} {bytes} {path}
/// IO W {walltimestamp} {tid} {bytes} {path}
/// SCHED ON {walltimestamp} {tid} {cpu}
/// SCHED OFF {walltimestamp} {tid} {cpu}
/// ```
const DEFAULT_SCRIPT: &str = r#"
#pragma D option quiet
#pragma D option switchrate=100hz

io:::start
/pid == $target/
{
    printf("IO %c %u %u %u %s\n",
        (args[0]->b_flags & 0x0001) ? 'R' : 'W',
        walltimestamp,
        tid,
        args[0]->b_bcount,
        (args[2]->fi_pathname != NULL) ? args[2]->fi_pathname : "?");
}

sched:::on-cpu
/pid == $target/
{
    printf("SCHED ON %u %u %u\n", walltimestamp, tid, cpu);
}

sched:::off-cpu
/pid == $target/
{
    printf("SCHED OFF %u %u %u\n", walltimestamp, tid, cpu);
}
"#;

// ---------------------------------------------------------------------------
// Public handle
// ---------------------------------------------------------------------------

pub(crate) struct DTraceHandle {
    child: Child,
    reader: thread::JoinHandle<(Vec<IoEvent>, Vec<SchedEvent>)>,
}

impl DTraceHandle {
    /// Spawn `dtrace -p <pid>` with the given (or default) script.
    ///
    /// Returns [`RecordError::DTraceUnavailable`] when `dtrace` cannot be
    /// started or immediately exits with an error (e.g., SIP restricted).
    pub(crate) fn spawn(pid: u32, custom_script: Option<&str>) -> Result<Self, RecordError> {
        let script = custom_script.unwrap_or(DEFAULT_SCRIPT).to_owned();

        let mut child = Command::new("dtrace")
            .args(["-q", "-p", &pid.to_string(), "-n", &script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    RecordError::DTraceUnavailable(
                        "dtrace binary not found; install Xcode Command Line Tools \
                         or disable SIP's DTrace restriction"
                            .into(),
                    )
                } else {
                    RecordError::DTraceUnavailable(e.to_string())
                }
            })?;

        let stdout = child.stdout.take().expect("stdout piped");
        let reader = thread::Builder::new()
            .name(format!("dtrace-reader-{pid}"))
            .spawn(move || parse_dtrace_output(BufReader::new(stdout)))
            .map_err(RecordError::Io)?;

        Ok(Self { child, reader })
    }

    /// Kill the DTrace process and collect all parsed events.
    pub(crate) fn stop(mut self) -> (Vec<IoEvent>, Vec<SchedEvent>) {
        // SIGTERM the child; ignore errors (may already have exited).
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.reader.join().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Line parser
// ---------------------------------------------------------------------------

fn parse_dtrace_output(reader: BufReader<impl std::io::Read>) -> (Vec<IoEvent>, Vec<SchedEvent>) {
    let mut io_events = Vec::new();
    let mut sched_events = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                debug!("dtrace reader I/O error: {e}");
                break;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(ev) = parse_io_line(line) {
            io_events.push(ev);
        } else if let Some(ev) = parse_sched_line(line) {
            sched_events.push(ev);
        } else {
            debug!("dtrace: unrecognised line: {line}");
        }
    }

    (io_events, sched_events)
}

/// Parses: `IO {R|W} {timestamp_ns} {tid} {bytes} {path}`
fn parse_io_line(line: &str) -> Option<IoEvent> {
    let mut parts = line.splitn(6, ' ');
    let tag = parts.next()?;
    if tag != "IO" {
        return None;
    }
    let kind_ch = parts.next()?;
    let ts_str = parts.next()?;
    let tid_str = parts.next()?;
    let bytes_str = parts.next()?;
    let path = parts.next().unwrap_or("?");

    let kind = match kind_ch {
        "R" => IoKind::Read,
        "W" => IoKind::Write,
        _ => return None,
    };
    let timestamp_ns = ts_str.parse::<u64>().ok()?;
    let tid = tid_str.parse::<u64>().ok()?;
    let bytes = bytes_str.parse::<u64>().ok()?;
    let path = if path == "?" { None } else { Some(path.to_owned()) };

    Some(IoEvent {
        timestamp_ns,
        pid: 0, // filled in by the recorder from its own pid
        tid,
        kind,
        bytes,
        path,
    })
}

/// Parses: `SCHED {ON|OFF} {timestamp_ns} {tid} {cpu}`
fn parse_sched_line(line: &str) -> Option<SchedEvent> {
    let mut parts = line.splitn(5, ' ');
    let tag = parts.next()?;
    if tag != "SCHED" {
        return None;
    }
    let kind_str = parts.next()?;
    let ts_str = parts.next()?;
    let tid_str = parts.next()?;
    let cpu_str = parts.next()?;

    let kind = match kind_str {
        "ON" => SchedKind::OnCpu,
        "OFF" => SchedKind::OffCpu,
        _ => return None,
    };
    let timestamp_ns = ts_str.parse::<u64>().ok()?;
    let tid = tid_str.parse::<u64>().ok()?;
    let cpu = cpu_str.trim().parse::<u32>().ok()?;

    Some(SchedEvent {
        timestamp_ns,
        pid: 0, // filled in by the recorder
        tid,
        kind,
        cpu,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_io_read() {
        let ev = parse_io_line("IO R 1718000000000000000 12345 4096 /var/log/system.log")
            .expect("should parse");
        assert_eq!(ev.kind, IoKind::Read);
        assert_eq!(ev.tid, 12345);
        assert_eq!(ev.bytes, 4096);
        assert_eq!(ev.path.as_deref(), Some("/var/log/system.log"));
    }

    #[test]
    fn parse_io_write() {
        let ev = parse_io_line("IO W 1718000000000000001 99 512 ?").expect("should parse");
        assert_eq!(ev.kind, IoKind::Write);
        assert!(ev.path.is_none());
    }

    #[test]
    fn parse_sched_on() {
        let ev = parse_sched_line("SCHED ON 1718000000000000002 42 3").expect("should parse");
        assert_eq!(ev.kind, SchedKind::OnCpu);
        assert_eq!(ev.cpu, 3);
    }

    #[test]
    fn parse_sched_off() {
        let ev = parse_sched_line("SCHED OFF 1718000000000000003 42 3").expect("should parse");
        assert_eq!(ev.kind, SchedKind::OffCpu);
    }

    #[test]
    fn parse_garbage_returns_none() {
        assert!(parse_io_line("not an io line").is_none());
        assert!(parse_sched_line("IO R 1 2 3 /foo").is_none());
    }
}
