/// Adapter for Windows ETW trace log files (.etl).
///
/// Cross-platform best-effort parser: extracts timestamp, pid, tid from
/// the `EVENT_HEADER` structure in each ETL buffer, emitting one
/// `CpuSample` per event. Full event decoding (by provider GUID via TDH)
/// is Windows-only and out of scope for this cross-platform import path.
///
/// Format references: <evntrace.h> (Windows SDK), <wmistr.h>, MS-ETW ETLF spec.
use std::path::Path;

use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use trace_core::RawEvent;
use trace_core::event::CpuSampleEvent;

use crate::error::ImportError;
use crate::ImportResult;

// WMI_BUFFER_HEADER — first 4 bytes are BufferSize (u32 LE).
// Valid ETL page sizes are powers of two from 4 KB to 64 KB.
const VALID_BUFFER_SIZES: &[u32] = &[4096, 8192, 16384, 32768, 65536];
const ETL_BUFFER_HEADER_SIZE: usize = 64;

// EVENT_HEADER offsets within each event record.
const ETL_THREAD_ID_OFFSET:    usize = 8;
const ETL_PROCESS_ID_OFFSET:   usize = 12;
const ETL_TIMESTAMP_OFFSET:    usize = 16;
const ETL_EVENT_HEADER_SIZE:   usize = 80;

// Windows FILETIME → Unix nanoseconds.
// FILETIME = 100-ns ticks since 1601-01-01T00:00:00Z.
const FILETIME_TO_UNIX_OFFSET_100NS: u64 = 116_444_736_000_000_000;

pub fn filetime_to_unix_ns(filetime: u64) -> u64 {
    filetime
        .saturating_sub(FILETIME_TO_UNIX_OFFSET_100NS)
        .saturating_mul(100)
}

pub async fn import(path: &Path) -> Result<ImportResult, ImportError> {
    let path_owned = path.to_path_buf();
    let mut file = tokio::fs::File::open(path).await.map_err(|e| ImportError::Io {
        path: path_owned.clone(), source: e,
    })?;

    let mut size_buf = [0u8; 4];
    file.read_exact(&mut size_buf).await.map_err(|e| ImportError::InvalidFile {
        path: path_owned.clone(), format: crate::TraceFormat::Etl,
        reason: format!("could not read buffer size: {e}"),
    })?;
    let buffer_size = u32::from_le_bytes(size_buf);

    if !VALID_BUFFER_SIZES.contains(&buffer_size) {
        return Err(ImportError::InvalidFile {
            path: path_owned,
            format: crate::TraceFormat::Etl,
            reason: format!(
                "first 4 bytes {buffer_size:#x} are not a valid ETL buffer size \
                 (expected one of {VALID_BUFFER_SIZES:?})"
            ),
        });
    }

    file.seek(std::io::SeekFrom::Start(0))
        .await
        .map_err(|e| ImportError::Io { path: path_owned.clone(), source: e })?;

    let (tx, rx) = mpsc::channel::<Result<RawEvent, ImportError>>(256);
    tokio::spawn(async move {
        stream_etl_buffers(file, path_owned, buffer_size as usize, tx).await;
    });

    Ok(ImportResult {
        events: Box::pin(ReceiverStream::new(rx)),
        is_complete: true,
        boundary_offset: None,
    })
}

async fn stream_etl_buffers(
    mut file: tokio::fs::File,
    path: std::path::PathBuf,
    buffer_size: usize,
    tx: mpsc::Sender<Result<RawEvent, ImportError>>,
) {
    let mut buf = vec![0u8; buffer_size];
    let mut file_offset: u64 = 0;

    loop {
        match file.read_exact(&mut buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                let _ = tx.send(Err(ImportError::PartialCorruption {
                    path, boundary_offset: file_offset, reason: e.to_string(),
                })).await;
                return;
            }
        }
        // SavedOffset (bytes 4-7): end of valid event data in this buffer; 0 = full.
        let saved_offset = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
        let event_end = if saved_offset == 0 || saved_offset > buffer_size {
            buffer_size
        } else {
            saved_offset
        };
        let events_slice = &buf[ETL_BUFFER_HEADER_SIZE..event_end];
        if parse_buffer_events(events_slice, &tx).await { return; }
        file_offset += buffer_size as u64;
    }
}

/// Returns `true` if the receiver is closed (caller should stop).
async fn parse_buffer_events(
    data: &[u8],
    tx: &mpsc::Sender<Result<RawEvent, ImportError>>,
) -> bool {
    let mut off = 0usize;
    while off + 2 <= data.len() {
        let ev_size = u16::from_le_bytes(data[off..off+2].try_into().unwrap()) as usize;
        if ev_size == 0 { break; } // end-of-events marker
        if ev_size < ETL_EVENT_HEADER_SIZE || off + ev_size > data.len() { break; }
        if let Some(ev) = decode_etl_record(&data[off..off+ev_size]) {
            if tx.send(Ok(ev)).await.is_err() { return true; }
        }
        off += ev_size;
        // 8-byte record alignment
        off = (off + 7) & !7;
    }
    false
}

fn decode_etl_record(record: &[u8]) -> Option<RawEvent> {
    if record.len() < ETL_EVENT_HEADER_SIZE { return None; }
    let tid = u32::from_le_bytes(record[ETL_THREAD_ID_OFFSET..ETL_THREAD_ID_OFFSET+4].try_into().ok()?);
    let pid = u32::from_le_bytes(record[ETL_PROCESS_ID_OFFSET..ETL_PROCESS_ID_OFFSET+4].try_into().ok()?);
    let filetime = u64::from_le_bytes(record[ETL_TIMESTAMP_OFFSET..ETL_TIMESTAMP_OFFSET+8].try_into().ok()?);
    Some(RawEvent::CpuSample(CpuSampleEvent {
        timestamp_ns: filetime_to_unix_ns(filetime) as i64,
        process_id: pid,
        thread_id: tid,
        cpu_id: 0,
        sample_weight: 1,
        stack_id: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_converts_to_unix_ns() {
        // 2000-01-01T00:00:00Z = FILETIME 125911584000000000
        let unix_ns = filetime_to_unix_ns(125_911_584_000_000_000);
        let expected = 946_684_800_000_000_000u64; // 2000-01-01 in ns
        assert!((unix_ns as i64 - expected as i64).unsigned_abs() <= 100,
            "got {unix_ns}, expected {expected}");
    }

    #[test]
    fn filetime_before_epoch_saturates() {
        assert_eq!(filetime_to_unix_ns(FILETIME_TO_UNIX_OFFSET_100NS - 1000), 0);
    }

    #[test]
    fn invalid_buffer_size_rejected() {
        assert!(!VALID_BUFFER_SIZES.contains(&1));
        assert!(VALID_BUFFER_SIZES.contains(&4096));
    }

    fn make_etl_record(pid: u32, tid: u32, filetime: u64) -> Vec<u8> {
        let mut r = vec![0u8; ETL_EVENT_HEADER_SIZE];
        r[0..2].copy_from_slice(&(ETL_EVENT_HEADER_SIZE as u16).to_le_bytes());
        r[ETL_THREAD_ID_OFFSET..ETL_THREAD_ID_OFFSET+4].copy_from_slice(&tid.to_le_bytes());
        r[ETL_PROCESS_ID_OFFSET..ETL_PROCESS_ID_OFFSET+4].copy_from_slice(&pid.to_le_bytes());
        r[ETL_TIMESTAMP_OFFSET..ETL_TIMESTAMP_OFFSET+8].copy_from_slice(&filetime.to_le_bytes());
        r
    }

    #[test]
    fn decode_extracts_pid_tid_timestamp() {
        let ft = 125_911_584_000_000_000u64;
        let record = make_etl_record(999, 1000, ft);
        if let RawEvent::CpuSample(ev) = decode_etl_record(&record).unwrap() {
            assert_eq!(ev.process_id, 999);
            assert_eq!(ev.thread_id, 1000);
            assert_eq!(ev.timestamp_ns, filetime_to_unix_ns(ft) as i64);
        } else { panic!(); }
    }

    #[test]
    fn too_short_record_returns_none() {
        assert!(decode_etl_record(&vec![0u8; ETL_EVENT_HEADER_SIZE - 1]).is_none());
    }
}
