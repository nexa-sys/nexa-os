use core::ptr;
use core::slice;
use core::str;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::safety::StaticArena;

use nexa_boot_info::{flags, BootInfo, MemoryRegion};

#[derive(Debug)]
pub enum BootInfoError {
    InvalidSignature,
    UnsupportedVersion(u16),
}

static BOOT_INFO_PTR: AtomicPtr<BootInfo> = AtomicPtr::new(ptr::null_mut());
static CMDLINE_STORAGE: StaticArena<512> = StaticArena::new();

/// Registers the UEFI boot information handoff block.
#[allow(dead_code)]
pub fn set(info: &'static BootInfo) -> Result<(), BootInfoError> {
    if !info.has_valid_signature() {
        return Err(BootInfoError::InvalidSignature);
    }
    if info.version != nexa_boot_info::BOOT_INFO_VERSION {
        return Err(BootInfoError::UnsupportedVersion(info.version));
    }
    BOOT_INFO_PTR.store(info as *const BootInfo as *mut BootInfo, Ordering::SeqCst);
    Ok(())
}

/// Clears the registered boot information (used by Multiboot boot path).
pub fn clear() {
    BOOT_INFO_PTR.store(ptr::null_mut(), Ordering::SeqCst);
}

pub fn stash_cmdline(cmdline: &str) -> &'static str {
    match CMDLINE_STORAGE.store_str(cmdline) {
        Ok(value) => value,
        Err(err) => {
            crate::kwarn!("bootinfo cmdline storage overflow: {:?}", err);
            ""
        }
    }
}

fn with_boot_info<F, R>(cb: F) -> Option<R>
where
    F: FnOnce(&'static BootInfo) -> Option<R>,
{
    let ptr = BOOT_INFO_PTR.load(Ordering::SeqCst);
    if ptr.is_null() {
        return None;
    }
    unsafe { cb(&*ptr) }
}

/// Returns the currently registered UEFI boot information block.
#[allow(dead_code)]
pub fn get() -> Option<&'static BootInfo> {
    let ptr = BOOT_INFO_PTR.load(Ordering::SeqCst);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

fn region_slice(region: &MemoryRegion) -> Option<&'static [u8]> {
    if region.is_empty() {
        return None;
    }
    let ptr = region.phys_addr as *const u8;
    if ptr.is_null() {
        return None;
    }
    let len = region.length as usize;
    if len == 0 {
        return None;
    }
    unsafe { Some(slice::from_raw_parts(ptr, len)) }
}

/// Returns initramfs bytes supplied by the UEFI loader, if any.
pub fn initramfs_slice() -> Option<&'static [u8]> {
    with_boot_info(|info| {
        if (info.flags & flags::HAS_INITRAMFS) == 0 {
            return None;
        }
        region_slice(&info.initramfs)
    })
}

/// Returns root filesystem bytes supplied by the UEFI loader, if any.
pub fn rootfs_slice() -> Option<&'static [u8]> {
    with_boot_info(|info| {
        if (info.flags & flags::HAS_ROOTFS) == 0 {
            return None;
        }
        region_slice(&info.rootfs)
    })
}

/// Returns the kernel command line string supplied by the UEFI loader.
pub fn cmdline_str() -> Option<&'static str> {
    with_boot_info(|info| {
        if (info.flags & flags::HAS_CMDLINE) == 0 {
            return None;
        }
        let slice = region_slice(&info.cmdline)?;
        // Command line may not be NUL-terminated; trim trailing NULs if present.
        let mut end = slice.len();
        while end > 0 && slice[end - 1] == 0 {
            end -= 1;
        }
        if end == 0 {
            return None;
        }
        str::from_utf8(&slice[..end]).ok()
    })
}

/// Returns framebuffer description supplied by the UEFI loader.
pub fn framebuffer_info() -> Option<nexa_boot_info::FramebufferInfo> {
    with_boot_info(|info| {
        if (info.flags & flags::HAS_FRAMEBUFFER) == 0 {
            return None;
        }
        Some(info.framebuffer)
    })
}
