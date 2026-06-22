//! [`MacosRecorder`] — top-level macOS implementation of the [`Recorder`] trait.
//!
//! # Privilege / fallback chain
//!
//! 1. **Full mode** (root / debugger entitlement):
//!    - Mach thread-suspend sampling via `task_for_pid`.
//!    - DTrace subprocess for I/O and scheduling events (best-effort; no-op on failure).
//!    - `task_info(TASK_VM_INFO)` memory snapshots.
//!    - dyld image list with UUIDs.
//!
//! 2. **xctrace fallback** (SIP enabled, no debugger entitlement):
//!    - `xctrace record --template "Time Profiler"` captures stack samples.
//!    - I/O, scheduling, memory snapshots, and image list are unavailable.
//!    - Requires Xcode or Xcode Command Line Tools installed.
//!
//! 3. **Hard failure**: `task_for_pid` denied AND `xctrace` not installed →
//!    returns `RecordError::PermissionDenied` with remediation instructions.

use tracing::{info, warn};

use crate::contracts::Recorder;
use crate::dtrace::DTraceHandle;
use crate::dyld;
use crate::error::RecordError;
use crate::ffi::task_t;
use crate::memory::MemoryHandle;
use crate::sampler::{self, SamplerHandle};
use crate::types::{DyldImage, RecordConfig, TraceData};
use crate::xctrace::XctraceHandle;

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

enum RecordState {
    Idle,
    Full(FullState),
    Xctrace(XctraceHandle),
}

struct FullState {
    pid: u32,
    task_port: task_t,
    sampler: SamplerHandle,
    dtrace: Option<DTraceHandle>,
    memory: Option<MemoryHandle>,
    dyld_images: Vec<DyldImage>,
}

// ---------------------------------------------------------------------------
// Public recorder
// ---------------------------------------------------------------------------

/// macOS implementation of the cross-platform [`Recorder`] contract.
pub struct MacosRecorder {
    state: RecordState,
}

impl Default for MacosRecorder {
    fn default() -> Self {
        Self { state: RecordState::Idle }
    }
}

impl Recorder for MacosRecorder {
    fn start(&mut self, pid: u32, config: RecordConfig) -> Result<(), RecordError> {
        if self.is_recording() {
            return Err(RecordError::AlreadyRecording(pid));
        }

        match start_full(pid, &config) {
            Ok(state) => {
                info!("macOS recorder: full mode for pid {pid}");
                self.state = RecordState::Full(state);
                Ok(())
            }
            Err(e) if e.is_permission_error() => {
                let duration = config.duration_secs.unwrap_or(60.0);
                match XctraceHandle::spawn(pid, duration) {
                    Ok(h) => {
                        warn!(
                            "macOS recorder: Mach access denied for pid {pid}; \
                             using xctrace fallback (I/O/sched/memory unavailable)"
                        );
                        self.state = RecordState::Xctrace(h);
                        Ok(())
                    }
                    Err(xctrace_err) => {
                        warn!("xctrace also unavailable: {xctrace_err}");
                        Err(e)
                    }
                }
            }
            Err(e) => Err(e),
        }
    }

    fn stop(&mut self) -> Result<TraceData, RecordError> {
        match std::mem::replace(&mut self.state, RecordState::Idle) {
            RecordState::Idle => Err(RecordError::NotRecording),
            RecordState::Xctrace(h) => {
                info!("macOS recorder: stopping xctrace");
                h.stop()
            }
            RecordState::Full(s) => {
                info!("macOS recorder: stopping full-mode capture for pid {}", s.pid);
                stop_full(s)
            }
        }
    }

    fn is_recording(&self) -> bool {
        !matches!(self.state, RecordState::Idle)
    }
}

// ---------------------------------------------------------------------------
// Full-mode startup
// ---------------------------------------------------------------------------

fn start_full(pid: u32, config: &RecordConfig) -> Result<FullState, RecordError> {
    let task_port = sampler::acquire_task_port(pid)?;

    let arch = dyld::detect_arch(task_port);

    let dyld_images = match dyld::collect_images(pid, task_port) {
        Ok(imgs) => imgs,
        Err(e) => {
            warn!("dyld image list unavailable for pid {pid}: {e}");
            Vec::new()
        }
    };

    let sampler = SamplerHandle::spawn(pid, task_port, arch, config.sample_rate_hz).map_err(|e| {
        sampler::release_task_port(task_port);
        e
    })?;

    let dtrace = if config.enable_dtrace {
        match DTraceHandle::spawn(pid, config.dtrace_script.as_deref()) {
            Ok(h) => Some(h),
            Err(e) => {
                warn!("DTrace unavailable for pid {pid}: {e}");
                None
            }
        }
    } else {
        None
    };

    let memory = if config.enable_memory {
        match MemoryHandle::spawn(pid, task_port, config.sample_rate_hz) {
            Ok(h) => Some(h),
            Err(e) => {
                warn!("memory sampler failed for pid {pid}: {e}");
                None
            }
        }
    } else {
        None
    };

    Ok(FullState { pid, task_port, sampler, dtrace, memory, dyld_images })
}

// ---------------------------------------------------------------------------
// Full-mode teardown
// ---------------------------------------------------------------------------

fn stop_full(state: FullState) -> Result<TraceData, RecordError> {
    // Stop DTrace first so we capture any late I/O before stack sampling stops.
    let (mut io_events, mut sched_events) =
        state.dtrace.map(|h| h.stop()).unwrap_or_default();

    let stack_samples = state.sampler.stop();
    let memory_snapshots = state.memory.map(|h| h.stop()).unwrap_or_default();

    // Back-fill pid into DTrace events (the parser leaves pid = 0).
    let pid = state.pid;
    for ev in &mut io_events {
        ev.pid = pid;
    }
    for ev in &mut sched_events {
        ev.pid = pid;
    }

    sampler::release_task_port(state.task_port);

    Ok(TraceData {
        stack_samples,
        io_events,
        sched_events,
        memory_snapshots,
        dyld_images: state.dyld_images,
    })
}
