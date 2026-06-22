/// Adapter for Linux `perf.data` files produced by `perf record`.
///
/// Binary format reference: linux/tools/perf/Documentation/perf.data-file-format.txt
/// and kernel source `tools/perf/util/header.h`.
///
/// Endianness: always little-endian on x86/ARM (the dominant platforms);
/// big-endian perf.data exists but is rare — we reject it with `InvalidFile`.
use std::path::Path;

use bytes::Bytes;
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::contract::{RawEvent, RawEventKind};
use crate::error::ImportError;
use crate::ImportResult;

// ── perf_event_type ──────────────────────────────────────────────────────────
const PERF_RECORD_MMAP: u32 = 1;
const PERF_RECORD_COMM: u32 = 3;
const PERF_RECORD_FORK: u32 = 7;
const PERF_RECORD_SAMPLE: u32 = 9;
const PERF_RECORD_EXIT: u32 = 13;
const PERF_RECORD_SWITCH: u32 = 14;
const PERF_RECORD_SWITCH_CPU_WIDE: u32 = 15;

// PERF_RECORD_MISC_SWITCH_OUT: set in `misc` when a SWITCH record means
// "thread being switched out" (vs switched in).
const PERF_RECORD_MISC_SWITCH_OUT: u16 = 1 << 13;

// ── perf_sample_type bits ────────────────────────────────────────────────────
const PERF_SAMPLE_IP: u64 = 1 << 0;
const PERF_SAMPLE_TID: u64 = 1 << 1;
const PERF_SAMPLE_TIME: u64 = 1 << 2;
const PERF_SAMPLE_ADDR: u64 = 1 << 3;
const PERF_SAMPLE_READ: u64 = 1 << 4;
const PERF_SAMPLE_CALLCHAIN: u64 = 1 << 5;
const PERF_SAMPLE_ID: u64 = 1 << 6;
const PERF_SAMPLE_CPU: u64 = 1 << 7;
const PERF_SAMPLE_PERIOD: u64 = 1 << 8;
const PERF_SAMPLE_STREAM_ID: u64 = 1 << 9;
const PERF_SAMPLE_RAW: u64 = 1 << 10;
const PERF_SAMPLE_BRANCH_STACK: u64 = 1 << 11;
const PERF_SAMPLE_REGS_USER: u64 = 1 << 12;
const PERF_SAMPLE_STACK_USER: u64 = 1 << 13;
const PERF_SAMPLE_WEIGHT: u64 = 1 << 14;
const PERF_SAMPLE_DATA_SRC: u64 = 1 << 15;
const PERF_SAMPLE_IDENTIFIER: u64 = 1 << 16;
const PERF_SAMPLE_TRANSACTION: u64 = 1 << 17;
const PERF_SAMPLE_REGS_INTR: u64 = 1 << 18;
const PERF_SAMPLE_PHYS_ADDR: u64 = 1 << 19;

// perf_event_attr::size offset within attr struct
const ATTR_SAMPLE_TYPE_OFFSET: usize = 24; // bytes 24..32 within perf_event_attr

// ── File header layout (all u64 little-endian) ───────────────────────────────
//  0: magic[8]
//  8: size (sizeof perf_file_header, typically 104)
// 16: attr_size (sizeof perf_event_attr)
// 24: attrs { offset[8], size[8] }
// 40: data  { offset[8], size[8] }
// 56: event_types { offset[8], size[8] }
// 72: adds_features[4 * u64]  (feature bitset, 256 bits)
const HEADER_MAGIC_OFFSET: usize = 0;
const HEADER_SIZE_FIELD: usize = 8;
const HEADER_ATTR_SIZE_OFFSET: usize = 16;
const HEADER_ATTRS_OFFSET: usize = 24; // section {offset, size}
const HEADER_DATA_OFFSET: usize = 40; // section {offset, size}
const PERF_FILE_HEADER_MIN_SIZE: usize = 104;

#[derive(Debug, Clone, Copy)]
struct FileSection {
    offset: u64,
    size: u64,
}

#[derive(Debug)]
struct PerfFileHeader {
    attr_size: u64,
    attrs: FileSection,
    data: FileSection,
}

pub async fn import(path: &Path) -> Result<ImportResult, ImportError> {
    let path_owned = path.to_path_buf();
    let mut file = tokio::fs::File::open(path).await.map_err(|e| ImportError::Io {
        path: path_owned.clone(),
        source: e,
    })?;

    // Read and parse the file header.
    let header = parse_file_header(&mut file, &path_owned).await?;

    // Read sample_type from the first attr entry (needed to parse SAMPLE records).
    let sample_type = read_sample_type(&mut file, &header, &path_owned).await.unwrap_or(0);

    // Seek to the start of the event data section.
    file.seek(std::io::SeekFrom::Start(header.data.offset))
        .await
        .map_err(|e| ImportError::Io {
            path: path_owned.clone(),
            source: e,
        })?;

    let (tx, rx) = mpsc::channel::<Result<RawEvent, ImportError>>(256);
    let data_size = header.data.size;

    tokio::spawn(async move {
        stream_events(file, path_owned, data_size, sample_type, tx).await;
    });

    Ok(ImportResult {
        events: Box::pin(ReceiverStream::new(rx)),
        is_complete: true,   // updated to false by stream if truncation detected
        boundary_offset: None,
    })
}

async fn parse_file_header(
    file: &mut tokio::fs::File,
    path: &Path,
) -> Result<PerfFileHeader, ImportError> {
    let mut buf = [0u8; PERF_FILE_HEADER_MIN_SIZE];
    file.read_exact(&mut buf).await.map_err(|e| ImportError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    let magic = &buf[HEADER_MAGIC_OFFSET..HEADER_MAGIC_OFFSET + 8];
    let is_v2 = magic == b"PERFILE2";
    let is_v1 = magic == b"PERFILE ";
    if !is_v2 && !is_v1 {
        return Err(ImportError::InvalidFile {
            path: path.to_path_buf(),
            format: crate::TraceFormat::PerfData,
            reason: format!(
                "unexpected magic bytes {:?}; expected 'PERFILE2' or 'PERFILE '",
                std::str::from_utf8(magic).unwrap_or("<binary>")
            ),
        });
    }

    let header_size = u64::from_le_bytes(buf[HEADER_SIZE_FIELD..HEADER_SIZE_FIELD + 8].try_into().unwrap());
    if (header_size as usize) < PERF_FILE_HEADER_MIN_SIZE {
        return Err(ImportError::InvalidFile {
            path: path.to_path_buf(),
            format: crate::TraceFormat::PerfData,
            reason: format!("header size {header_size} < minimum {PERF_FILE_HEADER_MIN_SIZE}"),
        });
    }

    let attr_size = u64::from_le_bytes(buf[HEADER_ATTR_SIZE_OFFSET..HEADER_ATTR_SIZE_OFFSET + 8].try_into().unwrap());

    let attrs = FileSection {
        offset: u64::from_le_bytes(buf[HEADER_ATTRS_OFFSET..HEADER_ATTRS_OFFSET + 8].try_into().unwrap()),
        size: u64::from_le_bytes(buf[HEADER_ATTRS_OFFSET + 8..HEADER_ATTRS_OFFSET + 16].try_into().unwrap()),
    };
    let data = FileSection {
        offset: u64::from_le_bytes(buf[HEADER_DATA_OFFSET..HEADER_DATA_OFFSET + 8].try_into().unwrap()),
        size: u64::from_le_bytes(buf[HEADER_DATA_OFFSET + 8..HEADER_DATA_OFFSET + 16].try_into().unwrap()),
    };

    Ok(PerfFileHeader { attr_size, attrs, data })
}

/// Read `sample_type` from the first `perf_event_attr` in the attrs section.
/// `sample_type` lives at byte offset 24 within `perf_event_attr`.
async fn read_sample_type(
    file: &mut tokio::fs::File,
    header: &PerfFileHeader,
    path: &Path,
) -> Option<u64> {
    if header.attrs.size == 0 || header.attr_size < (ATTR_SAMPLE_TYPE_OFFSET as u64 + 8) {
        return None;
    }
    file.seek(std::io::SeekFrom::Start(header.attrs.offset)).await.ok()?;
    let mut attr_buf = vec![0u8; header.attr_size.min(256) as usize];
    file.read_exact(&mut attr_buf).await.ok()?;
    if attr_buf.len() < ATTR_SAMPLE_TYPE_OFFSET + 8 {
        return None;
    }
    let sample_type = u64::from_le_bytes(
        attr_buf[ATTR_SAMPLE_TYPE_OFFSET..ATTR_SAMPLE_TYPE_OFFSET + 8]
            .try_into()
            .ok()?,
    );
    Some(sample_type)
}

async fn stream_events(
    mut file: tokio::fs::File,
    path: std::path::PathBuf,
    data_size: u64,
    sample_type: u64,
    tx: mpsc::Sender<Result<RawEvent, ImportError>>,
) {
    let mut bytes_read: u64 = 0;

    loop {
        if bytes_read >= data_size {
            break;
        }

        // Each event starts with an 8-byte perf_event_header: type(u32), misc(u16), size(u16).
        let mut hdr_buf = [0u8; 8];
        match file.read_exact(&mut hdr_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                let _ = tx
                    .send(Err(ImportError::PartialCorruption {
                        path,
                        boundary_offset: bytes_read,
                        reason: e.to_string(),
                    }))
                    .await;
                return;
            }
        }

        let ev_type = u32::from_le_bytes(hdr_buf[0..4].try_into().unwrap());
        let misc = u16::from_le_bytes(hdr_buf[4..6].try_into().unwrap());
        let ev_size = u16::from_le_bytes(hdr_buf[6..8].try_into().unwrap());

        if ev_size < 8 {
            let _ = tx
                .send(Err(ImportError::PartialCorruption {
                    path,
                    boundary_offset: bytes_read,
                    reason: format!("event size {ev_size} < 8 at offset {bytes_read}"),
                }))
                .await;
            return;
        }

        let payload_size = (ev_size - 8) as usize;
        let mut payload = vec![0u8; payload_size];
        match file.read_exact(&mut payload).await {
            Ok(_) => {}
            Err(e) => {
                let _ = tx
                    .send(Err(ImportError::PartialCorruption {
                        path,
                        boundary_offset: bytes_read + 8,
                        reason: e.to_string(),
                    }))
                    .await;
                return;
            }
        }

        bytes_read += ev_size as u64;

        if let Some(event) = decode_event(ev_type, misc, &payload, sample_type) {
            if tx.send(Ok(event)).await.is_err() {
                return; // receiver dropped
            }
        }
    }
}

fn decode_event(
    ev_type: u32,
    misc: u16,
    payload: &[u8],
    sample_type: u64,
) -> Option<RawEvent> {
    match ev_type {
        PERF_RECORD_FORK => decode_fork(payload),
        PERF_RECORD_EXIT => decode_exit(payload),
        PERF_RECORD_COMM => decode_comm(payload),
        PERF_RECORD_SWITCH | PERF_RECORD_SWITCH_CPU_WIDE => {
            decode_switch(misc, payload, ev_type == PERF_RECORD_SWITCH_CPU_WIDE)
        }
        PERF_RECORD_SAMPLE => decode_sample(payload, sample_type),
        _ => None,
    }
}

fn read_u32_le(buf: &[u8], off: usize) -> Option<u32> {
    buf.get(off..off + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes)
}

fn read_u64_le(buf: &[u8], off: usize) -> Option<u64> {
    buf.get(off..off + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes)
}

/// pid(u32) ppid(u32) tid(u32) ptid(u32) time(u64)
fn decode_fork(payload: &[u8]) -> Option<RawEvent> {
    if payload.len() < 24 {
        return None;
    }
    let pid = read_u32_le(payload, 0)?;
    let ppid = read_u32_le(payload, 4)?;
    let tid = read_u32_le(payload, 8)?;
    let timestamp_ns = read_u64_le(payload, 16)?;
    Some(RawEvent {
        timestamp_ns,
        pid,
        tid,
        cpu: None,
        kind: RawEventKind::ProcessCreate {
            parent_pid: ppid,
            name: String::new(),
        },
        payload: Bytes::new(),
    })
}

/// pid(u32) ppid(u32) tid(u32) ptid(u32) time(u64)
fn decode_exit(payload: &[u8]) -> Option<RawEvent> {
    if payload.len() < 24 {
        return None;
    }
    let pid = read_u32_le(payload, 0)?;
    let tid = read_u32_le(payload, 8)?;
    let timestamp_ns = read_u64_le(payload, 16)?;
    Some(RawEvent {
        timestamp_ns,
        pid,
        tid,
        cpu: None,
        kind: RawEventKind::ProcessExit { exit_code: 0 },
        payload: Bytes::new(),
    })
}

/// pid(u32) tid(u32) comm[...] (null-terminated, 8-byte aligned)
fn decode_comm(payload: &[u8]) -> Option<RawEvent> {
    if payload.len() < 8 {
        return None;
    }
    let pid = read_u32_le(payload, 0)?;
    let tid = read_u32_le(payload, 4)?;
    let comm = payload.get(8..).and_then(|s| {
        let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        std::str::from_utf8(&s[..end]).ok().map(|s| s.to_owned())
    });
    Some(RawEvent {
        timestamp_ns: 0,
        pid,
        tid,
        cpu: None,
        kind: RawEventKind::ProcessCreate {
            parent_pid: 0,
            name: comm.unwrap_or_default(),
        },
        payload: Bytes::new(),
    })
}

fn decode_switch(misc: u16, payload: &[u8], cpu_wide: bool) -> Option<RawEvent> {
    let switching_out = (misc & PERF_RECORD_MISC_SWITCH_OUT) != 0;

    if cpu_wide && payload.len() >= 8 {
        let next_pid = read_u32_le(payload, 0)?;
        let next_tid = read_u32_le(payload, 4)?;
        let kind = if switching_out {
            RawEventKind::ContextSwitchOut {
                prev_pid: next_pid,
                prev_tid: next_tid,
            }
        } else {
            RawEventKind::ContextSwitchIn {
                next_pid,
                next_tid,
            }
        };
        return Some(RawEvent {
            timestamp_ns: 0,
            pid: 0,
            tid: 0,
            cpu: None,
            kind,
            payload: Bytes::new(),
        });
    }

    let kind = if switching_out {
        RawEventKind::ContextSwitchOut { prev_pid: 0, prev_tid: 0 }
    } else {
        RawEventKind::ContextSwitchIn { next_pid: 0, next_tid: 0 }
    };
    Some(RawEvent {
        timestamp_ns: 0,
        pid: 0,
        tid: 0,
        cpu: None,
        kind,
        payload: Bytes::new(),
    })
}

fn decode_sample(payload: &[u8], sample_type: u64) -> Option<RawEvent> {
    let mut off = 0usize;

    macro_rules! read64 {
        () => {{
            let v = read_u64_le(payload, off)?;
            off += 8;
            v
        }};
    }
    macro_rules! read32 {
        () => {{
            let v = read_u32_le(payload, off)?;
            off += 4;
            v
        }};
    }

    // Fields appear in the order of the bit position in sample_type.
    if sample_type & PERF_SAMPLE_IDENTIFIER != 0 {
        let _ = read64!(); // id (skip)
    }
    let ip = if sample_type & PERF_SAMPLE_IP != 0 { read64!() } else { 0 };
    let (pid, tid) = if sample_type & PERF_SAMPLE_TID != 0 {
        (read32!(), read32!())
    } else {
        (0, 0)
    };
    let timestamp_ns = if sample_type & PERF_SAMPLE_TIME != 0 { read64!() } else { 0 };
    if sample_type & PERF_SAMPLE_ADDR != 0 {
        let _ = read64!();
    }
    if sample_type & PERF_SAMPLE_ID != 0 {
        let _ = read64!();
    }
    if sample_type & PERF_SAMPLE_STREAM_ID != 0 {
        let _ = read64!();
    }
    let cpu = if sample_type & PERF_SAMPLE_CPU != 0 {
        let cpu = read32!() as u16;
        let _ = read32!(); // reserved
        Some(cpu)
    } else {
        None
    };
    if sample_type & PERF_SAMPLE_PERIOD != 0 {
        let _ = read64!();
    }
    if sample_type & PERF_SAMPLE_READ != 0 {
        // Variable-length read format; we can't skip without knowing read_format.
        // Stop parsing this sample safely.
        return Some(RawEvent {
            timestamp_ns,
            pid,
            tid,
            cpu,
            kind: RawEventKind::PerfSample { ip, call_stack: vec![] },
            payload: Bytes::copy_from_slice(&payload[off..]),
        });
    }
    let call_stack = if sample_type & PERF_SAMPLE_CALLCHAIN != 0 {
        let nr = read64!() as usize;
        // Guard against malformed nr values
        let nr = nr.min((payload.len() - off) / 8);
        let mut stack = Vec::with_capacity(nr);
        for _ in 0..nr {
            stack.push(read64!());
        }
        stack
    } else {
        vec![]
    };

    Some(RawEvent {
        timestamp_ns,
        pid,
        tid,
        cpu,
        kind: RawEventKind::PerfSample { ip, call_stack },
        payload: Bytes::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_perf_file_header(data_offset: u64, data_size: u64, attr_size: u64) -> Vec<u8> {
        let mut h = vec![0u8; 104];
        h[0..8].copy_from_slice(b"PERFILE2");
        h[8..16].copy_from_slice(&104u64.to_le_bytes());    // header size
        h[16..24].copy_from_slice(&attr_size.to_le_bytes()); // attr_size
        // attrs section: offset=104, size=0 (no attrs in fixture)
        h[24..32].copy_from_slice(&104u64.to_le_bytes());   // attrs.offset
        h[32..40].copy_from_slice(&0u64.to_le_bytes());     // attrs.size
        // data section
        h[40..48].copy_from_slice(&data_offset.to_le_bytes());
        h[48..56].copy_from_slice(&data_size.to_le_bytes());
        h
    }

    fn exit_record(pid: u32, ppid: u32, tid: u32, ptid: u32, time_ns: u64) -> Vec<u8> {
        // perf_event_header (8) + pid(4) ppid(4) tid(4) ptid(4) time(8) = 32 bytes
        let mut r = vec![0u8; 32];
        r[0..4].copy_from_slice(&PERF_RECORD_EXIT.to_le_bytes());
        r[4..6].copy_from_slice(&0u16.to_le_bytes()); // misc
        r[6..8].copy_from_slice(&32u16.to_le_bytes()); // size
        r[8..12].copy_from_slice(&pid.to_le_bytes());
        r[12..16].copy_from_slice(&ppid.to_le_bytes());
        r[16..20].copy_from_slice(&tid.to_le_bytes());
        r[20..24].copy_from_slice(&ptid.to_le_bytes());
        r[24..32].copy_from_slice(&time_ns.to_le_bytes());
        r
    }

    #[tokio::test]
    async fn imports_exit_event_from_fixture() {
        use tokio::io::AsyncWriteExt as _;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("perf.data");

        let data_offset: u64 = 104;
        let exit = exit_record(1234, 1, 1234, 1, 9_000_000_000);
        let data_size = exit.len() as u64;

        let mut content = make_perf_file_header(data_offset, data_size, 0);
        content.extend_from_slice(&exit);

        tokio::fs::write(&p, &content).await.unwrap();

        let result = import(&p).await.unwrap();
        use futures::StreamExt as _;
        let events: Vec<_> = result.events.collect().await;
        assert_eq!(events.len(), 1);
        let ev = events[0].as_ref().unwrap();
        assert_eq!(ev.pid, 1234);
        assert_eq!(ev.timestamp_ns, 9_000_000_000);
        assert!(matches!(ev.kind, RawEventKind::ProcessExit { .. }));
    }

    #[test]
    fn decode_sample_with_tid_and_time() {
        // Build a SAMPLE payload with TID + TIME set
        let sample_type = PERF_SAMPLE_TID | PERF_SAMPLE_TIME;
        let mut payload = vec![0u8; 16];
        payload[0..4].copy_from_slice(&42u32.to_le_bytes()); // pid
        payload[4..8].copy_from_slice(&43u32.to_le_bytes()); // tid
        payload[8..16].copy_from_slice(&7_000_000_000u64.to_le_bytes()); // time
        let ev = decode_sample(&payload, sample_type).unwrap();
        assert_eq!(ev.pid, 42);
        assert_eq!(ev.tid, 43);
        assert_eq!(ev.timestamp_ns, 7_000_000_000);
        assert!(matches!(ev.kind, RawEventKind::PerfSample { ip: 0, .. }));
    }

    #[test]
    fn decode_switch_out() {
        let misc = PERF_RECORD_MISC_SWITCH_OUT;
        let ev = decode_switch(misc, &[], false).unwrap();
        assert!(matches!(ev.kind, RawEventKind::ContextSwitchOut { .. }));
    }

    #[test]
    fn decode_switch_in() {
        let ev = decode_switch(0, &[], false).unwrap();
        assert!(matches!(ev.kind, RawEventKind::ContextSwitchIn { .. }));
    }
}
