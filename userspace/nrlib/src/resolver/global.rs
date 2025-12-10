/// Global resolver instance and initialization
///
/// Manages the global resolver state and provides initialization.
use core::sync::atomic::{AtomicBool, Ordering};

use crate::get_system_dns_servers;

use super::constants::KERNEL_DNS_QUERY_CAP;
use super::core::Resolver;
use super::utils::read_file_content;

/// Global resolver instance
static mut GLOBAL_RESOLVER: Option<Resolver> = None;
static RESOLVER_INIT: AtomicBool = AtomicBool::new(false);

/// Initialize global resolver (call once at program startup)
#[no_mangle]
pub extern "C" fn resolver_init() -> i32 {
    if RESOLVER_INIT.load(Ordering::Acquire) {
        return 0; // Already initialized
    }

    let mut resolver = Resolver::new();

    // Try to load /etc/resolv.conf
    let mut resolv_buf = [0u8; 2048];
    if let Some(len) = read_file_content("/etc/resolv.conf", &mut resolv_buf) {
        if let Ok(content) = core::str::from_utf8(&resolv_buf[..len]) {
            resolver.parse_resolv_conf(content);
        }
    }

    if resolver.config.nameserver_count == 0 {
        let mut kernel_dns = [0u32; KERNEL_DNS_QUERY_CAP];
        if let Ok(count) = get_system_dns_servers(&mut kernel_dns) {
            for idx in 0..count {
                let ip_bytes = kernel_dns[idx].to_be_bytes();
                let _ = resolver.config.add_nameserver(ip_bytes);
            }
        }
    }

    if resolver.config.nameserver_count == 0 {
        let _ = resolver.config.add_nameserver([10, 0, 2, 3]);
        let _ = resolver.config.add_nameserver([8, 8, 8, 8]);
    }

    // Try to load /etc/hosts
    let mut hosts_buf = [0u8; 4096];
    if let Some(len) = read_file_content("/etc/hosts", &mut hosts_buf) {
        if let Ok(content) = core::str::from_utf8(&hosts_buf[..len]) {
            resolver.parse_hosts(content);
        }
    }

    // Try to load /etc/nsswitch.conf
    let mut nsswitch_buf = [0u8; 1024];
    if let Some(len) = read_file_content("/etc/nsswitch.conf", &mut nsswitch_buf) {
        if let Ok(content) = core::str::from_utf8(&nsswitch_buf[..len]) {
            resolver.parse_nsswitch(content);
        }
    }

    unsafe {
        GLOBAL_RESOLVER = Some(resolver);
    }

    RESOLVER_INIT.store(true, Ordering::Release);
    0
}

/// Get the global resolver instance
pub fn get_resolver() -> Option<&'static Resolver> {
    if !RESOLVER_INIT.load(Ordering::Acquire) {
        let _ = resolver_init();
    }
    unsafe { GLOBAL_RESOLVER.as_ref() }
}
