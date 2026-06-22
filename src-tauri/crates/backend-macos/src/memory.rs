//! Periodic memory snapshot collection via `task_info(TASK_VM_INFO)`.
//!
//! Samples resident size, virtual size, and the "physical footprint"
//! metric shown in Activity Monitor (`phys_footprint` in `task_vm_info`).
//! Runs on a background thread at up to 10 Hz (capped below the stack-sample
//! rate to keep overhead low — memory rarely changes faster than 100 ms).

use std::sync::mpsc::{Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tracing::debug;

use crate::error::RecordError;
use crate::ffi::{
    self, task_t, task_vm_info_partial, KERN_SUCCESS, TASK_VM_INFO,
    TASK_VM_INFO_PARTIAL_COUNT,
};
use crate::types::MemorySnapshot;

/// Maximum memory-sample frequency in Hz.
const MAX_MEM_HZ: u32 = 10;

// ---------------------------------------------------------------------------
// Public handle
// ---------------------------------------------------------------------------

pub(crate) struct MemoryHandle {
    stop_tx: SyncSender<()>,
    join: thread::JoinHandle<Vec<MemorySnapshot>>,
}

impl MemoryHandle {
    /// Spawn a background memory sampler for `task_port` at `rate_hz`
    /// (capped to `MAX_MEM_HZ` internally).
    ///
    /// `task_port` must remain valid until [`stop`][Self::stop] is called.
    pub(crate) fn spawn(pid: u32, task_port: task_t, rate_hz: u32) -> Result<Self, RecordError> {
        let effective_hz = rate_hz.min(MAX_MEM_HZ).max(1);
        let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel::<()>(1);
        let join = thread::Builder::new()
            .name(format!("macos-memory-{pid}"))
            .spawn(move || memory_loop(pid, task_port, effective_hz, stop_rx))
            .map_err(RecordError::Io)?;
        Ok(Self { stop_tx, join })
    }

    pub(crate) fn stop(self) -> Vec<MemorySnapshot> {
        let _ = self.stop_tx.try_send(());
        self.join.join().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Background loop
// ---------------------------------------------------------------------------

fn memory_loop(
    pid: u32,
    task_port: task_t,
    rate_hz: u32,
    stop_rx: Receiver<()>,
) -> Vec<MemorySnapshot> {
    let interval = Duration::from_secs_f64(1.0 / rate_hz as f64);
    let mut snapshots = Vec::new();

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        let deadline = Instant::now() + interval;
        let ts_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        if let Some(snap) = read_vm_info(pid, task_port, ts_ns) {
            snapshots.push(snap);
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining > Duration::ZERO {
            thread::sleep(remaining);
        }
    }

    snapshots
}

// ---------------------------------------------------------------------------
// task_info(TASK_VM_INFO) reader
// ---------------------------------------------------------------------------

fn read_vm_info(pid: u32, task_port: task_t, timestamp_ns: u64) -> Option<MemorySnapshot> {
    let mut info = task_vm_info_partial::default();
    let mut count = TASK_VM_INFO_PARTIAL_COUNT;

    let kr = unsafe {
        ffi::task_info(
            task_port,
            TASK_VM_INFO,
            &mut info as *mut _ as *mut ffi::integer_t,
            &mut count,
        )
    };

    if kr != KERN_SUCCESS {
        debug!("task_info(TASK_VM_INFO) for pid {pid}: {kr:#010x}");
        return None;
    }

    Some(MemorySnapshot {
        timestamp_ns,
        pid,
        resident_bytes: info.resident_size,
        virtual_bytes: info.virtual_size,
        footprint_bytes: info.phys_footprint,
    })
}
