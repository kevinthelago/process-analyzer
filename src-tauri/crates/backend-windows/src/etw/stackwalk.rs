// Enable kernel-side stack walk on PROFILE and CSWITCH events via
// TraceSetInformation(TraceStackTracingInfo).

use windows::core::GUID;
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Diagnostics::Etw::{
    TraceSetInformation, CLASSIC_EVENT_ID, CONTROLTRACE_HANDLE, TRACE_QUERY_INFO_CLASS,
};

use crate::recorder::KernelFlags;

// TraceStackTracingInfo = 3 (TRACE_QUERY_INFO_CLASS ordinal)
const TRACE_STACK_TRACING_INFO: TRACE_QUERY_INFO_CLASS = TRACE_QUERY_INFO_CLASS(3);

// PerfInfoGuid — PROFILE / SampledProfile events.
const PERF_INFO_GUID: GUID = GUID {
    data1: 0xce1d_bfb4,
    data2: 0x137e,
    data3: 0x4da6,
    data4: [0x87, 0xb0, 0x3f, 0x59, 0xaa, 0x10, 0x2c, 0xbc],
};
const OPCODE_SAMPLED_PROFILE: u8 = 46;

// ThreadGuid — Thread/CSwitch events.
const THREAD_GUID: GUID = GUID {
    data1: 0x3d6f_a8d1,
    data2: 0xfe05,
    data3: 0x11d0,
    data4: [0x9d, 0xda, 0x00, 0xc0, 0x4f, 0xd7, 0xba, 0x7c],
};
const OPCODE_CSWITCH: u8 = 36;

/// Ask the kernel to annotate PROFILE and/or CSWITCH events with call-stack addresses.
pub fn enable_stacks(
    session_handle: CONTROLTRACE_HANDLE,
    flags: KernelFlags,
) -> Result<(), crate::error::RecordError> {
    let mut ids: Vec<CLASSIC_EVENT_ID> = Vec::with_capacity(2);

    if flags.contains(KernelFlags::PROFILE) {
        ids.push(CLASSIC_EVENT_ID {
            EventGuid: PERF_INFO_GUID,
            Type: OPCODE_SAMPLED_PROFILE,
            Reserved: [0u8; 7],
        });
    }
    if flags.contains(KernelFlags::CSWITCH) {
        ids.push(CLASSIC_EVENT_ID {
            EventGuid: THREAD_GUID,
            Type: OPCODE_CSWITCH,
            Reserved: [0u8; 7],
        });
    }

    if ids.is_empty() {
        return Ok(());
    }

    let byte_len = (ids.len() * std::mem::size_of::<CLASSIC_EVENT_ID>()) as u32;
    let rc: WIN32_ERROR = unsafe {
        TraceSetInformation(
            session_handle,
            TRACE_STACK_TRACING_INFO,
            ids.as_ptr() as *const _,
            byte_len,
        )
    };

    if rc.is_err() {
        return Err(crate::error::RecordError::from_win32(rc));
    }
    Ok(())
}
