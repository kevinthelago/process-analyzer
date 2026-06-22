/// Adapter for Windows ETW trace log files (.etl).
///
/// ETL files are produced by `wpr.exe`, `xperf.exe`, or ETW's kernel logger.
/// On Windows, full decoding uses the TDH (Trace Data Helper) API which is
/// unavailable cross-platform. This adapter does best-effort binary parsing:
/// it extracts timestamp, pid, tid, provider GUID, and opcode from the
/// `EVENT_HEADER` structure present in every ETL buffer.
///
/// Format references:
/// - `<evntrace.h>` (Windows SDK) — EVENT_HEADER, EVENT_DESCRIPTOR
/// - `<wmistr.h>` — WMI_BUFFER_HEADER
/// - Microsoft Open Specifications MS-ETW (ETLF)
use std::path::Path;

use bytes::Bytes;
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::contract::{RawEvent, RawEventKind};
use crate::error::ImportError;
use crate::ImportResult;

// ── WMI_BUFFER_HEADER layout (little-endian, Windows 10+) ───────────────────
// [0]   BufferSize     u32  — page size (4096, 8192, 16384, 32768, or 65536)
// [4]   SavedOffset    u32  — byte offset of the end of valid data in this buffer
// [8]   CurrentOffset  u32
// [12]  ReferenceCount i32
// [16]  TimeStamp      i64  — Windows FILETIME (100-ns ticks since 1601-01-01)
// [24]  SequenceNumber i64
// [32]  (Clock union, 8 bytes)
// [40]  ClientContext (union: logger info, 8 bytes)
// [48]  Guid           [16 u8]  — logger session GUID
// [64]  end of header (next bytes are event records)
//
// Note: the header size varies slightly between Windows versions (NT 5.x vs 10.x).
// We use 64 bytes as the conservative minimum header size.
const ETL_BUFFER_HEADER_SIZE: usize = 64;

// ── EVENT_HEADER layout (from <evntrace.h>) ──────────────────────────────────
// [0]   Size           u16  — total size of this event record (header + payload)
// [2]   HeaderType     u16  — 0 = system, 1 = user-mode
// [4]   Flags          u16
// [6]   EventProperty  u16
// [8]   ThreadId       u32
// [12]  ProcessId      u32
// [16]  TimeStamp      i64  — Windows FILETIME
// [24]  ProviderId     [16 u8]  — provider GUID
// [40]  EventDescriptor:
//         Id      u16  [40]
//         Version u8   [42]
//         Channel u8   [43]
//         Level   u8   [44]
//         Opcode  u8   [45]
//         Task    u16  [46]
//         Keyword u64  [48]
// [56]  KernelTime / UserTime (u32 each, or ProcessorTime u64)
// [64]  ActivityId  [16 u8]
// total: 80 bytes
const ETL_EVENT_HEADER_SIZE: usize = 80;
const ETL_HEADER_TYPE_OFFSET: usize = 2;
const ETL_THREAD_ID_OFFSET: usize = 8;
const ETL_PROCESS_ID_OFFSET: usize = 12;
const ETL_TIMESTAMP_OFFSET: usize = 16;
const ETL_PROVIDER_ID_OFFSET: usize = 24;
const ETL_EVENT_ID_OFFSET: usize = 40;
const ETL_EVENT_VERSION_OFFSET: usize = 42;
const ETL_EVENT_LEVEL_OFFSET: usize = 44;
const ETL_EVENT_OPCODE_OFFSET: usize = 45;

// Windows FILETIME → Unix nanoseconds.
// FILETIME = 100-ns intervals since 1601-01-01T00:00:00Z.
// Unix epoch offset in 100-ns ticks: 116444736000000000.
const FILETIME_TO_UNIX_OFFSET_100NS: u64 = 116_444_736_000_000_000;

fn filetime_to_unix_ns(filetime: u64) -> u64 {
    // Saturate rather than panic on pre-epoch or overflow.
    filetime
        .saturating_sub(FILETIME_TO_UNIX_OFFSET_100NS)
        .saturating_mul(100)
}

/// Plausible ETL buffer sizes. Any first-buffer `BufferSize` field that isn't
/// one of these is likely a random binary file, not an ETL.
const VALID_BUFFER_SIZES: &[u32] = &[4096, 8192, 16384, 32768, 65536];

pub async fn import(path: &Path) -> Result<ImportResult, ImportError> {
    let path_owned = path.to_path_buf();
    let mut file = tokio::fs::File::open(path).await.map_err(|e| ImportError::Io {
        path: path_owned.clone(),
        source: e,
    })?;

    // Read first 4 bytes to determine buffer size (first field of WMI_BUFFER_HEADER).
    let mut size_buf = [0u8; 4];
    file.read_exact(&mut size_buf)
        .await
        .map_err(|e| ImportError::InvalidFile {
            path: path_owned.clone(),
            format: crate::TraceFormat::Etl,
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
        .map_err(|e| ImportError::Io {
            path: path_owned.clone(),
            source: e,
        })?;

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
                let _ = tx
                    .send(Err(ImportError::PartialCorruption {
                        path,
                        boundary_offset: file_offset,
                        reason: e.to_string(),
                    }))
                    .await;
                return;
            }
        }

        // `SavedOffset` (bytes 4-7 of the buffer header) is the end of valid
        // event data within this buffer; 0 means the entire buffer is used.
        let saved_offset = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
        let event_end = if saved_offset == 0 || saved_offset > buffer_size {
            buffer_size
        } else {
            saved_offset
        };

        let events_slice = &buf[ETL_BUFFER_HEADER_SIZE..event_end];
        let abort = parse_buffer_events(events_slice, file_offset, &path, &tx).await;
        file_offset += buffer_size as u64;
        if abort {
            return;
        }
    }
}

/// Parse all event records within a single ETL buffer's event region.
/// Returns `true` if the caller should stop (receiver dropped or fatal error).
async fn parse_buffer_events(
    data: &[u8],
    _buffer_file_offset: u64,
    _path: &Path,
    tx: &mpsc::Sender<Result<RawEvent, ImportError>>,
) -> bool {
    let mut off = 0usize;

    while off + 2 <= data.len() {
        // The first field of every event record is Size (u16).
        let ev_size = u16::from_le_bytes(data[off..off + 2].try_into().unwrap()) as usize;

        if ev_size == 0 {
            // Zero-size record marks the end of events in this buffer.
            break;
        }
        if ev_size < ETL_EVENT_HEADER_SIZE || off + ev_size > data.len() {
            // Corrupt or padded tail; stop this buffer.
            break;
        }

        let record = &data[off..off + ev_size];
        if let Some(event) = decode_etl_record(record) {
            if tx.send(Ok(event)).await.is_err() {
                return true;
            }
        }

        off += ev_size;
        // ETL records are aligned to 8-byte boundaries.
        let aligned = (off + 7) & !7;
        if aligned > data.len() {
            break;
        }
        off = aligned;
    }
    false
}

fn decode_etl_record(record: &[u8]) -> Option<RawEvent> {
    if record.len() < ETL_EVENT_HEADER_SIZE {
        return None;
    }

    let tid = u32::from_le_bytes(record[ETL_THREAD_ID_OFFSET..ETL_THREAD_ID_OFFSET + 4].try_into().ok()?);
    let pid = u32::from_le_bytes(record[ETL_PROCESS_ID_OFFSET..ETL_PROCESS_ID_OFFSET + 4].try_into().ok()?);
    let filetime = u64::from_le_bytes(record[ETL_TIMESTAMP_OFFSET..ETL_TIMESTAMP_OFFSET + 8].try_into().ok()?);
    let timestamp_ns = filetime_to_unix_ns(filetime);

    let mut provider = [0u8; 16];
    provider.copy_from_slice(&record[ETL_PROVIDER_ID_OFFSET..ETL_PROVIDER_ID_OFFSET + 16]);

    let event_id = u16::from_le_bytes(record[ETL_EVENT_ID_OFFSET..ETL_EVENT_ID_OFFSET + 2].try_into().ok()?);
    let version = record[ETL_EVENT_VERSION_OFFSET];
    let level = record[ETL_EVENT_LEVEL_OFFSET];
    let opcode = record[ETL_EVENT_OPCODE_OFFSET];

    let payload_start = ETL_EVENT_HEADER_SIZE;
    let payload = if record.len() > payload_start {
        Bytes::copy_from_slice(&record[payload_start..])
    } else {
        Bytes::new()
    };

    Some(RawEvent {
        timestamp_ns,
        pid,
        tid,
        cpu: None,
        kind: RawEventKind::EtwEvent {
            provider,
            opcode,
            version,
            event_id,
            level,
        },
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_converts_to_unix_ns() {
        // Windows FILETIME for 2000-01-01T00:00:00Z = 125911584000000000
        let filetime: u64 = 125_911_584_000_000_000;
        let unix_ns = filetime_to_unix_ns(filetime);
        // 2000-01-01 00:00:00 UTC in nanoseconds since 1970 = 946684800 * 1e9
        let expected = 946_684_800_000_000_000u64;
        // Allow ±1ns for rounding
        assert!((unix_ns as i64 - expected as i64).abs() <= 1,
            "got {unix_ns}, expected {expected}");
    }

    #[test]
    fn filetime_before_epoch_saturates_to_zero() {
        // Before 1970-01-01
        let filetime: u64 = FILETIME_TO_UNIX_OFFSET_100NS - 1000;
        assert_eq!(filetime_to_unix_ns(filetime), 0);
    }

    #[test]
    fn invalid_buffer_size_detected() {
        // A buffer that starts with 0x0000_0001 is not a valid ETL size
        assert!(!VALID_BUFFER_SIZES.contains(&1));
        assert!(VALID_BUFFER_SIZES.contains(&4096));
    }

    fn make_etl_record(pid: u32, tid: u32, filetime: u64, opcode: u8) -> Vec<u8> {
        let mut r = vec![0u8; ETL_EVENT_HEADER_SIZE];
        let size = ETL_EVENT_HEADER_SIZE as u16;
        r[0..2].copy_from_slice(&size.to_le_bytes());
        r[ETL_THREAD_ID_OFFSET..ETL_THREAD_ID_OFFSET + 4].copy_from_slice(&tid.to_le_bytes());
        r[ETL_PROCESS_ID_OFFSET..ETL_PROCESS_ID_OFFSET + 4].copy_from_slice(&pid.to_le_bytes());
        r[ETL_TIMESTAMP_OFFSET..ETL_TIMESTAMP_OFFSET + 8].copy_from_slice(&filetime.to_le_bytes());
        r[ETL_EVENT_OPCODE_OFFSET] = opcode;
        r
    }

    #[test]
    fn decode_etl_record_extracts_fields() {
        let filetime: u64 = 125_911_584_000_000_000; // 2000-01-01
        let record = make_etl_record(999, 1000, filetime, 42);
        let ev = decode_etl_record(&record).unwrap();
        assert_eq!(ev.pid, 999);
        assert_eq!(ev.tid, 1000);
        assert_eq!(ev.timestamp_ns, filetime_to_unix_ns(filetime));
        assert!(matches!(ev.kind, RawEventKind::EtwEvent { opcode: 42, .. }));
    }

    #[test]
    fn too_short_record_returns_none() {
        let short = vec![0u8; ETL_EVENT_HEADER_SIZE - 1];
        assert!(decode_etl_record(&short).is_none());
    }
}
