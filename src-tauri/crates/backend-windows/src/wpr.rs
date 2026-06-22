// wpr.exe fallback capture.
//
// Invokes `wpr.exe` from the Windows Performance Toolkit to start/stop a
// kernel capture in file mode.  Used when the NT Kernel Logger is already
// occupied by another tool.
//
// wpr.exe must be on PATH or in the standard SDK locations.

use std::path::PathBuf;
use std::process::Command;

use crate::error::RecordError;

// Search paths for wpr.exe when it's not on PATH.
const WPR_SEARCH_PATHS: &[&str] = &[
    r"C:\Program Files (x86)\Windows Kits\10\Windows Performance Toolkit\wpr.exe",
    r"C:\Program Files\Windows Kits\10\Windows Performance Toolkit\wpr.exe",
];

/// Active wpr.exe recording session.
pub struct WprSession {
    etl_path: PathBuf,
    wpr_path: PathBuf,
}

impl WprSession {
    /// Start a general-profile kernel capture with wpr.exe in file mode.
    pub fn start() -> Result<Self, RecordError> {
        let wpr_path = find_wpr()?;

        // Generate a unique output path in the system temp directory.
        let etl_path = std::env::temp_dir()
            .join(format!("process-analyzer-{}.etl", std::process::id()));

        let status = Command::new(&wpr_path)
            .args([
                "-start",
                "GeneralProfile",
                "-filemode",
                "-instancename",
                "process-analyzer",
            ])
            .status()
            .map_err(|e| RecordError::WprFailed(format!("could not launch wpr.exe: {e}")))?;

        if !status.success() {
            return Err(RecordError::WprFailed(format!(
                "wpr.exe -start exited with code {}",
                status.code().unwrap_or(-1)
            )));
        }

        tracing::info!(path = ?etl_path, "wpr.exe file-mode capture started");
        Ok(WprSession { etl_path, wpr_path })
    }

    /// Stop the recording and return the path to the produced ETL file.
    pub fn stop(self) -> Result<PathBuf, RecordError> {
        let output = Command::new(&self.wpr_path)
            .args([
                "-stop",
                self.etl_path.to_str().unwrap_or_default(),
                "-instancename",
                "process-analyzer",
            ])
            .output()
            .map_err(|e| RecordError::WprFailed(format!("could not launch wpr.exe: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RecordError::WprFailed(format!(
                "wpr.exe -stop exited with code {}: {stderr}",
                output.status.code().unwrap_or(-1)
            )));
        }

        if !self.etl_path.exists() {
            return Err(RecordError::WprFailed(format!(
                "wpr.exe reported success but ETL file not found: {}",
                self.etl_path.display()
            )));
        }

        Ok(self.etl_path)
    }
}

fn find_wpr() -> Result<PathBuf, RecordError> {
    // Try PATH first.
    if let Ok(output) = Command::new("where").arg("wpr.exe").output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            let first = s.lines().next().unwrap_or("").trim().to_owned();
            if !first.is_empty() {
                return Ok(PathBuf::from(first));
            }
        }
    }

    for path in WPR_SEARCH_PATHS {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    Err(RecordError::WprNotFound)
}
