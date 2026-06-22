// ETW NT Kernel Logger session controller.
//
// The NT Kernel Logger is a singleton system-wide ETW session:
//   - Only one can run at a time (ERROR_ALREADY_EXISTS = 183 if occupied).
//   - Session name MUST be "NT Kernel Logger".
//   - Requires Administrator / SeSystemProfilePrivilege.
//   - Wnode.Guid must be SystemTraceControlGuid.

use std::mem;

use windows::core::GUID;
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Diagnostics::Etw::{
    ControlTraceW, StartTraceW, CONTROLTRACE_HANDLE, EVENT_TRACE_CONTROL_QUERY,
    EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_FLAG, EVENT_TRACE_PROPERTIES,
};

use crate::error::RecordError;
use crate::etw::stackwalk;
use crate::recorder::RecordConfig;

// "NT Kernel Logger\0" as UTF-16 LE.
const KERNEL_LOGGER_NAME_W: &[u16] = &[
    0x004E, 0x0054, 0x0020, 0x004B, 0x0065, 0x0072, 0x006E, 0x0065,
    0x006C, 0x0020, 0x004C, 0x006F, 0x0067, 0x0067, 0x0065, 0x0072,
    0x0000,
];
const KERNEL_LOGGER_NAME_BYTES: usize = 17 * 2; // 16 chars + NUL, 2 bytes each

const SYSTEM_TRACE_CONTROL_GUID: GUID = GUID {
    data1: 0x9e81_4aad,
    data2: 0x3204,
    data3: 0x11d2,
    data4: [0x9a, 0x82, 0x00, 0x60, 0x08, 0xa8, 0x69, 0x39],
};

// EVENT_TRACE_REAL_TIME_MODE = 0x100
const LOG_FILE_MODE_REALTIME: u32 = 0x0000_0100;
// WNODE_FLAG_TRACED_GUID = 0x00020000
const WNODE_FLAG_TRACED_GUID: u32 = 0x0002_0000;

/// Live statistics read via `ControlTraceW(EVENT_TRACE_CONTROL_QUERY)`.
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub buffers_written: u32,
    pub log_buffers_lost: u32,
    pub events_lost: u32,
    pub realtime_buffers_lost: u32,
    pub number_of_buffers: u32,
    pub free_buffers: u32,
}

/// A running NT Kernel Logger ETW session.
pub struct EtwSession {
    handle: CONTROLTRACE_HANDLE,
}

// SAFETY: The session handle is only used from the thread that calls start/stop/query.
unsafe impl Send for EtwSession {}

impl EtwSession {
    pub fn start(config: &RecordConfig) -> Result<Self, RecordError> {
        let mut buf = PropsBuffer::new();
        let total_size = buf.total_size() as u32;

        {
            let props = buf.props_mut();
            props.Wnode.BufferSize = total_size;
            props.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
            props.Wnode.Guid = SYSTEM_TRACE_CONTROL_GUID;
            props.LogFileMode = LOG_FILE_MODE_REALTIME;
            props.EnableFlags = EVENT_TRACE_FLAG(config.kernel_flags.bits());
            if config.buffer_size_kb > 0 {
                props.BufferSize = config.buffer_size_kb;
            }
            if config.max_buffers > 0 {
                props.MaximumBuffers = config.max_buffers;
            }
            props.LoggerNameOffset = mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32;
            props.LogFileNameOffset = 0; // real-time — no log file
        }

        // Write session name as UTF-16 into the bytes immediately after the struct.
        {
            let name_dst = buf.name_bytes_mut();
            for (i, &w) in KERNEL_LOGGER_NAME_W.iter().enumerate() {
                let off = i * 2;
                name_dst[off] = (w & 0xFF) as u8;
                name_dst[off + 1] = (w >> 8) as u8;
            }
        }

        let mut handle = CONTROLTRACE_HANDLE::default();
        let rc: WIN32_ERROR = unsafe {
            StartTraceW(
                &mut handle,
                windows::core::PCWSTR(KERNEL_LOGGER_NAME_W.as_ptr()),
                buf.props_mut() as *mut _,
            )
        };

        if rc.is_err() {
            return Err(RecordError::from_win32(rc));
        }

        let session = EtwSession { handle };

        if config.collect_stacks {
            if let Err(e) = stackwalk::enable_stacks(handle, config.kernel_flags) {
                tracing::warn!("Stack-walk enablement failed (non-fatal): {e}");
            }
        }

        Ok(session)
    }

    pub fn query(&self) -> Result<SessionStats, RecordError> {
        let mut buf = PropsBuffer::new();
        let total_size = buf.total_size() as u32;
        {
            let props = buf.props_mut();
            props.Wnode.BufferSize = total_size;
            props.Wnode.Guid = SYSTEM_TRACE_CONTROL_GUID;
            props.LoggerNameOffset = mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32;
        }

        let rc: WIN32_ERROR = unsafe {
            ControlTraceW(
                self.handle,
                windows::core::PCWSTR::null(),
                buf.props_mut() as *mut _,
                EVENT_TRACE_CONTROL_QUERY,
            )
        };

        if rc.is_err() {
            return Err(RecordError::from_win32(rc));
        }

        let p = buf.props_ref();
        Ok(SessionStats {
            buffers_written: p.BuffersWritten,
            log_buffers_lost: p.LogBuffersLost,
            events_lost: p.EventsLost,
            realtime_buffers_lost: p.RealTimeBuffersLost,
            number_of_buffers: p.NumberOfBuffers,
            free_buffers: p.FreeBuffers,
        })
    }

    pub fn stop(&mut self) -> Result<(), RecordError> {
        let mut buf = PropsBuffer::new();
        let total_size = buf.total_size() as u32;
        {
            let props = buf.props_mut();
            props.Wnode.BufferSize = total_size;
            props.Wnode.Guid = SYSTEM_TRACE_CONTROL_GUID;
            props.LoggerNameOffset = mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32;
        }

        let rc: WIN32_ERROR = unsafe {
            ControlTraceW(
                self.handle,
                windows::core::PCWSTR::null(),
                buf.props_mut() as *mut _,
                EVENT_TRACE_CONTROL_STOP,
            )
        };

        // ERROR_MORE_DATA (234) is benign on stop.
        if rc.is_err() && rc.0 != 234 {
            return Err(RecordError::from_win32(rc));
        }
        Ok(())
    }

    pub fn handle(&self) -> CONTROLTRACE_HANDLE {
        self.handle
    }
}

// ---------------------------------------------------------------------------
// PropsBuffer — EVENT_TRACE_PROPERTIES with trailing session-name bytes
// ---------------------------------------------------------------------------

struct PropsBuffer {
    data: Vec<u8>,
}

impl PropsBuffer {
    fn new() -> Self {
        let size = mem::size_of::<EVENT_TRACE_PROPERTIES>() + KERNEL_LOGGER_NAME_BYTES;
        PropsBuffer { data: vec![0u8; size] }
    }

    fn total_size(&self) -> usize {
        self.data.len()
    }

    fn props_mut(&mut self) -> &mut EVENT_TRACE_PROPERTIES {
        // SAFETY: data is zero-initialized and large enough for EVENT_TRACE_PROPERTIES.
        // Vec<u8> is allocated with 1-byte alignment which is sufficient here because
        // all fields in EVENT_TRACE_PROPERTIES are accessed through the &mut reference
        // (no packed repr), and rustc/LLVM will handle any field-level alignment.
        unsafe { &mut *(self.data.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES) }
    }

    fn props_ref(&self) -> &EVENT_TRACE_PROPERTIES {
        unsafe { &*(self.data.as_ptr() as *const EVENT_TRACE_PROPERTIES) }
    }

    fn name_bytes_mut(&mut self) -> &mut [u8] {
        let off = mem::size_of::<EVENT_TRACE_PROPERTIES>();
        &mut self.data[off..]
    }
}
