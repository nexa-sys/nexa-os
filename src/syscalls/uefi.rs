//! UEFI compatibility syscalls
//!
//! Implements: uefi_get_counts, uefi_get_fb_info, uefi_get_net_info,
//!             uefi_get_block_info, uefi_map_net_mmio, uefi_get_usb_info,
//!             uefi_get_hid_info, uefi_map_usb_mmio

use super::types::*;
use crate::paging;
use crate::posix;
use crate::uefi_compat::{
    self, BlockDescriptor, CompatCounts, HidInputDescriptor, NetworkDescriptor,
    UsbHostDescriptor,
};
use core::mem;
use core::ptr;
use nexa_boot_info::FramebufferInfo;

/// SYS_UEFI_GET_COUNTS - Get UEFI compatibility layer device counts
pub fn uefi_get_counts(out: *mut CompatCounts) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }
    let length = mem::size_of::<CompatCounts>() as u64;
    if !user_buffer_in_range(out as u64, length) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let counts = uefi_compat::counts();
    unsafe {
        ptr::write(out, counts);
    }
    posix::set_errno(0);
    0
}

/// SYS_UEFI_GET_FB_INFO - Get framebuffer information
pub fn uefi_get_fb_info(out: *mut FramebufferInfo) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }
    let length = mem::size_of::<FramebufferInfo>() as u64;
    if !user_buffer_in_range(out as u64, length) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let Some(info) = uefi_compat::framebuffer() else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    unsafe {
        ptr::write(out, info);
    }
    posix::set_errno(0);
    0
}

/// SYS_UEFI_GET_NET_INFO - Get network device information
pub fn uefi_get_net_info(index: usize, out: *mut NetworkDescriptor) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let length = mem::size_of::<NetworkDescriptor>() as u64;
    if !user_buffer_in_range(out as u64, length) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let Some(descriptor) = uefi_compat::network_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    unsafe {
        ptr::write(out, descriptor);
    }
    posix::set_errno(0);
    0
}

/// SYS_UEFI_GET_BLOCK_INFO - Get block device information
pub fn uefi_get_block_info(index: usize, out: *mut BlockDescriptor) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let length = mem::size_of::<BlockDescriptor>() as u64;
    if !user_buffer_in_range(out as u64, length) {
        posix::set_errno(posix::errno::EFAULT);
        return u64::MAX;
    }

    let Some(descriptor) = uefi_compat::block_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    unsafe {
        ptr::write(out, descriptor);
    }
    posix::set_errno(0);
    0
}

/// SYS_UEFI_MAP_NET_MMIO - Map network device MMIO region to userspace
pub fn uefi_map_net_mmio(index: usize) -> u64 {
    let Some(descriptor) = uefi_compat::network_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    if descriptor.mmio_base == 0 {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    }

    let span = if descriptor.mmio_length == 0 {
        0x1000
    } else {
        descriptor
            .mmio_length
            .min(u64::from(usize::MAX as u32))
            .max(0x1000)
    } as usize;

    let map_result = unsafe { paging::map_user_device_region(descriptor.mmio_base, span) };
    match map_result {
        Ok(ptr) => {
            posix::set_errno(0);
            ptr as u64
        }
        Err(paging::MapDeviceError::OutOfTableSpace) => {
            posix::set_errno(posix::errno::ENOMEM);
            u64::MAX
        }
    }
}

/// SYS_UEFI_GET_USB_INFO - Get USB host controller information
pub fn uefi_get_usb_info(index: usize, out: *mut UsbHostDescriptor) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let Some(descriptor) = uefi_compat::usb_host_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    unsafe {
        ptr::write(out, descriptor);
    }
    posix::set_errno(0);
    0
}

/// SYS_UEFI_GET_HID_INFO - Get HID input device information
pub fn uefi_get_hid_info(index: usize, out: *mut HidInputDescriptor) -> u64 {
    if out.is_null() {
        posix::set_errno(posix::errno::EINVAL);
        return u64::MAX;
    }

    let Some(descriptor) = uefi_compat::hid_input_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    unsafe {
        ptr::write(out, descriptor);
    }
    posix::set_errno(0);
    0
}

/// SYS_UEFI_MAP_USB_MMIO - Map USB host controller MMIO region to userspace
pub fn uefi_map_usb_mmio(index: usize) -> u64 {
    let Some(descriptor) = uefi_compat::usb_host_descriptor(index) else {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    };

    if descriptor.mmio_base == 0 {
        posix::set_errno(posix::errno::ENODEV);
        return u64::MAX;
    }

    let span = if descriptor.mmio_size == 0 {
        0x1000
    } else {
        descriptor
            .mmio_size
            .min(u64::from(usize::MAX as u32))
            .max(0x1000)
    } as usize;

    let map_result = unsafe { paging::map_user_device_region(descriptor.mmio_base, span) };
    match map_result {
        Ok(ptr) => {
            posix::set_errno(0);
            ptr as u64
        }
        Err(paging::MapDeviceError::OutOfTableSpace) => {
            posix::set_errno(posix::errno::ENOMEM);
            u64::MAX
        }
    }
}
