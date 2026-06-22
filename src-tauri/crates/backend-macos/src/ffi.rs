//! Raw Mach kernel FFI declarations.
//!
//! Types and constants match `<mach/mach.h>` and related headers exactly.
//! Everything here is `pub(crate)` and must only be called from `unsafe` blocks
//! with documented invariants.

#![allow(non_camel_case_types, non_upper_case_globals, dead_code, clippy::upper_case_acronyms)]

use libc::{c_int, c_uint, c_void};

// ---------------------------------------------------------------------------
// Core Mach types
// ---------------------------------------------------------------------------

pub(crate) type kern_return_t = c_int;
pub(crate) type mach_port_t = c_uint;
pub(crate) type task_t = mach_port_t;
pub(crate) type thread_act_t = mach_port_t;
pub(crate) type thread_act_array_t = *mut thread_act_t;
pub(crate) type mach_msg_type_number_t = c_uint;
pub(crate) type natural_t = c_uint;
pub(crate) type integer_t = c_int;

pub(crate) type mach_vm_address_t = u64;
pub(crate) type mach_vm_size_t = u64;
pub(crate) type vm_offset_t = usize;
pub(crate) type vm_size_t = usize;

pub(crate) type thread_state_flavor_t = c_int;
pub(crate) type thread_state_t = *mut natural_t;

// ---------------------------------------------------------------------------
// Return codes
// ---------------------------------------------------------------------------

pub(crate) const KERN_SUCCESS: kern_return_t = 0;
pub(crate) const MACH_PORT_NULL: mach_port_t = 0;

// ---------------------------------------------------------------------------
// Thread state flavors
// ---------------------------------------------------------------------------

pub(crate) const ARM_THREAD_STATE64: thread_state_flavor_t = 6;
pub(crate) const x86_THREAD_STATE64: thread_state_flavor_t = 4;

// Thread state counts derived from struct sizes (computed at compile time on the macOS target).
pub(crate) const ARM_THREAD_STATE64_COUNT: mach_msg_type_number_t =
    (core::mem::size_of::<arm_thread_state64_t>() / core::mem::size_of::<natural_t>()) as u32;
pub(crate) const x86_THREAD_STATE64_COUNT: mach_msg_type_number_t =
    (core::mem::size_of::<x86_thread_state64_t>() / core::mem::size_of::<natural_t>()) as u32;

// ---------------------------------------------------------------------------
// task_info flavors
// ---------------------------------------------------------------------------

pub(crate) const TASK_DYLD_INFO: integer_t = 17;
pub(crate) const TASK_VM_INFO: integer_t = 22;

// ---------------------------------------------------------------------------
// thread_info flavors
// ---------------------------------------------------------------------------

pub(crate) const THREAD_IDENTIFIER_INFO: integer_t = 4;

// ---------------------------------------------------------------------------
// ARM64 thread state
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct arm_thread_state64_t {
    pub __x: [u64; 29], // x0–x28
    pub __fp: u64,       // x29 (frame pointer)
    pub __lr: u64,       // x30 (link register)
    pub __sp: u64,       // stack pointer
    pub __pc: u64,       // program counter
    pub __cpsr: u32,
    pub __opaque_flags: u32,
}

// ---------------------------------------------------------------------------
// x86_64 thread state
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct x86_thread_state64_t {
    pub __rax: u64,
    pub __rbx: u64,
    pub __rcx: u64,
    pub __rdx: u64,
    pub __rdi: u64,
    pub __rsi: u64,
    pub __rbp: u64, // frame pointer
    pub __rsp: u64, // stack pointer
    pub __r8: u64,
    pub __r9: u64,
    pub __r10: u64,
    pub __r11: u64,
    pub __r12: u64,
    pub __r13: u64,
    pub __r14: u64,
    pub __r15: u64,
    pub __rip: u64, // instruction pointer
    pub __rflags: u64,
    pub __cs: u64,
    pub __fs: u64,
    pub __gs: u64,
}

// ---------------------------------------------------------------------------
// Thread identifier info
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct thread_identifier_info_t {
    /// System-wide unique thread ID (matches `tid` in DTrace / `ps M`).
    pub thread_id: u64,
    pub thread_handle: u64,   // pthread_t (opaque)
    pub dispatch_qaddr: u64,  // GCD queue address
}

pub(crate) const THREAD_IDENTIFIER_INFO_COUNT: mach_msg_type_number_t =
    (core::mem::size_of::<thread_identifier_info_t>() / core::mem::size_of::<natural_t>()) as u32;

// ---------------------------------------------------------------------------
// task_dyld_info (TASK_DYLD_INFO = 17)
// ---------------------------------------------------------------------------

/// Pointer to dyld's all-image-info structure in the target address space.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct task_dyld_info_t {
    pub all_image_info_addr: mach_vm_address_t,
    pub all_image_info_size: mach_vm_size_t,
    pub all_image_info_format: integer_t,
}

pub(crate) const TASK_DYLD_INFO_COUNT: mach_msg_type_number_t =
    (core::mem::size_of::<task_dyld_info_t>() / core::mem::size_of::<natural_t>()) as u32;

// ---------------------------------------------------------------------------
// dyld all-image-infos (first-version fields only)
// From <mach-o/dyld_images.h>, struct dyld_all_image_infos version 1.
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct dyld_all_image_infos_v1 {
    pub version: u32,
    pub info_array_count: u32,
    /// Pointer to `dyld_image_info[]` in the target process.
    pub info_array: u64,
}

/// Entry in the dyld image list.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct dyld_image_info {
    /// Pointer to `struct mach_header` (or `mach_header_64`) in target VA space.
    pub image_load_address: u64,
    /// Pointer to C string (path) in target VA space.
    pub image_file_path: u64,
    pub image_file_mod_date: u64,
}

// ---------------------------------------------------------------------------
// Mach-O structures (used for UUID extraction)
// ---------------------------------------------------------------------------

pub(crate) const MH_MAGIC_64: u32 = 0xFEED_FACF;
pub(crate) const LC_UUID: u32 = 0x0000_001B;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct mach_header_64 {
    pub magic: u32,
    pub cputype: i32,
    pub cpusubtype: i32,
    pub filetype: u32,
    pub ncmds: u32,
    pub sizeofcmds: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct load_command {
    pub cmd: u32,
    pub cmdsize: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct uuid_command {
    pub cmd: u32,
    pub cmdsize: u32,
    pub uuid: [u8; 16],
}

pub(crate) const CPU_TYPE_X86_64: i32 = 0x0100_0007_u32 as i32;
pub(crate) const CPU_TYPE_ARM64: i32 = 0x0100_000C_u32 as i32;

// ---------------------------------------------------------------------------
// task_vm_info (TASK_VM_INFO = 22)
// We only define through `phys_footprint`; the kernel fills what it can.
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct task_vm_info_partial {
    pub virtual_size: mach_vm_size_t,      // 0
    pub region_count: integer_t,           // 8
    pub page_size: integer_t,              // 12
    pub resident_size: mach_vm_size_t,     // 16
    pub resident_size_peak: mach_vm_size_t,// 24
    pub device: mach_vm_address_t,         // 32
    pub device_peak: mach_vm_size_t,       // 40
    pub internal: mach_vm_address_t,       // 48
    pub internal_peak: mach_vm_size_t,     // 56
    pub external: mach_vm_address_t,       // 64
    pub external_peak: mach_vm_size_t,     // 72
    pub reusable: mach_vm_address_t,       // 80
    pub reusable_peak: mach_vm_size_t,     // 88
    pub purgeable_volatile_pmap: mach_vm_size_t,     // 96
    pub purgeable_volatile_resident: mach_vm_size_t, // 104
    pub purgeable_volatile_virtual: mach_vm_size_t,  // 112
    pub compressed: mach_vm_size_t,        // 120
    pub compressed_peak: mach_vm_size_t,   // 128
    pub compressed_lifetime: mach_vm_size_t,// 136
    pub phys_footprint: mach_vm_size_t,    // 144
}

pub(crate) const TASK_VM_INFO_PARTIAL_COUNT: mach_msg_type_number_t =
    (core::mem::size_of::<task_vm_info_partial>() / core::mem::size_of::<natural_t>()) as u32;

// ---------------------------------------------------------------------------
// Extern declarations — link against libSystem (always available on macOS)
// ---------------------------------------------------------------------------

#[link(name = "System")]
extern "C" {
    pub(crate) fn mach_task_self() -> mach_port_t;

    /// Obtain the Mach task port for `pid`.
    ///
    /// # Safety
    /// Caller must ensure `task` is a valid writable pointer.
    /// Returns KERN_SUCCESS on success; port must be deallocated via
    /// `mach_port_deallocate` when no longer needed.
    pub(crate) fn task_for_pid(
        host: mach_port_t,
        pid: c_int,
        task: *mut task_t,
    ) -> kern_return_t;

    /// Enumerate all threads in `target_task`.
    ///
    /// # Safety
    /// On success `*act_list` points to a vm_allocate'd array of `*act_list_count`
    /// send rights; caller must `vm_deallocate` the array and `mach_port_deallocate`
    /// each entry.
    pub(crate) fn task_threads(
        target_task: task_t,
        act_list: *mut thread_act_array_t,
        act_list_count: *mut mach_msg_type_number_t,
    ) -> kern_return_t;

    pub(crate) fn thread_suspend(target_thread: thread_act_t) -> kern_return_t;
    pub(crate) fn thread_resume(target_thread: thread_act_t) -> kern_return_t;

    pub(crate) fn thread_get_state(
        target_thread: thread_act_t,
        flavor: thread_state_flavor_t,
        old_state: thread_state_t,
        old_state_count: *mut mach_msg_type_number_t,
    ) -> kern_return_t;

    pub(crate) fn thread_info(
        target_act: thread_act_t,
        flavor: integer_t,
        thread_info_out: *mut integer_t,
        thread_info_out_count: *mut mach_msg_type_number_t,
    ) -> kern_return_t;

    /// Read memory from `target_task`'s address space without copying via vm_map.
    ///
    /// # Safety
    /// `data` must point to a buffer of at least `size` bytes owned by the caller.
    pub(crate) fn mach_vm_read_overwrite(
        target_task: mach_port_t,
        address: mach_vm_address_t,
        size: mach_vm_size_t,
        data: mach_vm_address_t,
        out_size: *mut mach_vm_size_t,
    ) -> kern_return_t;

    pub(crate) fn task_info(
        target_task: task_t,
        flavor: integer_t,
        task_info_out: *mut integer_t,
        task_info_out_count: *mut mach_msg_type_number_t,
    ) -> kern_return_t;

    pub(crate) fn mach_port_deallocate(task: mach_port_t, name: mach_port_t) -> kern_return_t;

    pub(crate) fn vm_deallocate(
        target_task: mach_port_t,
        address: vm_offset_t,
        size: vm_size_t,
    ) -> kern_return_t;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read `size` bytes from `address` in `task_port`'s address space into `buf`.
///
/// Returns `true` on success.
///
/// # Safety
/// `buf` must be valid for writes of `size` bytes.
pub(crate) unsafe fn read_task_memory(
    task_port: task_t,
    address: u64,
    buf: *mut c_void,
    size: usize,
) -> bool {
    let mut out: mach_vm_size_t = 0;
    let kr = mach_vm_read_overwrite(task_port, address, size as u64, buf as u64, &mut out);
    kr == KERN_SUCCESS && out == size as u64
}
