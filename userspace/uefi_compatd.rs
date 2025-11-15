// UEFI compatibility bridge daemon
//
// This process queries kernel-provided syscalls to expose framebuffer,
// network, and block device information preserved from UEFI boot services.

use core::ptr;
use nrlib::{
    get_errno, uefi_get_block, uefi_get_counts, uefi_get_framebuffer, uefi_get_network,
    uefi_map_network_mmio, uefi_get_usb_host, uefi_get_hid_input, uefi_map_usb_mmio,
    UefiBlockDescriptor, UefiCompatCounts, UefiNetworkDescriptor, UefiUsbHostDescriptor,
    UefiHidInputDescriptor,
};
use std::process::exit;

fn main() {
    println!("[uefi-compatd] starting");

    let mut counts = UefiCompatCounts::default();
    if uefi_get_counts(&mut counts) != 0 {
        eprintln!(
            "[uefi-compatd] failed to query device counts (errno={})",
            get_errno()
        );
        exit(1);
    }

    println!(
        "[uefi-compatd] framebuffer={}, network={}, block={}, usb_host={}, hid_input={}",
        counts.framebuffer, counts.network, counts.block, counts.usb_host, counts.hid_input
    );

    if counts.framebuffer != 0 {
        let mut fb_info = unsafe { core::mem::zeroed::<nexa_boot_info::FramebufferInfo>() };
        if uefi_get_framebuffer(&mut fb_info) == 0 {
            println!(
                "[uefi-compatd] framebuffer @ {:#x}, {}x{} pitch={} bpp={}",
                fb_info.address, fb_info.width, fb_info.height, fb_info.pitch, fb_info.bpp
            );
        } else {
            eprintln!(
                "[uefi-compatd] framebuffer query failed (errno={})",
                get_errno()
            );
        }
    }

    for idx in 0..counts.network as usize {
        let mut descriptor = UefiNetworkDescriptor::default();
        if uefi_get_network(idx, &mut descriptor) == 0 {
            let mac = &descriptor.info.mac_address[..descriptor.info.mac_len as usize];
            print!(
                "[uefi-compatd] net{} {:02x?} if_type={} mmio={:#x}+{:#x}",
                idx, mac, descriptor.info.if_type, descriptor.mmio_base, descriptor.mmio_length
            );
            println!(
                " flags={:#x} irq(line={},pin={})",
                descriptor.info.flags, descriptor.interrupt_line, descriptor.interrupt_pin
            );

            let mmio_ptr = uefi_map_network_mmio(idx);
            if mmio_ptr.is_null() {
                eprintln!(
                    "[uefi-compatd] net{} failed to map MMIO (errno={})",
                    idx,
                    get_errno()
                );
            } else {
                let reg0 = unsafe { ptr::read_volatile(mmio_ptr as *const u32) };
                println!(
                    "[uefi-compatd] net{} MMIO mapped at {:p}, REG0={:#x}",
                    idx, mmio_ptr, reg0
                );
            }
        } else {
            eprintln!(
                "[uefi-compatd] net{} query failed (errno={})",
                idx,
                get_errno()
            );
        }
    }

    for idx in 0..counts.block as usize {
        let mut descriptor = UefiBlockDescriptor::default();
        if uefi_get_block(idx, &mut descriptor) == 0 {
            println!(
                "[uefi-compatd] block{} size={} last_lba={} mmio={:#x}+{:#x} flags={:#x}",
                idx,
                descriptor.info.block_size,
                descriptor.info.last_block,
                descriptor.mmio_base,
                descriptor.mmio_length,
                descriptor.info.flags
            );
        } else {
            eprintln!(
                "[uefi-compatd] block{} query failed (errno={})",
                idx,
                get_errno()
            );
        }
    }

    for idx in 0..counts.usb_host as usize {
        let mut descriptor = UefiUsbHostDescriptor::default();
        if uefi_get_usb_host(idx, &mut descriptor) == 0 {
            let controller_type = match descriptor.info.controller_type {
                1 => "OHCI",
                2 => "EHCI",
                3 => "xHCI",
                _ => "Unknown",
            };
            println!(
                "[uefi-compatd] usb{} {} USB{}.{} ports={} mmio={:#x}+{:#x}",
                idx,
                controller_type,
                descriptor.info.usb_version >> 8,
                descriptor.info.usb_version & 0xFF,
                descriptor.info.port_count,
                descriptor.mmio_base,
                descriptor.mmio_size
            );

            let mmio_ptr = uefi_map_usb_mmio(idx);
            if mmio_ptr.is_null() {
                eprintln!(
                    "[uefi-compatd] usb{} failed to map MMIO (errno={})",
                    idx,
                    get_errno()
                );
            } else {
                println!(
                    "[uefi-compatd] usb{} MMIO mapped at {:p}",
                    idx, mmio_ptr
                );
                // Read first register to verify access
                let reg0 = unsafe { ptr::read_volatile(mmio_ptr as *const u32) };
                println!(
                    "[uefi-compatd] usb{} first register value: {:#x}",
                    idx, reg0
                );
            }
        } else {
            eprintln!(
                "[uefi-compatd] usb{} query failed (errno={})",
                idx,
                get_errno()
            );
        }
    }

    for idx in 0..counts.hid_input as usize {
        let mut descriptor = UefiHidInputDescriptor::default();
        if uefi_get_hid_input(idx, &mut descriptor) == 0 {
            let device_type = match descriptor.info.device_type {
                1 => "keyboard",
                2 => "mouse",
                3 => "combined",
                _ => "unknown",
            };
            println!(
                "[uefi-compatd] hid{} {} protocol={} usb={} vid={:#x} pid={:#x}",
                idx,
                device_type,
                descriptor.info.protocol,
                if descriptor.info.is_usb != 0 { "yes" } else { "no" },
                descriptor.info.vendor_id,
                descriptor.info.product_id
            );
            if descriptor.info.is_usb != 0 {
                println!(
                    "[uefi-compatd] hid{} USB device addr={} endpoint={}",
                    idx, descriptor.info.usb_device_addr, descriptor.info.usb_endpoint
                );
            }
        } else {
            eprintln!(
                "[uefi-compatd] hid{} query failed (errno={})",
                idx,
                get_errno()
            );
        }
    }

    println!("[uefi-compatd] initialisation complete");
}

