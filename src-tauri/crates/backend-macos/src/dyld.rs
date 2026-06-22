//! dyld image list extraction and architecture detection.
//!
//! Algorithm:
//!   1. `task_info(TASK_DYLD_INFO)` → `all_image_info_addr` in target VA space.
//!   2. `mach_vm_read_overwrite` that address → `dyld_all_image_infos` header.
//!   3. Walk `info_array[0..info_array_count]`, reading each `dyld_image_info`.
//!   4. For each image: read the C-string path and the Mach-O header.
//!   5. Scan load commands for `LC_UUID` to extract the 16-byte build UUID.
//!
//! Architecture detection reads the `cputype` field of the first image's
//! Mach-O header. This correctly identifies Rosetta 2 targets (x86_64
//! running on Apple Silicon) vs. native arm64.

use std::mem;

use crate::error::RecordError;
use crate::ffi::{
    self, dyld_all_image_infos_v1, dyld_image_info, load_command, mach_header_64,
    task_dyld_info_t, uuid_command, task_t, KERN_SUCCESS,
    LC_UUID, MH_MAGIC_64, TASK_DYLD_INFO, TASK_DYLD_INFO_COUNT,
};
use crate::sampler::Arch;
use crate::types::DyldImage;

/// Maximum number of images to enumerate (safety cap against corrupt lists).
const MAX_IMAGES: u32 = 4096;
/// Maximum path length to read for one image.
const MAX_PATH_LEN: usize = 4096;
/// Maximum load commands to walk per image (avoids infinite loops on corrupt headers).
const MAX_LOAD_CMDS: u32 = 512;

// ---------------------------------------------------------------------------
// Architecture detection
// ---------------------------------------------------------------------------

/// Detect the target process's architecture from its first dyld image.
///
/// Falls back to the host architecture on any error.
pub(crate) fn detect_arch(task_port: task_t) -> Arch {
    let Ok(images) = collect_images_raw(task_port) else {
        return Arch::host();
    };
    let Some(first) = images.first() else {
        return Arch::host();
    };

    // Read just the mach_header_64 of the first image to check cputype.
    let mut hdr = mach_header_64::default();
    let ok = unsafe {
        ffi::read_task_memory(
            task_port,
            first.image_load_address,
            &mut hdr as *mut _ as *mut libc::c_void,
            mem::size_of::<mach_header_64>(),
        )
    };
    if !ok {
        return Arch::host();
    }
    if hdr.magic != MH_MAGIC_64 {
        // Could be a fat binary or 32-bit; default to host.
        return Arch::host();
    }

    Arch::from_cputype(hdr.cputype)
}

// ---------------------------------------------------------------------------
// Image list collection
// ---------------------------------------------------------------------------

/// Collect the full dyld image list from a task.
pub(crate) fn collect_images(pid: u32, task_port: task_t) -> Result<Vec<DyldImage>, RecordError> {
    let raw = collect_images_raw(task_port).map_err(|e| {
        RecordError::DyldInfoUnavailable(format!("pid {pid}: {e}"))
    })?;

    let mut images = Vec::with_capacity(raw.len());
    for entry in &raw {
        if let Some(img) = parse_image(task_port, entry) {
            images.push(img);
        }
    }
    Ok(images)
}

/// Read the raw `dyld_image_info` array from the target.
fn collect_images_raw(task_port: task_t) -> Result<Vec<dyld_image_info>, &'static str> {
    // Step 1: TASK_DYLD_INFO
    let mut dyld_info = task_dyld_info_t::default();
    let mut count = TASK_DYLD_INFO_COUNT;
    let kr = unsafe {
        ffi::task_info(
            task_port,
            TASK_DYLD_INFO,
            &mut dyld_info as *mut _ as *mut ffi::integer_t,
            &mut count,
        )
    };
    if kr != KERN_SUCCESS {
        return Err("task_info(TASK_DYLD_INFO) failed");
    }
    if dyld_info.all_image_info_addr == 0 {
        return Err("all_image_info_addr is null");
    }

    // Step 2: Read the dyld_all_image_infos header (version 1 fields).
    let mut all_infos = dyld_all_image_infos_v1::default();
    let ok = unsafe {
        ffi::read_task_memory(
            task_port,
            dyld_info.all_image_info_addr,
            &mut all_infos as *mut _ as *mut libc::c_void,
            mem::size_of::<dyld_all_image_infos_v1>(),
        )
    };
    if !ok {
        return Err("read dyld_all_image_infos failed");
    }

    let n = all_infos.info_array_count.min(MAX_IMAGES);
    if n == 0 || all_infos.info_array == 0 {
        return Ok(Vec::new());
    }

    // Step 3: Read the dyld_image_info array.
    let entry_size = mem::size_of::<dyld_image_info>();
    let buf_size = n as usize * entry_size;
    let mut buf: Vec<u8> = vec![0u8; buf_size];

    let ok = unsafe {
        ffi::read_task_memory(
            task_port,
            all_infos.info_array,
            buf.as_mut_ptr().cast(),
            buf_size,
        )
    };
    if !ok {
        return Err("read dyld_image_info array failed");
    }

    let entries: Vec<dyld_image_info> = buf
        .chunks_exact(entry_size)
        .map(|chunk| {
            let mut entry = dyld_image_info::default();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    chunk.as_ptr(),
                    &mut entry as *mut _ as *mut u8,
                    entry_size,
                );
            }
            entry
        })
        .collect();

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Per-image parsing
// ---------------------------------------------------------------------------

fn parse_image(task_port: task_t, entry: &dyld_image_info) -> Option<DyldImage> {
    if entry.image_load_address == 0 {
        return None;
    }

    let path = read_cstring(task_port, entry.image_file_path, MAX_PATH_LEN)
        .unwrap_or_else(|| "<unknown>".to_owned());

    let (uuid, slide) = read_mach_o_metadata(task_port, entry.image_load_address)
        .unwrap_or(([0u8; 16], 0i64));

    Some(DyldImage {
        load_address: entry.image_load_address,
        slide,
        path,
        uuid,
    })
}

/// Read a null-terminated C string from `address` in the target process.
fn read_cstring(task_port: task_t, address: u64, max_len: usize) -> Option<String> {
    if address == 0 {
        return None;
    }
    let mut buf = vec![0u8; max_len];
    let ok = unsafe {
        ffi::read_task_memory(task_port, address, buf.as_mut_ptr().cast(), max_len)
    };
    if !ok {
        return None;
    }
    // Find null terminator.
    let end = buf.iter().position(|&b| b == 0).unwrap_or(max_len);
    String::from_utf8(buf[..end].to_vec()).ok()
}

/// Read the Mach-O header at `load_address` and return `(uuid, slide)`.
///
/// `slide` is `load_address - preferred_load_address` from the `__TEXT` segment.
/// For simplicity we return `slide = 0` when we cannot find the first segment
/// (UUID is still valid in that case).
fn read_mach_o_metadata(task_port: task_t, load_address: u64) -> Option<([u8; 16], i64)> {
    // Read the Mach-O header.
    let mut hdr = mach_header_64::default();
    let ok = unsafe {
        ffi::read_task_memory(
            task_port,
            load_address,
            &mut hdr as *mut _ as *mut libc::c_void,
            mem::size_of::<mach_header_64>(),
        )
    };
    if !ok || hdr.magic != MH_MAGIC_64 {
        return None;
    }

    // Walk load commands looking for LC_UUID (and optionally LC_SEGMENT_64
    // to compute the slide from the preferred load address of the __TEXT segment).
    let ncmds = hdr.ncmds.min(MAX_LOAD_CMDS);
    let header_size = mem::size_of::<mach_header_64>() as u64;
    let mut offset = load_address + header_size;

    let mut uuid: [u8; 16] = [0u8; 16];
    let mut found_uuid = false;
    let mut preferred_load: u64 = 0;
    let mut found_text = false;

    for _ in 0..ncmds {
        // Read the generic load_command header to get cmd and cmdsize.
        let mut lc = load_command::default();
        let ok = unsafe {
            ffi::read_task_memory(
                task_port,
                offset,
                &mut lc as *mut _ as *mut libc::c_void,
                mem::size_of::<load_command>(),
            )
        };
        if !ok || lc.cmdsize < mem::size_of::<load_command>() as u32 {
            break;
        }

        if lc.cmd == LC_UUID {
            let mut uc = uuid_command::default();
            let ok = unsafe {
                ffi::read_task_memory(
                    task_port,
                    offset,
                    &mut uc as *mut _ as *mut libc::c_void,
                    mem::size_of::<uuid_command>(),
                )
            };
            if ok {
                uuid = uc.uuid;
                found_uuid = true;
            }
        }

        // LC_SEGMENT_64 = 0x19 — read the preferred vmaddr of the first segment
        // (which is __TEXT, i.e., the first segment in a standard Mach-O).
        if lc.cmd == 0x19 && !found_text {
            // segment_command_64 layout:
            //   u32 cmd, u32 cmdsize, [16 bytes segname], u64 vmaddr, ...
            // vmaddr is at offset 24 from the start of the segment_command_64.
            let vmaddr_offset = offset + 24;
            let mut vmaddr: u64 = 0;
            let ok = unsafe {
                ffi::read_task_memory(
                    task_port,
                    vmaddr_offset,
                    &mut vmaddr as *mut _ as *mut libc::c_void,
                    mem::size_of::<u64>(),
                )
            };
            if ok {
                preferred_load = vmaddr;
                found_text = true;
            }
        }

        if found_uuid && found_text {
            break;
        }

        offset += lc.cmdsize as u64;
    }

    let slide = if found_text {
        load_address as i64 - preferred_load as i64
    } else {
        0
    };

    if found_uuid {
        Some((uuid, slide))
    } else {
        // Return zeroed UUID if not found — image list entry is still useful
        // for address → library lookup.
        Some(([0u8; 16], slide))
    }
}
