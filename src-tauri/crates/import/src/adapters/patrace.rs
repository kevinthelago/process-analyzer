/// Adapter for the Process Analyzer native container format (.patrace).
///
/// ## File layout
///
/// ```text
/// [0..8]   magic: b"PATRACE\0"
/// [8..12]  version: u32 LE  (currently 1)
/// [12..16] flags: u32 LE    (reserved, must be 0)
/// [16..24] manifest_offset: u64 LE
/// [24..32] manifest_length: u64 LE  (bytes of manifest JSON)
/// [32..40] data_offset: u64 LE
/// [40..48] data_length: u64 LE      (bytes of Arrow IPC stream)
/// [manifest_offset .. +manifest_length]  UTF-8 JSON manifest
/// [data_offset     .. +data_length    ]  Arrow IPC stream (schema + record batches)
/// ```
///
/// The Arrow IPC stream is decoded by the `pa-trace-core` normalizer; this
/// adapter reads it as opaque bytes and emits one `RawEvent::Raw` per IPC
/// message boundary. When trace-core lands, replace this with proper Arrow
/// decoding using the schema from the manifest.
///
/// Corrupt files: if `data_length > available bytes`, partial data is imported.
use std::path::Path;

use bytes::Bytes;
use serde::Deserialize;
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::contract::{RawEvent, RawEventKind};
use crate::detect::PATRACE_MAGIC;
use crate::error::ImportError;
use crate::ImportResult;

pub const PATRACE_VERSION: u32 = 1;

// Fixed header size: 8 (magic) + 4 (version) + 4 (flags) + 6×8 (offsets/lengths) = 64 bytes
const PATRACE_HEADER_SIZE: usize = 8 + 4 + 4 + 8 + 8 + 8 + 8;

// Arrow IPC messages start with a 4-byte continuation marker (0xFF_FF_FF_FF)
// followed by a 4-byte metadata length. We use this to find record-batch boundaries.
const IPC_CONTINUATION_MARKER: u32 = 0xFFFF_FFFF;
const IPC_EOS_MARKER: u32 = 0; // end-of-stream: metadata_length == 0

/// Metadata embedded in the container's manifest section.
#[derive(Debug, Deserialize)]
pub struct PatraceManifest {
    pub schema_version: u32,
    pub os: Option<String>,
    pub start_time_ns: u64,
    pub end_time_ns: Option<u64>,
    pub hostname: Option<String>,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub event_count: Option<u64>,
    /// `true` if capture was interrupted before a clean shutdown.
    pub incomplete: bool,
    pub incomplete_reason: Option<String>,
}

pub async fn import(path: &Path) -> Result<ImportResult, ImportError> {
    let path_owned = path.to_path_buf();
    let mut file = tokio::fs::File::open(path).await.map_err(|e| ImportError::Io {
        path: path_owned.clone(),
        source: e,
    })?;

    // Read and validate the fixed header.
    let header = parse_patrace_header(&mut file, &path_owned).await?;

    // Read and parse the manifest (best-effort; non-fatal if corrupt).
    let manifest = read_manifest(&mut file, &header, &path_owned).await;

    // The data section contains an Arrow IPC stream. We emit one RawEvent per
    // IPC record-batch boundary. Until trace-core defines the schema, each batch
    // is emitted as Raw with its bytes in the payload.
    file.seek(std::io::SeekFrom::Start(header.data_offset))
        .await
        .map_err(|e| ImportError::Io {
            path: path_owned.clone(),
            source: e,
        })?;

    let (tx, rx) = mpsc::channel::<Result<RawEvent, ImportError>>(128);
    let start_time = manifest.as_ref().map(|m| m.start_time_ns).unwrap_or(0);

    tokio::spawn(async move {
        stream_ipc_batches(file, path_owned, header.data_offset, header.data_length, start_time, tx).await;
    });

    let is_complete = manifest.as_ref().map(|m| !m.incomplete).unwrap_or(true);

    Ok(ImportResult {
        events: Box::pin(ReceiverStream::new(rx)),
        is_complete,
        boundary_offset: None,
    })
}

struct PatraceHeader {
    manifest_offset: u64,
    manifest_length: u64,
    data_offset: u64,
    data_length: u64,
}

async fn parse_patrace_header(
    file: &mut tokio::fs::File,
    path: &Path,
) -> Result<PatraceHeader, ImportError> {
    let mut buf = [0u8; PATRACE_HEADER_SIZE];
    file.read_exact(&mut buf).await.map_err(|e| ImportError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    let magic = &buf[0..8];
    if magic != PATRACE_MAGIC {
        return Err(ImportError::InvalidFile {
            path: path.to_path_buf(),
            format: crate::TraceFormat::PatraceContainer,
            reason: format!("invalid magic {:?}", std::str::from_utf8(magic).unwrap_or("<binary>")),
        });
    }

    let version = u32::from_le_bytes(buf[8..12].try_into().unwrap());
    if version != PATRACE_VERSION {
        return Err(ImportError::InvalidFile {
            path: path.to_path_buf(),
            format: crate::TraceFormat::PatraceContainer,
            reason: format!("unsupported container version {version}; this build supports version {PATRACE_VERSION}"),
        });
    }

    // flags at [12..16] — reserved, skip
    let manifest_offset = u64::from_le_bytes(buf[16..24].try_into().unwrap());
    let manifest_length = u64::from_le_bytes(buf[24..32].try_into().unwrap());
    let data_offset = u64::from_le_bytes(buf[32..40].try_into().unwrap());
    let data_length = u64::from_le_bytes(buf[40..48].try_into().unwrap());

    Ok(PatraceHeader {
        manifest_offset,
        manifest_length,
        data_offset,
        data_length,
    })
}

async fn read_manifest(
    file: &mut tokio::fs::File,
    header: &PatraceHeader,
    path: &Path,
) -> Option<PatraceManifest> {
    if header.manifest_length == 0 || header.manifest_length > 64 * 1024 {
        return None;
    }
    file.seek(std::io::SeekFrom::Start(header.manifest_offset)).await.ok()?;
    let mut json_bytes = vec![0u8; header.manifest_length as usize];
    file.read_exact(&mut json_bytes).await.ok()?;
    serde_json::from_slice(&json_bytes).ok()
}

/// Stream Arrow IPC batches from the data section as raw RawEvent::Raw items.
///
/// Arrow IPC format: each message starts with a 4-byte continuation marker
/// (0xFFFF_FFFF), followed by a 4-byte little-endian metadata size, followed
/// by `metadata_size` bytes of flatbuffer metadata, followed by optional body
/// buffers. A metadata_size of 0 signals end-of-stream.
async fn stream_ipc_batches(
    mut file: tokio::fs::File,
    path: std::path::PathBuf,
    data_offset: u64,
    data_length: u64,
    start_time_ns: u64,
    tx: mpsc::Sender<Result<RawEvent, ImportError>>,
) {
    let mut bytes_read: u64 = 0;
    let mut batch_index: u32 = 0;

    while bytes_read + 8 <= data_length {
        let mut prefix = [0u8; 8];
        match file.read_exact(&mut prefix).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                let _ = tx
                    .send(Err(ImportError::PartialCorruption {
                        path,
                        boundary_offset: data_offset + bytes_read,
                        reason: e.to_string(),
                    }))
                    .await;
                return;
            }
        }

        let continuation = u32::from_le_bytes(prefix[0..4].try_into().unwrap());
        let metadata_len = u32::from_le_bytes(prefix[4..8].try_into().unwrap());

        if continuation != IPC_CONTINUATION_MARKER {
            // Treat as corruption at this boundary.
            let _ = tx
                .send(Err(ImportError::PartialCorruption {
                    path,
                    boundary_offset: data_offset + bytes_read,
                    reason: format!(
                        "expected Arrow IPC continuation marker 0xFFFFFFFF, got {continuation:#010x}"
                    ),
                }))
                .await;
            return;
        }
        if metadata_len == IPC_EOS_MARKER {
            break; // clean end-of-stream
        }

        bytes_read += 8;

        // Read metadata flatbuffer + any body data described by it.
        // We don't decode the flatbuffer here — emit the whole chunk as a Raw event.
        let msg_bytes = (metadata_len as u64).min(data_length.saturating_sub(bytes_read));
        let mut meta_buf = vec![0u8; msg_bytes as usize];
        match file.read_exact(&mut meta_buf).await {
            Ok(_) => {}
            Err(e) => {
                let _ = tx
                    .send(Err(ImportError::PartialCorruption {
                        path,
                        boundary_offset: data_offset + bytes_read,
                        reason: e.to_string(),
                    }))
                    .await;
                return;
            }
        }
        bytes_read += msg_bytes;

        // Emit one Raw event per IPC batch as a placeholder until trace-core
        // decodes the Arrow schema and emits proper typed events.
        let event = RawEvent {
            timestamp_ns: start_time_ns,
            pid: 0,
            tid: 0,
            cpu: None,
            kind: RawEventKind::Raw {
                provider_tag: 0x5041_5441, // "PATA" in ASCII — patrace sentinel tag
                opcode: batch_index,
            },
            payload: Bytes::from(meta_buf),
        };
        if tx.send(Ok(event)).await.is_err() {
            return;
        }
        batch_index += 1;
    }
}

/// Build a valid `.patrace` container from a manifest and Arrow IPC bytes.
/// Used by the recorder's container finalization step.
pub fn build_patrace(manifest: &PatraceManifest, arrow_ipc_bytes: &[u8]) -> Vec<u8> {
    let manifest_bytes = serde_json::to_vec(manifest)
        .unwrap_or_else(|_| b"{}".to_vec());

    let manifest_offset = PATRACE_HEADER_SIZE as u64;
    let manifest_length = manifest_bytes.len() as u64;
    let data_offset = manifest_offset + manifest_length;
    let data_length = arrow_ipc_bytes.len() as u64;

    let mut out = Vec::with_capacity(PATRACE_HEADER_SIZE + manifest_bytes.len() + arrow_ipc_bytes.len());

    // Magic + version + flags
    out.extend_from_slice(PATRACE_MAGIC);
    out.extend_from_slice(&PATRACE_VERSION.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // flags

    // Offsets
    out.extend_from_slice(&manifest_offset.to_le_bytes());
    out.extend_from_slice(&manifest_length.to_le_bytes());
    out.extend_from_slice(&data_offset.to_le_bytes());
    out.extend_from_slice(&data_length.to_le_bytes());

    // Payload sections
    out.extend_from_slice(&manifest_bytes);
    out.extend_from_slice(arrow_ipc_bytes);

    out
}

// PatraceManifest also needs to be serializable (for build_patrace).
impl serde::Serialize for PatraceManifest {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("PatraceManifest", 10)?;
        st.serialize_field("schema_version", &self.schema_version)?;
        st.serialize_field("os", &self.os)?;
        st.serialize_field("start_time_ns", &self.start_time_ns)?;
        st.serialize_field("end_time_ns", &self.end_time_ns)?;
        st.serialize_field("hostname", &self.hostname)?;
        st.serialize_field("pid", &self.pid)?;
        st.serialize_field("process_name", &self.process_name)?;
        st.serialize_field("event_count", &self.event_count)?;
        st.serialize_field("incomplete", &self.incomplete)?;
        st.serialize_field("incomplete_reason", &self.incomplete_reason)?;
        st.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manifest(incomplete: bool) -> PatraceManifest {
        PatraceManifest {
            schema_version: 1,
            os: Some("linux".into()),
            start_time_ns: 1_000_000_000,
            end_time_ns: Some(2_000_000_000),
            hostname: Some("testhost".into()),
            pid: Some(42),
            process_name: Some("myapp".into()),
            event_count: Some(5),
            incomplete,
            incomplete_reason: None,
        }
    }

    /// Build an Arrow IPC EOS stream (just the end-of-stream marker).
    fn eos_ipc() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&IPC_CONTINUATION_MARKER.to_le_bytes());
        v.extend_from_slice(&IPC_EOS_MARKER.to_le_bytes());
        v
    }

    #[test]
    fn roundtrip_empty_container() {
        let manifest = make_manifest(false);
        let ipc = eos_ipc();
        let bytes = build_patrace(&manifest, &ipc);

        assert_eq!(&bytes[0..8], PATRACE_MAGIC);
        let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        assert_eq!(version, PATRACE_VERSION);

        let manifest_offset = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;
        let manifest_length = u64::from_le_bytes(bytes[24..32].try_into().unwrap()) as usize;
        let manifest_bytes = &bytes[manifest_offset..manifest_offset + manifest_length];
        let decoded: PatraceManifest = serde_json::from_slice(manifest_bytes).unwrap();
        assert_eq!(decoded.pid, Some(42));
        assert!(!decoded.incomplete);
    }

    #[tokio::test]
    async fn imports_eos_only_container() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("test.patrace");
        let manifest = make_manifest(false);
        let ipc = eos_ipc();
        let bytes = build_patrace(&manifest, &ipc);
        tokio::fs::write(&p, &bytes).await.unwrap();

        let result = import(&p).await.unwrap();
        assert!(result.is_complete);
        use futures::StreamExt as _;
        let events: Vec<_> = result.events.collect().await;
        // EOS marker produces no events
        assert_eq!(events.len(), 0);
    }

    #[tokio::test]
    async fn imports_patrace_with_one_ipc_batch() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("test.patrace");
        let manifest = make_manifest(false);

        // One IPC batch: 8-byte header + 4 bytes of fake metadata + EOS
        let mut ipc = Vec::new();
        ipc.extend_from_slice(&IPC_CONTINUATION_MARKER.to_le_bytes());
        ipc.extend_from_slice(&4u32.to_le_bytes()); // metadata_len = 4
        ipc.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        // EOS
        ipc.extend_from_slice(&IPC_CONTINUATION_MARKER.to_le_bytes());
        ipc.extend_from_slice(&IPC_EOS_MARKER.to_le_bytes());

        let bytes = build_patrace(&manifest, &ipc);
        tokio::fs::write(&p, &bytes).await.unwrap();

        let result = import(&p).await.unwrap();
        use futures::StreamExt as _;
        let events: Vec<_> = result.events.collect().await;
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].as_ref().unwrap().kind, RawEventKind::Raw { .. }));
    }

    #[tokio::test]
    async fn rejects_wrong_magic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.patrace");
        let mut bytes = vec![0u8; PATRACE_HEADER_SIZE + 16];
        bytes[0..8].copy_from_slice(b"WRONGMAG");
        tokio::fs::write(&p, &bytes).await.unwrap();
        assert!(matches!(import(&p).await, Err(ImportError::InvalidFile { .. })));
    }
}
