/// OS-specific privilege preflight checks.
///
/// Each check returns `Ok(())` if the current process has sufficient privileges,
/// or a `RecorderError` with an actionable fix message if not.
use crate::error::RecorderError;

/// Run the preflight check appropriate for the current OS.
pub fn check_privileges() -> Result<(), RecorderError> {
    _check_privileges_impl()
}

// ── Linux ─────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn _check_privileges_impl() -> Result<(), RecorderError> {
    // Acceptable if running as root.
    if is_root() {
        return Ok(());
    }
    // Otherwise require CAP_PERFMON (Linux 5.8+) or fall back to checking
    // CAP_SYS_ADMIN (pre-5.8 kernels used this for perf_event_open).
    if has_cap_perfmon() || has_cap_sys_admin() {
        return Ok(());
    }
    // Check perf_event_paranoid as a last resort — if it's ≤1, unprivileged
    // users can record their own processes.
    if perf_event_paranoid() <= 1 {
        return Ok(());
    }
    Err(RecorderError::InsufficientPrivilegesLinux)
}

// CAP_PERFMON (38) was added to Linux 5.8; CAP_SYS_ADMIN (21) is the classic fallback.
// We define them as constants rather than using libc::CAP_* to avoid depending on
// a specific libc version that has CAP_PERFMON.
#[cfg(target_os = "linux")]
const CAP_SYS_ADMIN: i32 = 21;
#[cfg(target_os = "linux")]
const CAP_PERFMON: i32 = 38;
#[cfg(target_os = "linux")]
const CAP_BPF: i32 = 39;

#[cfg(target_os = "linux")]
fn is_root() -> bool {
    // SAFETY: getuid() is always safe.
    unsafe { libc::getuid() == 0 }
}

#[cfg(target_os = "linux")]
fn has_cap_perfmon() -> bool {
    check_capability(CAP_PERFMON) || check_capability(CAP_BPF)
}

#[cfg(target_os = "linux")]
fn has_cap_sys_admin() -> bool {
    check_capability(CAP_SYS_ADMIN)
}

#[cfg(target_os = "linux")]
fn check_capability(cap: i32) -> bool {
    // Use capget(2) via the raw syscall to query the effective capability set.
    // _LINUX_CAPABILITY_VERSION_3 = 0x20080522; returns two 32-bit words.
    #[repr(C)]
    struct CapHdr { version: u32, pid: i32 }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct CapData { effective: u32, permitted: u32, inheritable: u32 }

    let hdr = CapHdr { version: 0x2008_0522, pid: 0 };
    let mut data = [CapData::default(); 2];
    // SAFETY: pointers are valid stack allocations; SYS_capget is a safe syscall.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_capget,
            &hdr as *const CapHdr,
            data.as_mut_ptr() as *mut CapData,
        )
    };
    if ret != 0 {
        return false;
    }
    let (word, bit) = (cap as usize / 32, cap as usize % 32);
    if word >= data.len() { return false; }
    (data[word].effective >> bit) & 1 == 1
}

#[cfg(target_os = "linux")]
fn perf_event_paranoid() -> i32 {
    std::fs::read_to_string("/proc/sys/kernel/perf_event_paranoid")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(3) // assume most restrictive if unreadable
}

// ── macOS ─────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn _check_privileges_impl() -> Result<(), RecorderError> {
    // DTrace and `xctrace` both require root on a default SIP-enabled system.
    if is_root_macos() {
        return Ok(());
    }
    Err(RecorderError::InsufficientPrivilegesMacos)
}

#[cfg(target_os = "macos")]
fn is_root_macos() -> bool {
    // SAFETY: getuid() is always safe.
    unsafe { libc::getuid() == 0 }
}

// ── Windows ───────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn _check_privileges_impl() -> Result<(), RecorderError> {
    if is_elevated_windows() {
        return Ok(());
    }
    Err(RecorderError::InsufficientPrivilegesWindows)
}

#[cfg(target_os = "windows")]
fn is_elevated_windows() -> bool {
    // IsUserAnAdmin() / CheckTokenMembership — call via winapi.
    // We do a raw check: open the current process token, then check for
    // TOKEN_ELEVATION via GetTokenInformation.
    //
    // For now we implement this using documented Win32 path via std + raw FFI.
    // A full winapi dep would be added when windows-backend lands; for
    // preflight only we inline the minimal check.
    is_elevated_impl()
}

#[cfg(target_os = "windows")]
fn is_elevated_impl() -> bool {
    use std::mem;

    extern "system" {
        fn OpenProcessToken(
            process_handle: *mut std::ffi::c_void,
            desired_access: u32,
            token_handle: *mut *mut std::ffi::c_void,
        ) -> i32;
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetTokenInformation(
            token_handle: *mut std::ffi::c_void,
            token_information_class: u32,
            token_information: *mut std::ffi::c_void,
            token_information_length: u32,
            return_length: *mut u32,
        ) -> i32;
        fn CloseHandle(h: *mut std::ffi::c_void) -> i32;
    }

    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_ELEVATION: u32 = 20; // TokenElevation class

    #[repr(C)]
    struct TokenElevation { token_is_elevated: u32 }

    let mut token: *mut std::ffi::c_void = std::ptr::null_mut();
    // SAFETY: Win32 FFI calls with valid pointers.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TokenElevation { token_is_elevated: 0 };
        let mut ret_len: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TOKEN_ELEVATION,
            &mut elevation as *mut _ as *mut std::ffi::c_void,
            mem::size_of::<TokenElevation>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        ok != 0 && elevation.token_is_elevated != 0
    }
}

// ── Fallback for other targets (e.g. test builds) ────────────────────────────

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn _check_privileges_impl() -> Result<(), RecorderError> {
    // Unknown platform — assume we have sufficient privileges and let the
    // backend report failures if it encounters permission errors.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_runs_without_panic() {
        // We don't assert the result — it depends on the environment.
        // We just verify the call doesn't panic or crash.
        let _ = check_privileges();
    }
}
