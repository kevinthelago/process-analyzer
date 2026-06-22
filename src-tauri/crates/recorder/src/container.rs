/// Finalization of the `.patrace` container after a recording session ends.
///
/// Finalization is triggered by any of: target-process exit, disk-full,
/// user-initiated stop, or max-duration/max-size limits. This module delegates
/// the actual write to `TraceStore::save`, which writes the directory-format
/// container that trace-core owns.
use std::path::PathBuf;

use trace_core::TraceStore;

use crate::error::RecorderError;

/// Reason the capture was terminated; recorded in session telemetry / logs.
#[derive(Debug, Clone)]
pub enum FinalizeReason {
    UserStop,
    TargetProcessExited,
    DiskFull,
    MaxDurationReached,
    MaxBytesReached,
    BackendError(String),
}

impl FinalizeReason {
    pub fn is_clean(&self) -> bool {
        matches!(self, FinalizeReason::UserStop | FinalizeReason::TargetProcessExited)
    }

    pub fn description(&self) -> Option<&str> {
        match self {
            FinalizeReason::UserStop | FinalizeReason::TargetProcessExited => None,
            FinalizeReason::DiskFull => Some("output disk full"),
            FinalizeReason::MaxDurationReached => Some("max duration reached"),
            FinalizeReason::MaxBytesReached => Some("max byte limit reached"),
            FinalizeReason::BackendError(_) => Some("backend error"),
        }
    }
}

/// Parameters for finalizing a recording container.
pub struct FinalizeParams {
    pub output_dir: PathBuf,
    pub label: Option<String>,
    /// The completed store produced by `StandardRecorder::finish()`.
    /// The store's manifest must already have `os`, `hostname`, `arch`, and
    /// `duration_ns` populated before passing here.
    pub store: TraceStore,
    pub reason: FinalizeReason,
    pub event_count: u64,
}

/// Write the `.patrace` directory container and return its path.
pub async fn finalize_container(params: FinalizeParams) -> Result<PathBuf, RecorderError> {
    if !params.reason.is_clean() {
        tracing::warn!(
            reason = ?params.reason.description(),
            "recording terminated abnormally"
        );
    }

    let created_at_ns = params.store.manifest.created_at_ns as u64;
    let filename = make_filename(params.label.as_deref(), created_at_ns);
    let out_path = params.output_dir.join(&filename);

    let store = params.store;
    let out_path_for_save = out_path.clone();

    tokio::task::spawn_blocking(move || {
        store
            .save(&out_path_for_save)
            .map_err(|e| RecorderError::ContainerError(e.to_string()))
    })
    .await
    .map_err(|e| RecorderError::ContainerError(e.to_string()))??;

    tracing::info!(
        path = %out_path.display(),
        event_count = params.event_count,
        "container finalized"
    );

    Ok(out_path)
}

fn make_filename(label: Option<&str>, start_time_ns: u64) -> String {
    let ts = start_time_ns / 1_000_000_000;
    match label {
        Some(l) => {
            let safe: String = l
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
                .collect();
            format!("{safe}_{ts}.patrace")
        }
        None => format!("capture_{ts}.patrace"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_sanitizes_label() {
        let name = make_filename(Some("my app/session:1"), 1_700_000_000_000_000_000);
        assert!(name.starts_with("my_app_session_1_"));
        assert!(name.ends_with(".patrace"));
    }

    #[test]
    fn filename_uses_default_when_no_label() {
        let name = make_filename(None, 1_700_000_000_000_000_000);
        assert!(name.starts_with("capture_"));
        assert!(name.ends_with(".patrace"));
    }

    #[test]
    fn finalize_reason_clean_check() {
        assert!(FinalizeReason::UserStop.is_clean());
        assert!(FinalizeReason::TargetProcessExited.is_clean());
        assert!(!FinalizeReason::DiskFull.is_clean());
        assert!(!FinalizeReason::MaxDurationReached.is_clean());
    }

    #[tokio::test]
    async fn finalize_creates_directory_container() {
        use trace_core::{Manifest, TraceStore};

        let dir = tempfile::tempdir().unwrap();
        let manifest = Manifest::new("test-trace-1", 1_000_000_000_i64);
        let store = TraceStore::new(manifest);

        let path = finalize_container(FinalizeParams {
            output_dir: dir.path().to_path_buf(),
            label: Some("test-session".into()),
            store,
            reason: FinalizeReason::UserStop,
            event_count: 0,
        })
        .await
        .unwrap();

        assert!(path.is_dir(), "output should be a directory");
        assert!(path.join("manifest.json").exists(), "manifest.json must exist");

        let manifest_bytes = std::fs::read(path.join("manifest.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
        assert_eq!(manifest["schema_version"], 1);
        assert_eq!(manifest["trace_id"], "test-trace-1");
    }
}
