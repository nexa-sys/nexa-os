//! mkswap - Set up a Linux swap area
//!
//! Usage:
//!   mkswap [OPTIONS] DEVICE [SIZE]
//!
//! Sets up a Linux swap area on a device or in a file.

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::process;

// Linux swap header constants
const PAGE_SIZE: usize = 4096;
const SWAP_MAGIC: &[u8] = b"SWAPSPACE2";
const SWAP_MAGIC_OFFSET: usize = PAGE_SIZE - 10; // 4086

// swap_header structure offsets (version 1 format)
const VERSION_OFFSET: usize = 1024;
const LAST_PAGE_OFFSET: usize = 1028;
const NR_BADPAGES_OFFSET: usize = 1032;
const UUID_OFFSET: usize = 1036;
const VOLUME_NAME_OFFSET: usize = 1052;
const PADDING_OFFSET: usize = 1068;

fn print_usage() {
    eprintln!("mkswap - Set up a Linux swap area");
    eprintln!();
    eprintln!("Usage: mkswap [OPTIONS] DEVICE [SIZE]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -L, --label NAME    Specify a label");
    eprintln!("  -U, --uuid UUID     Specify UUID (or 'random')");
    eprintln!("  -f, --force         Force, even on a mounted device");
    eprintln!("  -c, --check         Check for bad blocks before creating");
    eprintln!("  -p, --pagesize N    Override page size (default: 4096)");
    eprintln!("  -v, --verbose       Be verbose");
    eprintln!("  -h, --help          Show this help message");
    eprintln!();
    eprintln!("SIZE can be specified with suffix K, M, or G");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  mkswap /dev/vdb              Create swap on entire device");
    eprintln!("  mkswap -L myswap /dev/vdb    Create with label");
    eprintln!("  mkswap /swapfile 256M        Create 256MB swap file");
}

fn generate_uuid() -> [u8; 16] {
    // Simple pseudo-random UUID generation
    // In a real implementation, would use proper random source
    let mut uuid = [0u8; 16];

    // Use simple counter + device path hash as seed
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x12345678);

    let mut state = seed;
    for byte in &mut uuid {
        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        *byte = (state >> 16) as u8;
    }

    // Set version (4) and variant (RFC 4122)
    uuid[6] = (uuid[6] & 0x0f) | 0x40;
    uuid[8] = (uuid[8] & 0x3f) | 0x80;

    uuid
}

fn format_uuid(uuid: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        uuid[0], uuid[1], uuid[2], uuid[3],
        uuid[4], uuid[5],
        uuid[6], uuid[7],
        uuid[8], uuid[9],
        uuid[10], uuid[11], uuid[12], uuid[13], uuid[14], uuid[15]
    )
}

fn parse_size(size_str: &str) -> Option<u64> {
    let size_str = size_str.trim();

    let (num_str, multiplier) = if size_str.ends_with('K') || size_str.ends_with('k') {
        (&size_str[..size_str.len() - 1], 1024u64)
    } else if size_str.ends_with('M') || size_str.ends_with('m') {
        (&size_str[..size_str.len() - 1], 1024 * 1024)
    } else if size_str.ends_with('G') || size_str.ends_with('g') {
        (&size_str[..size_str.len() - 1], 1024 * 1024 * 1024)
    } else {
        (size_str, 1)
    };

    num_str.parse::<u64>().ok().map(|n| n * multiplier)
}

fn do_mkswap(
    path: &str,
    label: Option<&str>,
    uuid: Option<[u8; 16]>,
    size: Option<u64>,
    verbose: bool,
) -> i32 {
    // Open device/file
    let mut file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(size.is_some())
        .open(path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("mkswap: {}: {}", path, e);
            return 1;
        }
    };

    // Determine size
    let total_size = match size {
        Some(s) => {
            // Extend/truncate file to specified size
            if let Err(e) = file.set_len(s) {
                eprintln!("mkswap: failed to set file size: {}", e);
                return 1;
            }
            s
        }
        None => {
            // Get device/file size
            match file.seek(SeekFrom::End(0)) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("mkswap: failed to get size: {}", e);
                    return 1;
                }
            }
        }
    };

    if total_size < 2 * PAGE_SIZE as u64 {
        eprintln!(
            "mkswap: {}: file/device too small (minimum {} bytes)",
            path,
            2 * PAGE_SIZE
        );
        return 1;
    }

    // Calculate number of pages
    let total_pages = total_size / PAGE_SIZE as u64;
    let last_page = total_pages - 1; // last usable page (0-indexed)

    if verbose {
        println!("Device: {}", path);
        println!("Size: {} bytes ({} pages)", total_size, total_pages);
    }

    // Generate UUID if not provided
    let uuid = uuid.unwrap_or_else(generate_uuid);

    // Create header page
    let mut header = vec![0u8; PAGE_SIZE];

    // Version (1)
    header[VERSION_OFFSET] = 1;
    header[VERSION_OFFSET + 1] = 0;
    header[VERSION_OFFSET + 2] = 0;
    header[VERSION_OFFSET + 3] = 0;

    // Last page (little-endian 32-bit)
    let last_page_u32 = last_page as u32;
    header[LAST_PAGE_OFFSET] = (last_page_u32 & 0xFF) as u8;
    header[LAST_PAGE_OFFSET + 1] = ((last_page_u32 >> 8) & 0xFF) as u8;
    header[LAST_PAGE_OFFSET + 2] = ((last_page_u32 >> 16) & 0xFF) as u8;
    header[LAST_PAGE_OFFSET + 3] = ((last_page_u32 >> 24) & 0xFF) as u8;

    // Number of bad pages (0)
    header[NR_BADPAGES_OFFSET..NR_BADPAGES_OFFSET + 4].copy_from_slice(&[0, 0, 0, 0]);

    // UUID
    header[UUID_OFFSET..UUID_OFFSET + 16].copy_from_slice(&uuid);

    // Volume name/label
    if let Some(label) = label {
        let label_bytes = label.as_bytes();
        let copy_len = label_bytes.len().min(16);
        header[VOLUME_NAME_OFFSET..VOLUME_NAME_OFFSET + copy_len]
            .copy_from_slice(&label_bytes[..copy_len]);
    }

    // Magic signature at end of page
    header[SWAP_MAGIC_OFFSET..SWAP_MAGIC_OFFSET + SWAP_MAGIC.len()].copy_from_slice(SWAP_MAGIC);

    // Write header
    if let Err(e) = file.seek(SeekFrom::Start(0)) {
        eprintln!("mkswap: seek failed: {}", e);
        return 1;
    }

    if let Err(e) = file.write_all(&header) {
        eprintln!("mkswap: write failed: {}", e);
        return 1;
    }

    // Sync to disk
    if let Err(e) = file.sync_all() {
        eprintln!("mkswap: sync failed: {}", e);
        return 1;
    }

    // Print results
    println!(
        "Setting up swapspace version 1, size = {} bytes ({} pages)",
        total_size - PAGE_SIZE as u64,
        last_page
    );

    if let Some(label) = label {
        println!("LABEL={}", label);
    }

    println!("UUID={}", format_uuid(&uuid));

    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut device: Option<String> = None;
    let mut size: Option<u64> = None;
    let mut label: Option<String> = None;
    let mut uuid: Option<[u8; 16]> = None;
    let mut verbose = false;
    let mut force = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            "-f" | "--force" => {
                force = true;
            }
            "-L" | "--label" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkswap: --label requires an argument");
                    process::exit(1);
                }
                label = Some(args[i].clone());
            }
            "-U" | "--uuid" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("mkswap: --uuid requires an argument");
                    process::exit(1);
                }
                if args[i] == "random" {
                    uuid = Some(generate_uuid());
                } else {
                    // Parse UUID string
                    // Format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
                    let uuid_str = args[i].replace("-", "");
                    if uuid_str.len() != 32 {
                        eprintln!("mkswap: invalid UUID: {}", args[i]);
                        process::exit(1);
                    }
                    let mut bytes = [0u8; 16];
                    for (j, chunk) in uuid_str.as_bytes().chunks(2).enumerate() {
                        let hex_str = std::str::from_utf8(chunk).unwrap_or("00");
                        bytes[j] = u8::from_str_radix(hex_str, 16).unwrap_or(0);
                    }
                    uuid = Some(bytes);
                }
            }
            "-c" | "--check" => {
                // Bad block check not implemented
                eprintln!("mkswap: warning: bad block check not implemented");
            }
            "-p" | "--pagesize" => {
                i += 1;
                // Page size override not implemented
                eprintln!("mkswap: warning: custom page size not supported, using 4096");
            }
            arg if arg.starts_with('-') => {
                eprintln!("mkswap: unknown option: {}", arg);
                print_usage();
                process::exit(1);
            }
            arg => {
                if device.is_none() {
                    device = Some(arg.to_string());
                } else if size.is_none() {
                    size = parse_size(arg);
                    if size.is_none() {
                        eprintln!("mkswap: invalid size: {}", arg);
                        process::exit(1);
                    }
                } else {
                    eprintln!("mkswap: extra argument: {}", arg);
                    print_usage();
                    process::exit(1);
                }
            }
        }
        i += 1;
    }

    // Need a device
    let device = match device {
        Some(d) => d,
        None => {
            eprintln!("mkswap: no device specified");
            print_usage();
            process::exit(1);
        }
    };

    process::exit(do_mkswap(&device, label.as_deref(), uuid, size, verbose));
}
