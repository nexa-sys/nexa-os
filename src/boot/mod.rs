//! Boot subsystem for NexaOS
//!
//! This module contains all boot-related functionality including:
//! - Boot stage management
//! - Boot information handling  
//! - Init process management
//! - UEFI compatibility layer

pub mod info;
pub mod init;
pub mod stages;
pub mod uefi;

// Re-export commonly used items from info
pub use info::{
    acpi_rsdp_addr, block_devices, clear, cmdline_str, device_descriptors, framebuffer_info,
    get, hid_input_devices, initramfs_slice, kernel_entry_points, kernel_load_offset,
    kernel_segments, network_devices, pci_device_by_location, pci_devices, rootfs_slice, set,
    stash_cmdline, usb_host_devices, BootInfoError,
};

// Re-export from init
pub use init::{
    change_runlevel, current_runlevel, exec_init_process, handle_process_exit, init as init_init,
    is_single_user_mode, load_inittab, parse_inittab_line, reboot, register_default_gettys,
    register_service, shutdown, start_init_process, InitState, RunLevel, ServiceEntry, INIT_PID,
};

// Re-export from stages
pub use stages::{
    advance_stage, boot_config, current_stage, enter_emergency_mode, init, initramfs_stage,
    is_mounted, mark_mounted, mount_real_root, parse_boot_config, pivot_to_real_root,
    start_real_root_init, BootConfig, BootStage,
};

// Re-export from uefi
pub use uefi::{
    block_descriptor, counts as uefi_counts, framebuffer as uefi_framebuffer,
    hid_input_descriptor, init as init_uefi, install_device_nodes, network_descriptor,
    reset as reset_uefi, usb_host_descriptor, BlockDescriptor, CompatCounts, HidInputDescriptor,
    NetworkDescriptor, UsbHostDescriptor,
};
