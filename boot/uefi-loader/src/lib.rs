#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use core::mem;
use core::ptr;

use nexa_boot_info::{flags, BootInfo, FramebufferInfo, MemoryRegion};
use uefi::prelude::*;
use uefi::proto::console::gop::{GraphicsOutput, PixelFormat};
use uefi::proto::media::file::{Directory, File, FileAttribute, FileInfo, FileMode, RegularFile};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::table::boot::{AllocateType, MemoryDescriptor, MemoryType};
use uefi::{cstr16, Handle, Status};

uefi_services::allocator!();
uefi_services::panic_handler!();

const KERNEL_PATH: &uefi::CStr16 = cstr16!("\\EFI\\NEXAOS\\KERNEL.ELF");
const INITRAMFS_PATH: &uefi::CStr16 = cstr16!("\\EFI\\NEXAOS\\INITRAMFS.CPIO");
const ROOTFS_PATH: &uefi::CStr16 = cstr16!("\\EFI\\NEXAOS\\ROOTFS.EXT2");
const MAX_PHYS_ADDR: u64 = 0x0000FFFF_FFFF;

#[entry]
fn efi_main(image: Handle, mut st: SystemTable<Boot>) -> Status {
    if let Err(e) = uefi_services::init(&mut st) {
        return e;
    }

    log::info!("NexaOS UEFI loader starting");

    let bs = st.boot_services();

    let mut root = match open_boot_volume(bs, image) {
        Ok(dir) => dir,
        Err(status) => return status,
    };

    let kernel_bytes = match read_file(&mut root, KERNEL_PATH) {
        Ok(data) => data,
        Err(status) => {
            log::error!("Failed to load kernel image: {:?}", status);
            return status;
        }
    };

    let initramfs_bytes = match read_file(&mut root, INITRAMFS_PATH) {
        Ok(data) => data,
        Err(status) if status == Status::NOT_FOUND => Vec::new(),
        Err(status) => {
            log::error!("Failed to load initramfs: {:?}", status);
            return status;
        }
    };

    let rootfs_bytes = match read_file(&mut root, ROOTFS_PATH) {
        Ok(data) => data,
        Err(status) if status == Status::NOT_FOUND => Vec::new(),
        Err(status) => {
            log::error!("Failed to load rootfs image: {:?}", status);
            return status;
        }
    };

    drop(root);

    let loaded = match load_kernel_image(bs, &kernel_bytes) {
        Ok(info) => info,
        Err(status) => {
            log::error!("Kernel load failed: {:?}", status);
            return status;
        }
    };

    let initramfs_region = match stage_payload(bs, &initramfs_bytes, MemoryType::LOADER_DATA) {
        Ok(region) => region,
        Err(status) => {
            log::error!("Failed to allocate initramfs region: {:?}", status);
            return status;
        }
    };

    let rootfs_region = match stage_payload(bs, &rootfs_bytes, MemoryType::LOADER_DATA) {
        Ok(region) => region,
        Err(status) => {
            log::error!("Failed to allocate rootfs region: {:?}", status);
            return status;
        }
    };

    let framebuffer = detect_framebuffer(bs);

    let boot_info_region = match stage_boot_info(bs, initramfs_region, rootfs_region, framebuffer) {
        Ok(region) => region,
        Err(status) => {
            log::error!("Failed to allocate boot info block: {:?}", status);
            return status;
        }
    };

    let (_, map_key) = match prepare_memory_map(bs) {
        Ok(pair) => pair,
        Err(status) => {
            log::error!("Failed to fetch memory map: {:?}", status);
            return status;
        }
    };

    if let Err(status) = st.exit_boot_services(image, map_key) {
        log::error!("ExitBootServices failed: {:?}", status);
        return status;
    }

    log::info!(
        "Transferring control to kernel UEFI entry at {:#x}",
        loaded.uefi_entry_point
    );

    unsafe {
        let entry: extern "C" fn(*const BootInfo) -> ! = mem::transmute(loaded.uefi_entry_point);
        entry(boot_info_region.phys_addr as *const BootInfo)
    }
}

struct LoadedKernel {
    entry_point: u64,
    uefi_entry_point: u64,
}

fn open_boot_volume(bs: &BootServices, image: Handle) -> Result<Directory, Status> {
    let fs = unsafe { bs.get_image_file_system(image)? };
    let mut file_system = unsafe { &mut *fs.get() };
    file_system.open_volume()
}

fn read_file(root: &mut Directory, path: &uefi::CStr16) -> Result<Vec<u8>, Status> {
    let file = match root.open(path, FileMode::Read, FileAttribute::empty())? {
        File::Regular(f) => f,
        _ => return Err(Status::UNSUPPORTED),
    };
    read_entire_file(file)
}

fn read_entire_file(mut file: RegularFile) -> Result<Vec<u8>, Status> {
    let info: FileInfo = file.get_info()?;
    let size = info.file_size() as usize;
    let mut buffer = Vec::with_capacity(size);
    unsafe {
        buffer.set_len(size);
    }
    let read = file.read(&mut buffer)?;
    buffer.truncate(read);
    Ok(buffer)
}

fn load_kernel_image(bs: &BootServices, image: &[u8]) -> Result<LoadedKernel, Status> {
    if image.len() < mem::size_of::<Elf64Ehdr>() {
        return Err(Status::LOAD_ERROR);
    }

    let header = unsafe { &*(image.as_ptr() as *const Elf64Ehdr) };

    if &header.e_ident[0..4] != b"\x7FELF" {
        return Err(Status::LOAD_ERROR);
    }
    if header.e_ident[4] != 2 || header.e_ident[5] != 1 {
        return Err(Status::LOAD_ERROR);
    }

    let phoff = header.e_phoff as usize;
    let phentsize = header.e_phentsize as usize;
    let phnum = header.e_phnum as usize;

    for i in 0..phnum {
        let offset = phoff + i * phentsize;
        if offset + mem::size_of::<Elf64Phdr>() > image.len() {
            return Err(Status::LOAD_ERROR);
        }
        let ph = unsafe { &*(image.as_ptr().add(offset) as *const Elf64Phdr) };
        if ph.p_type != 1 {
            continue;
        }

        let dest = ph.p_paddr as usize;
        let memsz = ph.p_memsz as usize;
        let filesz = ph.p_filesz as usize;
        if memsz == 0 {
            continue;
        }

        let pages = (memsz + 0xFFF) / 0x1000;
        unsafe {
            bs.allocate_pages(AllocateType::Address, MemoryType::LOADER_DATA, pages, dest)?;
        }

        if filesz > 0 {
            let src_offset = ph.p_offset as usize;
            if src_offset + filesz > image.len() {
                return Err(Status::LOAD_ERROR);
            }
            unsafe {
                ptr::copy_nonoverlapping(
                    image.as_ptr().add(src_offset),
                    dest as *mut u8,
                    filesz,
                );
            }
        }

        if memsz > filesz {
            unsafe {
                ptr::write_bytes((dest + filesz) as *mut u8, 0, memsz - filesz);
            }
        }
    }

    let uefi_entry_point = match find_uefi_entry(image) {
        Some(ptr) => ptr,
        None => {
            log::error!("Kernel image missing .nexa.uefi_entry section");
            return Err(Status::LOAD_ERROR);
        }
    };

    Ok(LoadedKernel {
        entry_point: header.e_entry,
        uefi_entry_point,
    })
}

fn stage_payload(bs: &BootServices, data: &[u8], mem_type: MemoryType) -> Result<MemoryRegion, Status> {
    if data.is_empty() {
        return Ok(MemoryRegion::empty());
    }

    let pages = (data.len() + 0xFFF) / 0x1000;
    let addr = unsafe { bs.allocate_pages(AllocateType::MaxAddress(MAX_PHYS_ADDR), mem_type, pages)? };
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), addr as *mut u8, data.len());
        let total = pages * 0x1000;
        if total > data.len() {
            ptr::write_bytes((addr as usize + data.len()) as *mut u8, 0, total - data.len());
        }
    }
    Ok(MemoryRegion {
        phys_addr: addr as u64,
        length: data.len() as u64,
    })
}

fn stage_boot_info(
    bs: &BootServices,
    initramfs: MemoryRegion,
    rootfs: MemoryRegion,
    framebuffer: Option<FramebufferInfo>,
) -> Result<MemoryRegion, Status> {
    let pages = 1;
    let addr = unsafe { bs.allocate_pages(AllocateType::MaxAddress(MAX_PHYS_ADDR), MemoryType::LOADER_DATA, pages)? };
    let boot_info = BootInfo {
        signature: nexa_boot_info::BOOT_INFO_SIGNATURE,
        version: nexa_boot_info::BOOT_INFO_VERSION,
        size: mem::size_of::<BootInfo>() as u16,
        flags: determine_flags(&initramfs, &rootfs, framebuffer.is_some()),
        initramfs,
        rootfs,
        cmdline: MemoryRegion::empty(),
        framebuffer: framebuffer.unwrap_or(FramebufferInfo {
            address: 0,
            pitch: 0,
            width: 0,
            height: 0,
            bpp: 0,
            red_position: 0,
            red_size: 0,
            green_position: 0,
            green_size: 0,
            blue_position: 0,
            blue_size: 0,
            reserved: [0; 5],
        }),
        reserved: [0; 32],
    };

    unsafe {
        ptr::write(addr as *mut BootInfo, boot_info);
    }

    Ok(MemoryRegion {
        phys_addr: addr as u64,
        length: mem::size_of::<BootInfo>() as u64,
    })
}

fn determine_flags(initramfs: &MemoryRegion, rootfs: &MemoryRegion, has_fb: bool) -> u32 {
    let mut flags_val = 0u32;
    if !initramfs.is_empty() {
        flags_val |= flags::HAS_INITRAMFS;
    }
    if !rootfs.is_empty() {
        flags_val |= flags::HAS_ROOTFS;
    }
    if has_fb {
        flags_val |= flags::HAS_FRAMEBUFFER;
    }
    flags_val
}

fn detect_framebuffer(bs: &BootServices) -> Option<FramebufferInfo> {
    let gop = unsafe { bs.locate_protocol::<GraphicsOutput>().ok()? };
    let gop = unsafe { &mut *gop.get() };
    let mode = gop.current_mode_info();
    let fb = gop.frame_buffer();

    let (bytes_per_pixel, bpp, red_position, red_size, green_position, green_size, blue_position, blue_size) =
        match mode.pixel_format() {
            PixelFormat::Rgb => (4, 32, 0, 8, 8, 8, 16, 8),
            PixelFormat::Bgr => (4, 32, 16, 8, 8, 8, 0, 8),
            PixelFormat::Bitmask { red, green, blue, .. } => {
                let red_position = red.trailing_zeros() as u8;
                let green_position = green.trailing_zeros() as u8;
                let blue_position = blue.trailing_zeros() as u8;
                let red_size = red.count_ones() as u8;
                let green_size = green.count_ones() as u8;
                let blue_size = blue.count_ones() as u8;
                let bpp = (red_size as u16 + green_size as u16 + blue_size as u16).min(32) as u8;
                (4, bpp, red_position, red_size, green_position, green_size, blue_position, blue_size)
            }
            PixelFormat::BltOnly => return None,
        };

    Some(FramebufferInfo {
        address: fb.as_mut_ptr() as u64,
        pitch: mode.stride() as u32 * bytes_per_pixel as u32,
        width: mode.resolution().0 as u32,
        height: mode.resolution().1 as u32,
        bpp,
        red_position,
        red_size,
        green_position,
        green_size,
        blue_position,
        blue_size,
        reserved: [0; 5],
    })
}

fn prepare_memory_map(bs: &BootServices) -> Result<(Vec<MemoryDescriptor>, usize), Status> {
    let map_size = bs.memory_map_size();
    let buffer_size = map_size.map_size + map_size.entry_size * 8;
    let mut buffer = Vec::with_capacity(buffer_size);
    unsafe {
        buffer.set_len(buffer_size);
    }
    let (map_key, descriptors) = bs.memory_map(&mut buffer)?;
    Ok((descriptors.to_vec(), map_key))
}

fn find_uefi_entry(image: &[u8]) -> Option<u64> {
    if image.len() < mem::size_of::<Elf64Ehdr>() {
        return None;
    }

    let header = unsafe { &*(image.as_ptr() as *const Elf64Ehdr) };
    let shoff = header.e_shoff as usize;
    let shentsize = header.e_shentsize as usize;
    let shnum = header.e_shnum as usize;
    let shstrndx = header.e_shstrndx as usize;

    if shoff == 0 || shentsize == 0 || shnum == 0 {
        return None;
    }
    if shoff + shentsize.saturating_mul(shnum) > image.len() {
        return None;
    }
    if shstrndx >= shnum {
        return None;
    }

    let section = |idx: usize| -> &Elf64Shdr {
        let offset = shoff + idx * shentsize;
        unsafe { &*(image.as_ptr().add(offset) as *const Elf64Shdr) }
    };

    let shstr = section(shstrndx);
    let str_offset = shstr.sh_offset as usize;
    let str_size = shstr.sh_size as usize;
    if str_offset.saturating_add(str_size) > image.len() {
        return None;
    }
    let strtab = &image[str_offset..str_offset + str_size];

    for idx in 0..shnum {
        let sh = section(idx);
        let name_offset = sh.sh_name as usize;
        if name_offset >= strtab.len() {
            continue;
        }
        let name = read_cstr(&strtab[name_offset..]);
        if name == ".nexa.uefi_entry" {
            let off = sh.sh_offset as usize;
            let size = sh.sh_size as usize;
            if off.saturating_add(size) > image.len() || size < 8 {
                return None;
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&image[off..off + 8]);
            return Some(u64::from_le_bytes(bytes));
        }
    }

    None
}

fn read_cstr(data: &[u8]) -> &str {
    let nul = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    unsafe { core::str::from_utf8_unchecked(&data[..nul]) }
}

#[repr(C)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

#[repr(C)]
struct Elf64Shdr {
    sh_name: u32,
    sh_type: u32,
    sh_flags: u64,
    sh_addr: u64,
    sh_offset: u64,
    sh_size: u64,
    sh_link: u32,
    sh_info: u32,
    sh_addralign: u64,
    sh_entsize: u64,
}
