// UEFI compatibility bridge daemon
//
// This process queries kernel-provided syscalls to expose framebuffer,
// network, and block device information preserved from UEFI boot services.

use core::ptr;
use nrlib::{
    get_errno, uefi_get_block, uefi_get_counts, uefi_get_framebuffer, uefi_get_network,
    uefi_map_network_mmio, UefiBlockDescriptor, UefiCompatCounts, UefiNetworkDescriptor,
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
        "[uefi-compatd] framebuffer={}, network={}, block={}",
        counts.framebuffer, counts.network, counts.block
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

    println!("[uefi-compatd] initialisation complete");
}

