//! Cross-cutting unit tests that compile on any platform.
//!
//! Platform-specific integration tests live in `tests/` at the crate root.

use crate::types::RecordConfig;

#[test]
fn default_config_reasonable() {
    let cfg = RecordConfig::default();
    assert_eq!(cfg.sample_rate_hz, 100);
    assert!(cfg.enable_dtrace);
    assert!(cfg.enable_memory);
    assert!(cfg.dtrace_script.is_none());
    assert!(cfg.duration_secs.is_none());
}

#[cfg(target_os = "macos")]
mod macos {
    use crate::ffi::{CPU_TYPE_ARM64, CPU_TYPE_X86_64};
    use crate::sampler::Arch;

    #[test]
    fn cputype_arm64_detected() {
        assert_eq!(Arch::from_cputype(CPU_TYPE_ARM64), Arch::Arm64);
    }

    #[test]
    fn cputype_x86_64_detected() {
        assert_eq!(Arch::from_cputype(CPU_TYPE_X86_64), Arch::X86_64);
    }

    #[test]
    fn unknown_cputype_falls_back_to_host() {
        let _ = Arch::from_cputype(0); // must not panic
    }
}
