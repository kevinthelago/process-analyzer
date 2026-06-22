/// Finalization of the `.patrace` container after a recording session ends.
///
/// Finalization is triggered by any of: target-process exit, disk-full,
/// user-initiated stop, or max-duration/max-size limits. This module handles
/// writing the Arrow IPC data that was buffered during capture into the
/// container format defined in `pa-import::adapters::patrace`.
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pa_import::adapters::patrace::{build_patrace, PatraceManifest};

use crate::error::RecorderError;

/// Reason the capture was terminated; embedded in the container manifest.
#[derive(Debug, Clone)]
pub enum FinalizeReason {
    /// User called `stop()` explicitly.
    UserStop,
    /// Target process exited.
    TargetProcessExited,
    /// Output directory ran out of space.
    DiskFull,
    /// Max capture duration reached.
    MaxDurationReached,
    /// Max disk usage limit reached.
    MaxBytesReached,
    /// Unrecoverable backend error.
    BackendError(String),
}

impl FinalizeReason {
    fn is_clean(&self) -> bool {
        matches!(self, FinalizeReason::UserStop | FinalizeReason::TargetProcessExited)
    }

    fn description(&self) -> Option<String> {
        match self {
            FinalizeReason::UserStop | FinalizeReason::TargetProcessExited => None,
            FinalizeReason::DiskFull => Some("capture stopped: output disk full".into()),
            FinalizeReason::MaxDurationReached => Some("capture stopped: max duration reached".into()),
            FinalizeReason::MaxBytesReached => Some("capture stopped: max byte limit reached".into()),
            FinalizeReason::BackendError(e) => Some(format!("capture stopped: backend error: {e}")),
        }
    }
}

/// Parameters for finalizing a recording container.
pub struct FinalizeParams<'a> {
    pub output_dir: &'a Path,
    pub label: Option<&'a str>,
    pub os: &'static str,
    pub pid: Option<u32>,
    pub process_name: Option<&'a str>,
    pub start_time_ns: u64,
    pub event_count: u64,
    /// The Arrow IPC stream bytes produced during capture.
    /// Empty vec is valid (produces a container with an EOS-only stream).
    pub arrow_ipc_bytes: Vec<u8>,
    pub reason: FinalizeReason,
}

/// Write the `.patrace` container file and return its path.
///
/// If `arrow_ipc_bytes` does not end with an Arrow IPC EOS marker, one is
/// appended automatically so the container is always self-consistent.
pub async fn finalize_container(params: FinalizeParams<'_>) -> Result<PathBuf, RecorderError> {
    let end_time_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    let manifest = PatraceManifest {
        schema_version: 1,
        os: Some(params.os.to_owned()),
        start_time_ns: params.start_time_ns,
        end_time_ns: Some(end_time_ns),
        hostname: hostname(),
        pid: params.pid,
        process_name: params.process_name.map(|s| s.to_owned()),
        event_count: Some(params.event_count),
        incomplete: !params.reason.is_clean(),
        incomplete_reason: params.reason.description(),
    };

    let mut ipc = params.arrow_ipc_bytes;
    ensure_ipc_eos(&mut ipc);

    let container_bytes = build_patrace(&manifest, &ipc);

    let filename = make_filename(params.label, params.start_time_ns);
    let out_path = params.output_dir.join(&filename);

    tokio::fs::write(&out_path, &container_bytes)
        .await
        .map_err(RecorderError::Io)?;

    tracing::info!(
        path = %out_path.display(),
        event_count = params.event_count,
        bytes = container_bytes.len(),
        "container finalized"
    );

    Ok(out_path)
}

/// Append an Arrow IPC end-of-stream marker if not already present.
fn ensure_ipc_eos(ipc: &mut Vec<u8>) {
    // EOS = continuation(0xFFFF_FFFF) + metadata_len(0x0000_0000)
    const EOS: [u8; 8] = [0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00];
    let n = ipc.len();
    if n < 8 || &ipc[n - 8..] != EOS {
        ipc.extend_from_slice(&EOS);
    }
}

fn make_filename(label: Option<&str>, start_time_ns: u64) -> String {
    let ts = start_time_ns / 1_000_000_000; // seconds since epoch
    match label {
        Some(l) => {
            // Sanitize label: replace non-alphanumeric characters with '_'.
            let safe: String = l
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
                .collect();
            format!("{safe}_{ts}.patrace")
        }
        None => format!("capture_{ts}.patrace"),
    }
}

fn hostname() -> Option<String> {
    // Read from /etc/hostname (Linux/macOS) or COMPUTERNAME env (Windows).
    #[cfg(target_os = "windows")]
    {
        std::env::var("COMPUTERNAME").ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::fs::read_to_string("/etc/hostname")
            .ok()
            .map(|s| s.trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_eos_appends_when_missing() {
        let mut ipc = vec![0xDE, 0xAD];
        ensure_ipc_eos(&mut ipc);
        assert_eq!(&ipc[2..], &[0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn ensure_eos_idempotent() {
        let mut ipc = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00];
        let before = ipc.clone();
        ensure_ipc_eos(&mut ipc);
        assert_eq!(ipc, before);
    }

    #[test]
    fn filename_sanitizes_label() {
        let name = make_filename(Some("my app/session:1"), 1_700_000_000);
        assert!(name.starts_with("my_app_session_1_"));
        assert!(name.ends_with(".patrace"));
    }

    #[test]
    fn filename_uses_default_when_no_label() {
        let name = make_filename(None, 1_700_000_000);
        assert!(name.starts_with("capture_"));
        assert!(name.ends_with(".patrace"));
    }

    #[tokio::test]
    async fn finalize_creates_valid_container() {
        let dir = tempfile::tempdir().unwrap();
        let path = finalize_container(FinalizeParams {
            output_dir: dir.path(),
            label: Some("test-session"),
            os: "linux",
            pid: Some(999),
            process_name: Some("myapp"),
            start_time_ns: 1_000_000_000,
            event_count: 42,
            arrow_ipc_bytes: vec![],
            reason: FinalizeReason::UserStop,
        })
        .await
        .unwrap();

        assert!(path.exists());
        let bytes = tokio::fs::read(&path).await.unwrap();
        assert_eq!(&bytes[0..8], b"PATRACE\0");

        // Parse manifest
        let manifest_offset = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;
        let manifest_length = u64::from_le_bytes(bytes[24..32].try_into().unwrap()) as usize;
        let manifest: PatraceManifest =
            serde_json::from_slice(&bytes[manifest_offset..manifest_offset + manifest_length]).unwrap();
        assert_eq!(manifest.pid, Some(999));
        assert!(!manifest.incomplete);
    }
}
