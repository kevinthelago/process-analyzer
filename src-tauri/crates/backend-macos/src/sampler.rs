//! Mach thread-suspend stack sampling for macOS.
//!
//! Each sample pass:
//!   1. `task_threads` → enumerate all threads in the target.
//!   2. For each thread: `thread_suspend` → `thread_get_state` → `thread_resume`.
//!   3. Walk the frame-pointer chain via `mach_vm_read_overwrite`.
//!
//! Frame pointer chain walking requires the target was compiled with
//! `-fno-omit-frame-pointer`, which is the default on Apple platforms.
//!
//! Apple Silicon note: saved return addresses on the stack may have PAC bits
//! set (pointer-authentication codes). We strip them with a 48-bit VA mask.
//! The initial PC from `thread_get_state` is already stripped by the kernel.

use std::mem;
use std::sync::mpsc::{Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tracing::{debug, warn};

use crate::error::RecordError;
use crate::ffi::{
    self, arm_thread_state64_t, mach_msg_type_number_t, task_t, thread_act_t,
    thread_act_array_t, thread_identifier_info_t, x86_thread_state64_t, ARM_THREAD_STATE64,
    ARM_THREAD_STATE64_COUNT, KERN_SUCCESS, MACH_PORT_NULL, THREAD_IDENTIFIER_INFO,
    THREAD_IDENTIFIER_INFO_COUNT, x86_THREAD_STATE64, x86_THREAD_STATE64_COUNT,
};
use crate::types::StackSample;

/// Maximum frames to collect per thread per sample.
const MAX_FRAMES: usize = 128;
/// Maximum threads to sample per pass (safety cap).
const MAX_THREADS: usize = 1024;

// ---------------------------------------------------------------------------
// Architecture detection
// ---------------------------------------------------------------------------

/// CPU architecture of the *target* process (may differ from the host on
/// Apple Silicon when the target runs under Rosetta 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arch {
    Arm64,
    X86_64,
}

impl Arch {
    /// Detect the target's architecture from the cputype in the first dyld image.
    ///
    /// Falls back to the host architecture on failure (safe on native; wrong for Rosetta).
    pub(crate) fn from_cputype(cputype: i32) -> Self {
        use crate::ffi::{CPU_TYPE_ARM64, CPU_TYPE_X86_64};
        if cputype == CPU_TYPE_ARM64 {
            Arch::Arm64
        } else if cputype == CPU_TYPE_X86_64 {
            Arch::X86_64
        } else {
            // Unknown — fall back to host arch.
            Self::host()
        }
    }

    pub(crate) fn host() -> Self {
        #[cfg(target_arch = "aarch64")]
        { Arch::Arm64 }
        #[cfg(target_arch = "x86_64")]
        { Arch::X86_64 }
        #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
        { Arch::X86_64 }
    }
}

// ---------------------------------------------------------------------------
// Task port acquisition / release
// ---------------------------------------------------------------------------

/// Acquire the Mach task port for `pid`.
///
/// Requires root or the `com.apple.security.cs.debugger` entitlement.
/// The returned port must be released via [`release_task_port`].
pub(crate) fn acquire_task_port(pid: u32) -> Result<task_t, RecordError> {
    let mut task_port: task_t = MACH_PORT_NULL;
    let kr = unsafe { ffi::task_for_pid(ffi::mach_task_self(), pid as libc::c_int, &mut task_port) };

    if kr == KERN_SUCCESS && task_port != MACH_PORT_NULL {
        return Ok(task_port);
    }

    // Distinguish "process doesn't exist" from "permission denied".
    let exists = process_exists(pid);
    if !exists {
        return Err(RecordError::ProcessNotFound(pid));
    }

    Err(RecordError::PermissionDenied {
        pid,
        detail: format!("task_for_pid returned {kr:#010x}"),
    })
}

pub(crate) fn release_task_port(port: task_t) {
    if port != MACH_PORT_NULL {
        unsafe { ffi::mach_port_deallocate(ffi::mach_task_self(), port) };
    }
}

/// Check whether `pid` is alive using `kill(pid, 0)`.
fn process_exists(pid: u32) -> bool {
    let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if ret == 0 {
        return true;
    }
    // EPERM → process exists but we lack permission to signal it.
    let errno = unsafe { *libc::__error() };
    errno == libc::EPERM
}

// ---------------------------------------------------------------------------
// Single sample pass
// ---------------------------------------------------------------------------

/// Sample all threads in `task_port` once, returning one [`StackSample`] per
/// thread. Suspends each thread individually to read its register state.
pub(crate) fn sample_once(pid: u32, task_port: task_t, arch: Arch, timestamp_ns: u64) -> Vec<StackSample> {
    let mut thread_list: thread_act_array_t = std::ptr::null_mut();
    let mut thread_count: mach_msg_type_number_t = 0;

    let kr = unsafe { ffi::task_threads(task_port, &mut thread_list, &mut thread_count) };
    if kr != KERN_SUCCESS || thread_list.is_null() || thread_count == 0 {
        if kr != KERN_SUCCESS {
            debug!("task_threads failed: {kr:#010x}");
        }
        return Vec::new();
    }

    let actual = thread_count as usize;
    let n = actual.min(MAX_THREADS);
    // Safety: task_threads guarantees `actual` valid ports in the array.
    let threads: &[thread_act_t] = unsafe { std::slice::from_raw_parts(thread_list, actual) };

    let mut samples = Vec::with_capacity(n);
    for &thread in &threads[..n] {
        if let Some(s) = sample_thread(pid, task_port, thread, arch, timestamp_ns) {
            samples.push(s);
        }
        // Release the send right this slot holds.
        unsafe { ffi::mach_port_deallocate(ffi::mach_task_self(), thread) };
    }
    // Release send rights for threads beyond the cap (they still have rights).
    for &thread in &threads[n..] {
        unsafe { ffi::mach_port_deallocate(ffi::mach_task_self(), thread) };
    }

    // Deallocate the vm_allocated thread list array (full original size).
    let list_bytes = actual * mem::size_of::<thread_act_t>();
    unsafe { ffi::vm_deallocate(ffi::mach_task_self(), thread_list as ffi::vm_offset_t, list_bytes) };

    samples
}

/// Suspend `thread`, read register state and walk the stack, then resume.
///
/// Thread is always resumed, even if state reading fails, to avoid leaving
/// the target process permanently suspended.
fn sample_thread(
    pid: u32,
    task_port: task_t,
    thread: thread_act_t,
    arch: Arch,
    timestamp_ns: u64,
) -> Option<StackSample> {
    // Suspend.
    let kr = unsafe { ffi::thread_suspend(thread) };
    if kr != KERN_SUCCESS {
        debug!("thread_suspend thread={thread}: {kr:#010x}");
        return None;
    }

    // Collect state with resume guaranteed even on early return.
    let result = collect_thread_sample(pid, task_port, thread, arch, timestamp_ns);

    let resume_kr = unsafe { ffi::thread_resume(thread) };
    if resume_kr != KERN_SUCCESS {
        // This is a serious problem — log loudly.
        warn!("thread_resume thread={thread}: {resume_kr:#010x}; target may be stuck");
    }

    result
}

fn collect_thread_sample(
    pid: u32,
    task_port: task_t,
    thread: thread_act_t,
    arch: Arch,
    timestamp_ns: u64,
) -> Option<StackSample> {
    let tid = get_thread_id(thread).unwrap_or(thread as u64);

    let (pc, fp) = match arch {
        Arch::Arm64 => read_arm64_regs(thread)?,
        Arch::X86_64 => read_x86_64_regs(thread)?,
    };

    let frames = walk_stack(task_port, arch, pc, fp);
    if frames.is_empty() {
        return None;
    }

    Some(StackSample { timestamp_ns, pid, tid, frames })
}

// ---------------------------------------------------------------------------
// Register state reading
// ---------------------------------------------------------------------------

/// Returns `(pc, fp)` for an ARM64 thread.
fn read_arm64_regs(thread: thread_act_t) -> Option<(u64, u64)> {
    let mut state = arm_thread_state64_t::default();
    let mut count = ARM_THREAD_STATE64_COUNT;
    let kr = unsafe {
        ffi::thread_get_state(
            thread,
            ARM_THREAD_STATE64,
            &mut state as *mut _ as ffi::thread_state_t,
            &mut count,
        )
    };
    if kr != KERN_SUCCESS {
        debug!("thread_get_state(ARM64) thread={thread}: {kr:#010x}");
        return None;
    }
    Some((state.__pc, state.__fp))
}

/// Returns `(rip, rbp)` for an x86_64 thread.
fn read_x86_64_regs(thread: thread_act_t) -> Option<(u64, u64)> {
    let mut state = x86_thread_state64_t::default();
    let mut count = x86_THREAD_STATE64_COUNT;
    let kr = unsafe {
        ffi::thread_get_state(
            thread,
            x86_THREAD_STATE64,
            &mut state as *mut _ as ffi::thread_state_t,
            &mut count,
        )
    };
    if kr != KERN_SUCCESS {
        debug!("thread_get_state(x86_64) thread={thread}: {kr:#010x}");
        return None;
    }
    Some((state.__rip, state.__rbp))
}

fn get_thread_id(thread: thread_act_t) -> Option<u64> {
    let mut info = thread_identifier_info_t::default();
    let mut count = THREAD_IDENTIFIER_INFO_COUNT;
    let kr = unsafe {
        ffi::thread_info(
            thread,
            THREAD_IDENTIFIER_INFO,
            &mut info as *mut _ as *mut ffi::integer_t,
            &mut count,
        )
    };
    if kr == KERN_SUCCESS { Some(info.thread_id) } else { None }
}

// ---------------------------------------------------------------------------
// Frame-pointer stack walking
// ---------------------------------------------------------------------------

/// Walk the frame-pointer chain from `(pc, fp)` in `task_port`'s address space.
///
/// Frame record layout (both arches, stack grows down):
/// ```text
///   fp  →  [saved_fp(u64) | saved_lr/return_addr(u64)]
/// ```
/// Higher-addressed frames have higher fp values (stack grows downward).
///
/// PAC (pointer-authentication): saved return addresses on ARM64 may carry
/// PAC bits in the upper 16 bits. We strip those bits before storing frames.
fn walk_stack(task_port: task_t, _arch: Arch, pc: u64, mut fp: u64) -> Vec<u64> {
    let mut frames = Vec::with_capacity(32);
    frames.push(pc);

    // Minimum sensible user-space address (avoids reading near null).
    const MIN_ADDR: u64 = 0x1000;

    while fp >= MIN_ADDR && frames.len() < MAX_FRAMES {
        // Read 16 bytes: [saved_fp, return_address].
        let mut record = [0u64; 2];
        let ok = unsafe {
            ffi::read_task_memory(
                task_port,
                fp,
                record.as_mut_ptr().cast(),
                16,
            )
        };
        if !ok {
            break;
        }

        let saved_fp = strip_pac(record[0]);
        let return_addr = strip_pac(record[1]);

        if return_addr < MIN_ADDR {
            break;
        }

        frames.push(return_addr);

        // Guard against corrupt or non-decreasing frame pointers.
        // On a downward-growing stack, each saved_fp must be strictly greater
        // than (i.e., deeper in the stack than) the current fp.
        if saved_fp <= fp {
            break;
        }
        fp = saved_fp;
    }

    frames
}

/// Strip pointer-authentication code bits.
///
/// Current Apple Silicon hardware uses at most 39-bit user-space VAs.
/// We use a 48-bit mask, which is safe for both ARM64 and x86_64 and
/// wider than any current PAC implementation.
#[inline]
fn strip_pac(addr: u64) -> u64 {
    addr & 0x0000_FFFF_FFFF_FFFF
}

// ---------------------------------------------------------------------------
// Background sampling thread
// ---------------------------------------------------------------------------

pub(crate) struct SamplerHandle {
    stop_tx: SyncSender<()>,
    join: thread::JoinHandle<Vec<StackSample>>,
}

impl SamplerHandle {
    /// Spawn the sampler on a dedicated OS thread.
    ///
    /// `task_port` must remain valid until `stop()` is called.
    pub(crate) fn spawn(
        pid: u32,
        task_port: task_t,
        arch: Arch,
        rate_hz: u32,
    ) -> Result<Self, RecordError> {
        let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel::<()>(1);
        let join = thread::Builder::new()
            .name(format!("macos-sampler-{pid}"))
            .spawn(move || sampling_loop(pid, task_port, arch, rate_hz, stop_rx))?;
        Ok(Self { stop_tx, join })
    }

    /// Signal the sampler to stop and collect all accumulated samples.
    pub(crate) fn stop(self) -> Vec<StackSample> {
        // Best-effort send; if the channel is full the loop will see the next attempt.
        let _ = self.stop_tx.try_send(());
        self.join.join().unwrap_or_default()
    }
}

fn sampling_loop(
    pid: u32,
    task_port: task_t,
    arch: Arch,
    rate_hz: u32,
    stop_rx: Receiver<()>,
) -> Vec<StackSample> {
    let interval = Duration::from_secs_f64(1.0 / rate_hz.max(1) as f64);
    let mut all = Vec::new();

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        let deadline = Instant::now() + interval;
        let ts_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        all.extend(sample_once(pid, task_port, arch, ts_ns));

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining > Duration::ZERO {
            thread::sleep(remaining);
        }
    }

    all
}
