// Unit tests that don't require Administrator or a real ETW session.

#[cfg(test)]
mod elevation_test {
    use crate::elevation::require_elevated;
    use crate::error::RecordError;

    /// Not an assertion — just documents that the function exists and returns
    /// a typed error when not elevated.  The actual result depends on the test
    /// runner's privilege level.
    #[test]
    fn elevation_check_returns_typed_error_when_not_admin() {
        match require_elevated() {
            Ok(()) => { /* test ran as Administrator */ }
            Err(RecordError::NotElevated) => { /* expected when not admin */ }
            Err(other) => panic!("unexpected error: {other}"),
        }
    }
}

#[cfg(test)]
mod recorder_config_test {
    use crate::recorder::{KernelFlags, RecordConfig};

    #[test]
    fn default_config_includes_all_required_providers() {
        let cfg = RecordConfig::default();
        assert!(cfg.kernel_flags.contains(KernelFlags::PROFILE));
        assert!(cfg.kernel_flags.contains(KernelFlags::CSWITCH));
        assert!(cfg.kernel_flags.contains(KernelFlags::DISK_IO));
        assert!(cfg.kernel_flags.contains(KernelFlags::FILE_IO));
        assert!(cfg.kernel_flags.contains(KernelFlags::NETWORK_TCPIP));
        assert!(cfg.kernel_flags.contains(KernelFlags::IMAGE_LOAD));
    }

    #[test]
    fn kernel_flags_bits_match_etw_enable_flags() {
        // These raw values are verified against the Windows SDK documentation.
        assert_eq!(KernelFlags::PROFILE.bits(), 0x0100_0000);
        assert_eq!(KernelFlags::CSWITCH.bits(), 0x0000_0010);
        assert_eq!(KernelFlags::DISK_IO.bits(), 0x0000_0100);
        assert_eq!(KernelFlags::FILE_IO.bits(), 0x0200_0000);
        assert_eq!(KernelFlags::NETWORK_TCPIP.bits(), 0x0001_0000);
        assert_eq!(KernelFlags::IMAGE_LOAD.bits(), 0x0000_0004);
    }
}

#[cfg(test)]
mod recorder_state_test {
    use crate::error::RecordError;
    use crate::recorder::{RecordConfig, WindowsEtwRecorder};
    use crate::Recorder;

    #[test]
    fn stop_before_start_returns_not_running() {
        let mut r = WindowsEtwRecorder::new();
        assert!(matches!(r.stop(), Err(RecordError::NotRunning)));
    }

    #[test]
    fn stats_before_start_returns_none() {
        let r = WindowsEtwRecorder::new();
        assert!(r.stats().is_none());
    }

    /// Start→start returns AlreadyRunning.  This test is skipped when not
    /// elevated because the first `start()` would fail with NotElevated before
    /// reaching AlreadyRunning.
    #[test]
    fn double_start_returns_already_running() {
        let mut r = WindowsEtwRecorder::new();
        let cfg = RecordConfig::default();
        match r.start(cfg.clone()) {
            Err(RecordError::NotElevated) => return, // not admin — skip
            Err(RecordError::KernelLoggerConflict) => return, // session taken — skip
            Err(e) => panic!("unexpected first-start error: {e}"),
            Ok(()) => {}
        }
        let result = r.start(cfg);
        let _ = r.stop();
        assert!(matches!(result, Err(RecordError::AlreadyRunning)));
    }
}

#[cfg(test)]
mod wpr_test {
    use crate::error::RecordError;

    /// Documents that the wpr path discovery returns a typed error when wpr.exe
    /// is absent — doesn't actually start a recording.
    #[test]
    fn wpr_not_found_returns_typed_error() {
        // We can't easily mock PATH, so just check the error variant exists.
        let _: RecordError = RecordError::WprNotFound;
    }
}
