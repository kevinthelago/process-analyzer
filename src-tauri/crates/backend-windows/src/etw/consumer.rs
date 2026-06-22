// ETW real-time consumer thread.
//
// Opens the NT Kernel Logger for real-time event consumption and dispatches
// events via a callback.  ProcessTrace() is blocking, so it lives on its own
// OS thread.  CloseTrace() causes ProcessTrace() to return, which is the
// clean shutdown path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, OpenTraceW, ProcessTrace, EVENT_RECORD, EVENT_TRACE_LOGFILEW,
    PROCESSTRACE_HANDLE,
};

use crate::error::RecordError;
use crate::etw::session::EtwSession;

// "NT Kernel Logger\0" wide string for OpenTraceW.
const KERNEL_LOGGER_NAME_W: &[u16] = &[
    0x004E, 0x0054, 0x0020, 0x004B, 0x0065, 0x0072, 0x006E, 0x0065,
    0x006C, 0x0020, 0x004C, 0x006F, 0x0067, 0x0067, 0x0065, 0x0072,
    0x0000,
];

// PROCESS_TRACE_MODE_REAL_TIME = 0x00000100
// PROCESS_TRACE_MODE_EVENT_RECORD = 0x10000000
const PROCESS_TRACE_MODE_REAL_TIME: u32 = 0x0000_0100;
const PROCESS_TRACE_MODE_EVENT_RECORD: u32 = 0x1000_0000;

// INVALID_PROCESSTRACE_HANDLE = (TRACEHANDLE)(ULONG_PTR)-1 == 0xFFFFFFFFFFFFFFFF
const INVALID_PROCESSTRACE_HANDLE_VALUE: u64 = u64::MAX;

/// A running ETW consumer thread.
pub struct EtwConsumer {
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    trace_handle: PROCESSTRACE_HANDLE,
}

// SAFETY: PROCESSTRACE_HANDLE is only closed on drop/close, from a single thread.
unsafe impl Send for EtwConsumer {}

impl EtwConsumer {
    /// Open a real-time consumer against the running `session`.
    pub fn open(_session: &EtwSession) -> Result<Self, RecordError> {
        let shutdown = Arc::new(AtomicBool::new(false));

        // The context pointer carries the shutdown flag into the event callback.
        // We leak an Arc clone; it is reclaimed inside the callback when shutdown.
        let ctx_ptr = Arc::into_raw(Arc::clone(&shutdown)) as *mut std::ffi::c_void;

        let mut logfile = EVENT_TRACE_LOGFILEW::default();
        // LogFileName, not LoggerName, is used for real-time consumption.
        logfile.LoggerName =
            windows::core::PWSTR(KERNEL_LOGGER_NAME_W.as_ptr() as *mut u16);
        logfile.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        logfile.Anonymous2.EventRecordCallback = Some(event_record_callback);
        logfile.Context = ctx_ptr;

        let trace_handle: PROCESSTRACE_HANDLE = unsafe { OpenTraceW(&mut logfile) };

        if trace_handle.Value == INVALID_PROCESSTRACE_HANDLE_VALUE {
            // Reclaim the Arc we leaked into the context pointer.
            unsafe { drop(Arc::from_raw(ctx_ptr as *const AtomicBool)) };
            let err = unsafe { windows::Win32::Foundation::GetLastError() };
            return Err(RecordError::EtwConsumer(format!(
                "OpenTraceW failed: {err:?}"
            )));
        }

        let shutdown_clone = Arc::clone(&shutdown);
        let thread = thread::Builder::new()
            .name("etw-consumer".into())
            .spawn(move || {
                let mut handles = [trace_handle];
                // ProcessTrace blocks until CloseTrace() is called or the session ends.
                let rc: WIN32_ERROR =
                    unsafe { ProcessTrace(handles.as_mut_slice(), None, None) };
                if rc.is_err() && !shutdown_clone.load(Ordering::Relaxed) {
                    tracing::error!("ProcessTrace returned error: {rc:?}");
                }
            })
            .map_err(|e| RecordError::Internal(format!("spawn etw-consumer: {e}")))?;

        Ok(EtwConsumer {
            shutdown,
            thread: Some(thread),
            trace_handle,
        })
    }

    /// Signal shutdown and wait for the consumer thread to exit.
    pub fn close(mut self) {
        self.do_close();
    }

    fn do_close(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = unsafe { CloseTrace(self.trace_handle) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for EtwConsumer {
    fn drop(&mut self) {
        self.do_close();
    }
}

// ---------------------------------------------------------------------------
// Event callback (called by ProcessTrace on the consumer thread)
// ---------------------------------------------------------------------------

unsafe extern "system" fn event_record_callback(record: *mut EVENT_RECORD) {
    let record = unsafe { &*record };

    // Check shutdown flag via the context pointer (an Arc<AtomicBool>).
    let shutdown = record.UserContext as *const AtomicBool;
    if !shutdown.is_null() && unsafe { (*shutdown).load(Ordering::Relaxed) } {
        return;
    }

    let opcode = record.EventHeader.EventDescriptor.Opcode;
    let provider = record.EventHeader.ProviderId;

    // ImageLoadGuid {2cb15d1d-5fc1-11d2-abe1-00a0c911f518}, opcode 10 = image load.
    // Logged so the symbols stream can resolve PDBs for sampled addresses.
    const IMAGE_LOAD_GUID: windows::core::GUID = windows::core::GUID {
        data1: 0x2cb1_5d1d,
        data2: 0x5fc1,
        data3: 0x11d2,
        data4: [0xab, 0xe1, 0x00, 0xa0, 0xc9, 0x11, 0xf5, 0x18],
    };

    if provider == IMAGE_LOAD_GUID && opcode == 10 {
        tracing::trace!(
            pid = record.EventHeader.ProcessId,
            tid = record.EventHeader.ThreadId,
            "image-load event"
        );
    }
}
