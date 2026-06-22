//! Integration tests — require a real macOS environment.
//!
//! These tests are skipped automatically on non-macOS builds via the
//! `#[cfg(target_os = "macos")]` attribute.
//!
//! Run with: `cargo test --test integration` on macOS.

#[cfg(target_os = "macos")]
mod macos_integration {
    use backend_macos::{MacosRecorder, RecordConfig, Recorder};
    use std::time::Duration;

    /// Smoke-test: record the current process for a short time.
    ///
    /// Requires either root privileges or the `com.apple.security.cs.debugger`
    /// entitlement. If neither is available the test falls through to the
    /// xctrace path (which requires Xcode CLT). Skips entirely if both paths
    /// are unavailable.
    #[test]
    fn record_self_smoke() {
        let pid = std::process::id();
        let config = RecordConfig {
            sample_rate_hz: 50,
            enable_dtrace: false, // avoid DTrace permission requirement in CI
            enable_memory: true,
            dtrace_script: None,
            duration_secs: Some(0.5),
        };

        let mut recorder = MacosRecorder::default();

        match recorder.start(pid, config) {
            Ok(()) => {}
            Err(e) => {
                // If we can't start (permission denied + no xctrace), skip.
                eprintln!("Skipping record_self_smoke: {e}");
                return;
            }
        }

        std::thread::sleep(Duration::from_millis(300));

        let data = recorder.stop().expect("stop should succeed");

        // We should have collected at least a few stack samples from our own thread.
        assert!(
            !data.stack_samples.is_empty(),
            "expected stack samples, got none"
        );

        // Every sample should reference our pid.
        for sample in &data.stack_samples {
            assert_eq!(sample.pid, pid);
            assert!(!sample.frames.is_empty(), "sample has empty frame list");
        }

        if !data.memory_snapshots.is_empty() {
            let snap = &data.memory_snapshots[0];
            assert_eq!(snap.pid, pid);
            assert!(snap.resident_bytes > 0, "resident_bytes should be > 0");
        }

        println!(
            "record_self_smoke: {} stack samples, {} memory snapshots, {} dyld images",
            data.stack_samples.len(),
            data.memory_snapshots.len(),
            data.dyld_images.len()
        );
    }

    /// Verify that recording twice without stopping returns AlreadyRecording.
    #[test]
    fn double_start_returns_error() {
        use backend_macos::RecordError;

        let pid = std::process::id();
        let config = RecordConfig::default();
        let mut recorder = MacosRecorder::default();

        match recorder.start(pid, config.clone()) {
            Ok(()) => {
                let err = recorder.start(pid, config).unwrap_err();
                assert!(
                    matches!(err, RecordError::AlreadyRecording(_)),
                    "expected AlreadyRecording, got: {err}"
                );
                let _ = recorder.stop();
            }
            Err(_) => {
                // Can't start (insufficient privilege); skip the double-start check.
            }
        }
    }

    /// Verify that stop without start returns NotRecording.
    #[test]
    fn stop_without_start() {
        use backend_macos::RecordError;
        let mut recorder = MacosRecorder::default();
        let err = recorder.stop().unwrap_err();
        assert!(matches!(err, RecordError::NotRecording));
    }
}
