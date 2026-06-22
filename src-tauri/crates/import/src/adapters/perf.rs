/// Adapter for Linux `perf.data` files produced by `perf record`.
///
/// Only little-endian files are supported (the dominant encoding).
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use trace_core::RawEvent;
use trace_core::event::{
    CpuSampleEvent, ProcessInfoEvent, SchedulingEvent, SchedulingEventType, StackEntryEvent,
};

use crate::error::ImportError;
use crate::ImportResult;

const PERF_RECORD_COMM: u32            = 3;
const PERF_RECORD_FORK: u32            = 7;
const PERF_RECORD_SAMPLE: u32          = 9;
const PERF_RECORD_EXIT: u32            = 13;
const PERF_RECORD_SWITCH: u32          = 14;
const PERF_RECORD_SWITCH_CPU_WIDE: u32 = 15;
const PERF_RECORD_MISC_SWITCH_OUT: u16 = 1 << 13;

const PERF_SAMPLE_IP: u64         = 1 << 0;
const PERF_SAMPLE_TID: u64        = 1 << 1;
const PERF_SAMPLE_TIME: u64       = 1 << 2;
const PERF_SAMPLE_ADDR: u64       = 1 << 3;
const PERF_SAMPLE_READ: u64       = 1 << 4;
const PERF_SAMPLE_CALLCHAIN: u64  = 1 << 5;
const PERF_SAMPLE_ID: u64         = 1 << 6;
const PERF_SAMPLE_CPU: u64        = 1 << 7;
const PERF_SAMPLE_PERIOD: u64     = 1 << 8;
const PERF_SAMPLE_STREAM_ID: u64  = 1 << 9;
const PERF_SAMPLE_IDENTIFIER: u64 = 1 << 16;

const HEADER_ATTR_SIZE_OFFSET: usize = 16;
const HEADER_ATTRS_SECTION:    usize = 24;
const HEADER_DATA_SECTION:     usize = 40;
const PERF_FILE_HEADER_SIZE:   usize = 104;
const ATTR_SAMPLE_TYPE_OFFSET: usize = 24;

static NEXT_STACK_ID: AtomicU64 = AtomicU64::new(1);
fn alloc_stack_id() -> u64 { NEXT_STACK_ID.fetch_add(1, Ordering::Relaxed) }

#[derive(Clone, Copy)]
struct FileSection { offset: u64, size: u64 }

pub async fn import(path: &Path) -> Result<ImportResult, ImportError> {
    let path_owned = path.to_path_buf();
    let mut file = tokio::fs::File::open(path).await.map_err(|e| ImportError::Io {
        path: path_owned.clone(), source: e,
    })?;
    let (attr_size, attrs, data) = parse_file_header(&mut file, &path_owned).await?;
    let sample_type = read_sample_type(&mut file, attrs, attr_size).await.unwrap_or(0);
    file.seek(std::io::SeekFrom::Start(data.offset)).await
        .map_err(|e| ImportError::Io { path: path_owned.clone(), source: e })?;

    let (tx, rx) = mpsc::channel::<Result<RawEvent, ImportError>>(512);
    tokio::spawn(async move {
        stream_events(file, path_owned, data.size, sample_type, tx).await;
    });
    Ok(ImportResult {
        events: Box::pin(ReceiverStream::new(rx)),
        is_complete: true,
        boundary_offset: None,
    })
}

async fn parse_file_header(
    file: &mut tokio::fs::File,
    path: &Path,
) -> Result<(u64, FileSection, FileSection), ImportError> {
    let mut buf = [0u8; PERF_FILE_HEADER_SIZE];
    file.read_exact(&mut buf).await
        .map_err(|e| ImportError::Io { path: path.to_path_buf(), source: e })?;
    let magic = &buf[0..8];
    if magic != b"PERFILE2" && magic != b"PERFILE " {
        return Err(ImportError::InvalidFile {
            path: path.to_path_buf(),
            format: crate::TraceFormat::PerfData,
            reason: format!("unexpected magic {:?}", std::str::from_utf8(magic).unwrap_or("<binary>")),
        });
    }
    let attr_size = u64::from_le_bytes(buf[HEADER_ATTR_SIZE_OFFSET..HEADER_ATTR_SIZE_OFFSET+8].try_into().unwrap());
    let attrs = FileSection {
        offset: u64::from_le_bytes(buf[HEADER_ATTRS_SECTION..HEADER_ATTRS_SECTION+8].try_into().unwrap()),
        size:   u64::from_le_bytes(buf[HEADER_ATTRS_SECTION+8..HEADER_ATTRS_SECTION+16].try_into().unwrap()),
    };
    let data = FileSection {
        offset: u64::from_le_bytes(buf[HEADER_DATA_SECTION..HEADER_DATA_SECTION+8].try_into().unwrap()),
        size:   u64::from_le_bytes(buf[HEADER_DATA_SECTION+8..HEADER_DATA_SECTION+16].try_into().unwrap()),
    };
    Ok((attr_size, attrs, data))
}

async fn read_sample_type(
    file: &mut tokio::fs::File,
    attrs: FileSection,
    attr_size: u64,
) -> Option<u64> {
    if attrs.size == 0 || attr_size < (ATTR_SAMPLE_TYPE_OFFSET as u64 + 8) { return None; }
    file.seek(std::io::SeekFrom::Start(attrs.offset)).await.ok()?;
    let mut buf = vec![0u8; attr_size.min(256) as usize];
    file.read_exact(&mut buf).await.ok()?;
    if buf.len() < ATTR_SAMPLE_TYPE_OFFSET + 8 { return None; }
    Some(u64::from_le_bytes(buf[ATTR_SAMPLE_TYPE_OFFSET..ATTR_SAMPLE_TYPE_OFFSET+8].try_into().ok()?))
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
        if bytes_read >= data_size { break; }
        let mut hdr = [0u8; 8];
        match file.read_exact(&mut hdr).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                let _ = tx.send(Err(ImportError::PartialCorruption {
                    path, boundary_offset: bytes_read, reason: e.to_string(),
                })).await;
                return;
            }
        }
        let ev_type = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
        let misc    = u16::from_le_bytes(hdr[4..6].try_into().unwrap());
        let ev_size = u16::from_le_bytes(hdr[6..8].try_into().unwrap());
        if ev_size < 8 {
            let _ = tx.send(Err(ImportError::PartialCorruption {
                path, boundary_offset: bytes_read,
                reason: format!("event size {ev_size} < 8"),
            })).await;
            return;
        }
        let mut payload = vec![0u8; (ev_size - 8) as usize];
        match file.read_exact(&mut payload).await {
            Ok(_) => {}
            Err(e) => {
                let _ = tx.send(Err(ImportError::PartialCorruption {
                    path, boundary_offset: bytes_read + 8, reason: e.to_string(),
                })).await;
                return;
            }
        }
        bytes_read += ev_size as u64;
        for ev in decode_event(ev_type, misc, &payload, sample_type) {
            if tx.send(Ok(ev)).await.is_err() { return; }
        }
    }
}

fn decode_event(ev_type: u32, misc: u16, payload: &[u8], sample_type: u64) -> Vec<RawEvent> {
    match ev_type {
        PERF_RECORD_FORK  => decode_fork(payload).into_iter().collect(),
        PERF_RECORD_EXIT  => decode_exit(payload).into_iter().collect(),
        PERF_RECORD_COMM  => decode_comm(payload).into_iter().collect(),
        PERF_RECORD_SWITCH | PERF_RECORD_SWITCH_CPU_WIDE =>
            decode_switch(misc, payload, ev_type == PERF_RECORD_SWITCH_CPU_WIDE).into_iter().collect(),
        PERF_RECORD_SAMPLE => decode_sample(payload, sample_type),
        _ => vec![],
    }
}

fn r32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off+4).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes)
}
fn r64(b: &[u8], off: usize) -> Option<u64> {
    b.get(off..off+8).and_then(|s| s.try_into().ok()).map(u64::from_le_bytes)
}

fn decode_fork(p: &[u8]) -> Option<RawEvent> {
    if p.len() < 24 { return None; }
    Some(RawEvent::Process(ProcessInfoEvent {
        process_id: r32(p, 0)?, parent_process_id: r32(p, 4),
        name: String::new(), cmdline: None,
        start_time_ns: r64(p, 16).unwrap_or(0) as i64,
        exit_time_ns: None, exit_code: None,
    }))
}

fn decode_exit(p: &[u8]) -> Option<RawEvent> {
    if p.len() < 24 { return None; }
    Some(RawEvent::Process(ProcessInfoEvent {
        process_id: r32(p, 0)?, parent_process_id: r32(p, 4),
        name: String::new(), cmdline: None, start_time_ns: 0,
        exit_time_ns: Some(r64(p, 16).unwrap_or(0) as i64),
        exit_code: Some(0),
    }))
}

fn decode_comm(p: &[u8]) -> Option<RawEvent> {
    if p.len() < 8 { return None; }
    let name = p.get(8..).and_then(|s| {
        let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        std::str::from_utf8(&s[..end]).ok().map(|s| s.to_owned())
    }).unwrap_or_default();
    Some(RawEvent::Process(ProcessInfoEvent {
        process_id: r32(p, 0)?, parent_process_id: None,
        name, cmdline: None, start_time_ns: 0, exit_time_ns: None, exit_code: None,
    }))
}

fn decode_switch(misc: u16, payload: &[u8], cpu_wide: bool) -> Option<RawEvent> {
    let event_type = if (misc & PERF_RECORD_MISC_SWITCH_OUT) != 0 {
        SchedulingEventType::ContextSwitchOut
    } else {
        SchedulingEventType::ContextSwitchIn
    };
    let (next_pid, next_tid) = if cpu_wide && payload.len() >= 8 {
        (r32(payload, 0), r32(payload, 4))
    } else {
        (None, None)
    };
    Some(RawEvent::Scheduling(SchedulingEvent {
        timestamp_ns: 0, process_id: 0, thread_id: 0, cpu_id: 0,
        event_type, prev_state: None,
        next_process_id: next_pid, next_thread_id: next_tid, duration_ns: None,
    }))
}

fn decode_sample(payload: &[u8], sample_type: u64) -> Vec<RawEvent> {
    decode_sample_inner(payload, sample_type).unwrap_or_default()
}

fn decode_sample_inner(payload: &[u8], sample_type: u64) -> Option<Vec<RawEvent>> {
    let mut off = 0usize;
    macro_rules! r64p { () => {{ let v = r64(payload, off)?; off += 8; v }}; }
    macro_rules! r32p { () => {{ let v = r32(payload, off)?; off += 4; v }}; }

    if sample_type & PERF_SAMPLE_IDENTIFIER != 0 { r64p!(); }
    let ip   = if sample_type & PERF_SAMPLE_IP  != 0 { r64p!() } else { 0 };
    let pid  = if sample_type & PERF_SAMPLE_TID != 0 { r32p!() } else { 0 };
    let tid  = if sample_type & PERF_SAMPLE_TID != 0 { r32p!() } else { 0 };
    let time = if sample_type & PERF_SAMPLE_TIME != 0 { r64p!() } else { 0 };
    if sample_type & PERF_SAMPLE_ADDR      != 0 { r64p!(); }
    if sample_type & PERF_SAMPLE_ID        != 0 { r64p!(); }
    if sample_type & PERF_SAMPLE_STREAM_ID != 0 { r64p!(); }
    let cpu_id = if sample_type & PERF_SAMPLE_CPU != 0 {
        let c = r32p!() as u32; r32p!(); c
    } else { 0 };
    if sample_type & PERF_SAMPLE_PERIOD != 0 { r64p!(); }
    if sample_type & PERF_SAMPLE_READ   != 0 {
        return Some(vec![RawEvent::CpuSample(CpuSampleEvent {
            timestamp_ns: time as i64, process_id: pid, thread_id: tid,
            cpu_id, sample_weight: 1, stack_id: None,
        })]);
    }
    let call_stack: Vec<u64> = if sample_type & PERF_SAMPLE_CALLCHAIN != 0 {
        let nr = (r64p!() as usize).min((payload.len().saturating_sub(off)) / 8);
        let mut v = Vec::with_capacity(nr);
        for _ in 0..nr { if let Some(a) = r64(payload, off) { off += 8; v.push(a); } else { break; } }
        v
    } else {
        vec![]
    };

    let mut events = Vec::new();
    let stack_id = if call_stack.is_empty() {
        None
    } else {
        let sid = alloc_stack_id();
        let frames: Vec<u64> = std::iter::once(ip).chain(call_stack).collect();
        for (depth, addr) in frames.into_iter().enumerate() {
            events.push(RawEvent::StackEntry(StackEntryEvent {
                stack_id: sid, depth: depth as u32, frame_id: addr,
            }));
        }
        Some(sid)
    };
    events.push(RawEvent::CpuSample(CpuSampleEvent {
        timestamp_ns: time as i64, process_id: pid, thread_id: tid,
        cpu_id, sample_weight: 1, stack_id,
    }));
    Some(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use trace_core::event::SchedulingEventType;

    fn make_file_header(data_offset: u64, data_size: u64) -> Vec<u8> {
        let mut h = vec![0u8; PERF_FILE_HEADER_SIZE];
        h[0..8].copy_from_slice(b"PERFILE2");
        h[8..16].copy_from_slice(&(PERF_FILE_HEADER_SIZE as u64).to_le_bytes());
        h[HEADER_ATTRS_SECTION..HEADER_ATTRS_SECTION+8].copy_from_slice(&(PERF_FILE_HEADER_SIZE as u64).to_le_bytes());
        h[HEADER_DATA_SECTION..HEADER_DATA_SECTION+8].copy_from_slice(&data_offset.to_le_bytes());
        h[HEADER_DATA_SECTION+8..HEADER_DATA_SECTION+16].copy_from_slice(&data_size.to_le_bytes());
        h
    }

    fn exit_record_bytes(pid: u32, ppid: u32, time_ns: u64) -> Vec<u8> {
        let mut r = vec![0u8; 32];
        r[0..4].copy_from_slice(&PERF_RECORD_EXIT.to_le_bytes());
        r[6..8].copy_from_slice(&32u16.to_le_bytes());
        r[8..12].copy_from_slice(&pid.to_le_bytes());
        r[12..16].copy_from_slice(&ppid.to_le_bytes());
        r[24..32].copy_from_slice(&time_ns.to_le_bytes());
        r
    }

    #[tokio::test]
    async fn imports_exit_event() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("perf.data");
        let exit = exit_record_bytes(1234, 1, 9_000_000_000);
        let mut content = make_file_header(PERF_FILE_HEADER_SIZE as u64, exit.len() as u64);
        content.extend_from_slice(&exit);
        tokio::fs::write(&p, &content).await.unwrap();

        use futures::StreamExt as _;
        let events: Vec<_> = import(&p).await.unwrap().events.collect().await;
        assert_eq!(events.len(), 1);
        if let RawEvent::Process(e) = events[0].as_ref().unwrap() {
            assert_eq!(e.process_id, 1234);
            assert!(e.exit_time_ns.is_some());
        } else {
            panic!("expected Process event, got {:?}", events[0]);
        }
    }

    #[test]
    fn switch_out_event_type() {
        if let RawEvent::Scheduling(s) = decode_switch(PERF_RECORD_MISC_SWITCH_OUT, &[], false).unwrap() {
            assert_eq!(s.event_type, SchedulingEventType::ContextSwitchOut);
        } else { panic!(); }
    }

    #[test]
    fn switch_in_event_type() {
        if let RawEvent::Scheduling(s) = decode_switch(0, &[], false).unwrap() {
            assert_eq!(s.event_type, SchedulingEventType::ContextSwitchIn);
        } else { panic!(); }
    }
}
