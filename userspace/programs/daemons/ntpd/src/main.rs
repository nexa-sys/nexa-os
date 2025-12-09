//! NTP (Network Time Protocol) Client for NexaOS
//!
//! This daemon synchronizes the system time with NTP servers.
//! It implements SNTPv4 (Simple Network Time Protocol version 4) as per RFC 4330.
//!
//! Usage:
//!   ntpd                    - Sync once with default NTP server (pool.ntp.org)
//!   ntpd -s <server>        - Sync once with specified server
//!   ntpd -d                 - Run as daemon, sync periodically
//!   ntpd -q                 - Query only, don't set time
//!   ntpd -v                 - Verbose output

use std::env;
use std::mem;
use std::process;
use std::net::UdpSocket;
use std::time::Duration;

// NTP Constants
const NTP_PORT: u16 = 123;
const NTP_PACKET_SIZE: usize = 48;
const NTP_TIMESTAMP_DELTA: u64 = 2208988800; // Seconds between 1900 and 1970

// NTP Leap Indicator (LI)
const LI_NO_WARNING: u8 = 0;

// NTP Version (VN) - Using version 4
const NTP_VERSION: u8 = 4;

// NTP Mode
const MODE_CLIENT: u8 = 3;
const MODE_SERVER: u8 = 4;

// Default NTP servers
const DEFAULT_NTP_SERVERS: &[&str] = &[
    "pool.ntp.org",
    "time.google.com",
    "time.cloudflare.com",
    "time.windows.com",
];

// Sync interval for daemon mode (seconds)
const SYNC_INTERVAL_SECS: u64 = 3600; // 1 hour

// Timeout for NTP queries
const NTP_TIMEOUT_SECS: u64 = 5;

// Retry configuration
const MAX_RETRIES: u32 = 3;           // Max retries per server
const RETRY_DELAY_SECS: u32 = 2;      // Delay between retries
const INITIAL_SYNC_TIMEOUT: u32 = 30; // Total timeout for initial sync attempt

// Maximum allowed offset before adjusting time (seconds)
const MAX_OFFSET_SECS: f64 = 1000.0;

// Syscall numbers
const SYS_CLOCK_SETTIME: u64 = 227;

// Clock IDs
const CLOCK_REALTIME: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
struct TimeSpec {
    tv_sec: i64,
    tv_nsec: i64,
}

/// NTP Packet structure (48 bytes)
/// 
/// Format (RFC 4330):
/// ```text
///  0                   1                   2                   3
///  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |LI | VN  |Mode |    Stratum    |     Poll      |   Precision   |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                          Root Delay                           |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                       Root Dispersion                         |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                    Reference Identifier                       |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   Reference Timestamp (64)                    |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   Originate Timestamp (64)                    |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                    Receive Timestamp (64)                     |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   Transmit Timestamp (64)                     |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
#[repr(C, packed)]
struct NtpPacket {
    /// LI (2 bits) | VN (3 bits) | Mode (3 bits)
    li_vn_mode: u8,
    /// Stratum level of the local clock
    stratum: u8,
    /// Maximum interval between successive messages
    poll: u8,
    /// Precision of the local clock
    precision: i8,
    /// Total round-trip delay to the primary reference source
    root_delay: u32,
    /// Maximum error relative to the primary reference source
    root_dispersion: u32,
    /// Reference identifier
    ref_id: u32,
    /// Reference timestamp
    ref_ts_sec: u32,
    ref_ts_frac: u32,
    /// Originate timestamp (time request sent by client)
    orig_ts_sec: u32,
    orig_ts_frac: u32,
    /// Receive timestamp (time request received by server)
    recv_ts_sec: u32,
    recv_ts_frac: u32,
    /// Transmit timestamp (time reply sent by server)
    trans_ts_sec: u32,
    trans_ts_frac: u32,
}

impl NtpPacket {
    /// Create a new NTP client request packet
    fn new_request() -> Self {
        let mut packet = Self {
            li_vn_mode: (LI_NO_WARNING << 6) | (NTP_VERSION << 3) | MODE_CLIENT,
            stratum: 0,
            poll: 0,
            precision: 0,
            root_delay: 0,
            root_dispersion: 0,
            ref_id: 0,
            ref_ts_sec: 0,
            ref_ts_frac: 0,
            orig_ts_sec: 0,
            orig_ts_frac: 0,
            recv_ts_sec: 0,
            recv_ts_frac: 0,
            trans_ts_sec: 0,
            trans_ts_frac: 0,
        };
        
        // Set transmit timestamp to current time
        let now = get_current_time_ntp();
        packet.trans_ts_sec = now.0.to_be();
        packet.trans_ts_frac = now.1.to_be();
        
        packet
    }
    
    /// Parse response and extract mode
    fn get_mode(&self) -> u8 {
        self.li_vn_mode & 0x07
    }
    
    /// Get stratum level
    fn get_stratum(&self) -> u8 {
        self.stratum
    }
    
    /// Get transmit timestamp as seconds since NTP epoch
    fn get_transmit_timestamp(&self) -> (u32, u32) {
        (u32::from_be(self.trans_ts_sec), u32::from_be(self.trans_ts_frac))
    }
    
    /// Get receive timestamp
    fn get_receive_timestamp(&self) -> (u32, u32) {
        (u32::from_be(self.recv_ts_sec), u32::from_be(self.recv_ts_frac))
    }
    
    /// Get originate timestamp
    fn get_originate_timestamp(&self) -> (u32, u32) {
        (u32::from_be(self.orig_ts_sec), u32::from_be(self.orig_ts_frac))
    }
}

/// Get current time as NTP timestamp (seconds since 1900, fraction)
fn get_current_time_ntp() -> (u32, u32) {
    // Get current Unix timestamp from system
    extern "C" {
        fn get_uptime() -> u64;
    }
    
    // For now, use uptime as a base (will be corrected after sync)
    // In a real implementation, we'd use clock_gettime(CLOCK_REALTIME)
    let uptime = unsafe { get_uptime() };
    
    // Convert to NTP timestamp (add delta from 1900 to 1970)
    let ntp_secs = uptime + NTP_TIMESTAMP_DELTA;
    
    (ntp_secs as u32, 0)
}

/// Convert NTP timestamp to Unix timestamp
fn ntp_to_unix(ntp_secs: u32, _ntp_frac: u32) -> i64 {
    (ntp_secs as i64) - (NTP_TIMESTAMP_DELTA as i64)
}

/// Set system time using clock_settime syscall
fn set_system_time(unix_secs: i64, nsecs: i64) -> Result<(), String> {
    let ts = TimeSpec {
        tv_sec: unix_secs,
        tv_nsec: nsecs,
    };
    
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_CLOCK_SETTIME,
            in("rdi") CLOCK_REALTIME,
            in("rsi") &ts as *const TimeSpec,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    
    if ret == 0 {
        Ok(())
    } else {
        Err(format!("clock_settime failed with error {}", ret))
    }
}

/// Resolve hostname to IP address using DNS
fn resolve_hostname(hostname: &str) -> Result<[u8; 4], String> {
    // Try to parse as IP address first
    let parts: Vec<&str> = hostname.split('.').collect();
    if parts.len() == 4 {
        let mut ip = [0u8; 4];
        let mut valid = true;
        for (i, part) in parts.iter().enumerate() {
            match part.parse::<u8>() {
                Ok(n) => ip[i] = n,
                Err(_) => {
                    valid = false;
                    break;
                }
            }
        }
        if valid {
            return Ok(ip);
        }
    }
    
    // Use DNS resolution via nslookup-style lookup
    // For simplicity, we'll use a hardcoded resolver or fall back to known IPs
    
    // Known NTP server IPs (fallback)
    match hostname {
        "pool.ntp.org" | "0.pool.ntp.org" => Ok([162, 159, 200, 1]), // time.cloudflare.com
        "time.google.com" => Ok([216, 239, 35, 0]),
        "time.cloudflare.com" => Ok([162, 159, 200, 1]),
        "time.windows.com" => Ok([52, 231, 114, 183]),
        "time.nist.gov" => Ok([129, 6, 15, 28]),
        _ => {
            // Try to resolve via system DNS
            // This would use gethostbyname or similar, but for now use fallback
            eprintln!("Warning: Cannot resolve '{}', using fallback server", hostname);
            Ok([162, 159, 200, 1]) // Cloudflare NTP
        }
    }
}

/// Query NTP server and calculate time offset
fn query_ntp_server(server_ip: [u8; 4], verbose: bool) -> Result<(i64, i64), String> {
    let server_addr = format!(
        "{}.{}.{}.{}:{}",
        server_ip[0], server_ip[1], server_ip[2], server_ip[3], NTP_PORT
    );
    
    if verbose {
        println!("Querying NTP server at {}...", server_addr);
    }
    
    // Create UDP socket
    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("Failed to create socket: {}", e))?;
    
    socket.set_read_timeout(Some(Duration::from_secs(NTP_TIMEOUT_SECS)))
        .map_err(|e| format!("Failed to set timeout: {}", e))?;
    
    // Create and send NTP request
    let request = NtpPacket::new_request();
    let request_bytes = unsafe {
        core::slice::from_raw_parts(
            &request as *const NtpPacket as *const u8,
            NTP_PACKET_SIZE
        )
    };
    
    // Record time of request (T1)
    let t1 = get_current_time_ntp();
    
    socket.send_to(request_bytes, &server_addr)
        .map_err(|e| format!("Failed to send request: {}", e))?;
    
    // Receive response
    let mut response_buf = [0u8; NTP_PACKET_SIZE];
    let (n, _) = socket.recv_from(&mut response_buf)
        .map_err(|e| format!("Failed to receive response: {}", e))?;
    
    // Record time of response (T4)
    let t4 = get_current_time_ntp();
    
    if n != NTP_PACKET_SIZE {
        return Err(format!("Invalid response size: {} bytes", n));
    }
    
    // Parse response
    let response: &NtpPacket = unsafe { &*(response_buf.as_ptr() as *const NtpPacket) };
    
    // Validate response
    if response.get_mode() != MODE_SERVER {
        return Err(format!("Invalid response mode: {}", response.get_mode()));
    }
    
    if response.get_stratum() == 0 {
        return Err("Server returned stratum 0 (kiss-of-death)".to_string());
    }
    
    // Get timestamps from response
    let t2 = response.get_receive_timestamp();  // Server receive time
    let t3 = response.get_transmit_timestamp(); // Server transmit time
    
    if verbose {
        println!("  Stratum: {}", response.get_stratum());
        println!("  T1 (client send):    {} s", t1.0);
        println!("  T2 (server receive): {} s", t2.0);
        println!("  T3 (server send):    {} s", t3.0);
        println!("  T4 (client receive): {} s", t4.0);
    }
    
    // Calculate offset and delay using NTP algorithm
    // offset = ((T2 - T1) + (T3 - T4)) / 2
    // delay = (T4 - T1) - (T3 - T2)
    
    let t1_secs = t1.0 as i64;
    let t2_secs = t2.0 as i64;
    let t3_secs = t3.0 as i64;
    let t4_secs = t4.0 as i64;
    
    let offset_secs = ((t2_secs - t1_secs) + (t3_secs - t4_secs)) / 2;
    let delay_secs = (t4_secs - t1_secs) - (t3_secs - t2_secs);
    
    if verbose {
        println!("  Offset: {} seconds", offset_secs);
        println!("  Round-trip delay: {} seconds", delay_secs);
    }
    
    // Calculate correct Unix timestamp
    let server_time_unix = ntp_to_unix(t3.0, t3.1);
    
    Ok((server_time_unix, offset_secs))
}

/// Synchronize system time with NTP server
fn sync_time(server: &str, verbose: bool, query_only: bool) -> Result<(), String> {
    println!("NTP sync: using server '{}'", server);
    
    // Resolve server hostname
    let server_ip = resolve_hostname(server)?;
    
    if verbose {
        println!("Resolved to IP: {}.{}.{}.{}", 
                 server_ip[0], server_ip[1], server_ip[2], server_ip[3]);
    }
    
    // Query NTP server
    let (server_time, offset) = query_ntp_server(server_ip, verbose)?;
    
    println!("Server time: {} (Unix timestamp)", server_time);
    println!("Time offset: {} seconds", offset);
    
    if query_only {
        println!("Query only mode - not setting system time");
        return Ok(());
    }
    
    // Check if offset is reasonable
    if (offset as f64).abs() > MAX_OFFSET_SECS {
        println!("Warning: Large time offset detected ({} seconds)", offset);
    }
    
    // Set system time
    println!("Setting system time...");
    set_system_time(server_time, 0)?;
    
    println!("System time synchronized successfully!");
    
    // Verify the time was set
    if verbose {
        extern "C" {
            fn get_uptime() -> u64;
        }
        let new_time = unsafe { get_uptime() };
        println!("New system uptime: {} seconds", new_time);
    }
    
    Ok(())
}

/// Attempt to sync time with retries
fn sync_time_with_retry(server: &str, verbose: bool, query_only: bool) -> Result<(), String> {
    extern "C" {
        fn sleep(seconds: u32);
    }
    
    for attempt in 1..=MAX_RETRIES {
        if verbose {
            println!("NTP sync attempt {}/{}", attempt, MAX_RETRIES);
        }
        
        match sync_time(server, verbose, query_only) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if attempt < MAX_RETRIES {
                    eprintln!("Attempt {} failed: {}, retrying in {}s...", attempt, e, RETRY_DELAY_SECS);
                    unsafe { sleep(RETRY_DELAY_SECS) };
                } else {
                    return Err(format!("All {} attempts failed. Last error: {}", MAX_RETRIES, e));
                }
            }
        }
    }
    
    Err("Max retries exceeded".to_string())
}

/// Try multiple NTP servers until one succeeds
fn sync_with_fallback_servers(servers: &[&str], verbose: bool, query_only: bool) -> Result<(), String> {
    let mut last_error = String::new();
    
    for server in servers {
        match sync_time_with_retry(server, verbose, query_only) {
            Ok(()) => return Ok(()),
            Err(e) => {
                eprintln!("Server '{}' failed: {}", server, e);
                last_error = e;
            }
        }
    }
    
    Err(format!("All servers failed. Last error: {}", last_error))
}

/// Run as daemon, syncing periodically
fn run_daemon(server: &str, verbose: bool) {
    println!("NTP daemon starting...");
    println!("Sync interval: {} seconds", SYNC_INTERVAL_SECS);
    
    extern "C" {
        fn sleep(seconds: u32);
    }
    
    // Initial sync with fallback to other servers if primary fails
    let mut synced = false;
    
    // First try the specified server with retries
    match sync_time_with_retry(server, verbose, false) {
        Ok(()) => {
            println!("Initial sync successful");
            synced = true;
        }
        Err(e) => {
            eprintln!("Primary server failed: {}", e);
            // Try fallback servers
            println!("Trying fallback NTP servers...");
            match sync_with_fallback_servers(DEFAULT_NTP_SERVERS, verbose, false) {
                Ok(()) => {
                    println!("Sync successful with fallback server");
                    synced = true;
                }
                Err(e) => {
                    eprintln!("All fallback servers failed: {}", e);
                    eprintln!("Will continue trying in background...");
                }
            }
        }
    }
    
    if !synced {
        println!("Initial sync failed, will retry periodically");
    }
    
    // Main daemon loop - continue even if initial sync failed
    loop {
        println!("Next sync in {} seconds...", SYNC_INTERVAL_SECS);
        unsafe { sleep(SYNC_INTERVAL_SECS as u32) };
        
        match sync_time_with_retry(server, verbose, false) {
            Ok(()) => println!("Periodic sync successful"),
            Err(e) => eprintln!("Periodic sync failed: {}", e),
        }
    }
}

fn print_usage() {
    println!("NTP Client for NexaOS");
    println!();
    println!("Usage: ntpd [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -s <server>   Use specified NTP server (default: pool.ntp.org)");
    println!("  -d            Run as daemon (sync periodically)");
    println!("  -q            Query only, don't set system time");
    println!("  -v            Verbose output");
    println!("  -h            Show this help message");
    println!();
    println!("Examples:");
    println!("  ntpd                      Sync time with default server");
    println!("  ntpd -s time.google.com   Sync with Google's NTP server");
    println!("  ntpd -d                   Run as daemon");
    println!("  ntpd -q -v                Query time without setting (verbose)");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let mut server = DEFAULT_NTP_SERVERS[0].to_string();
    let mut daemon_mode = false;
    let mut query_only = false;
    let mut verbose = false;
    
    // Parse arguments
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-s" => {
                if i + 1 < args.len() {
                    server = args[i + 1].clone();
                    i += 1;
                } else {
                    eprintln!("Error: -s requires a server argument");
                    process::exit(1);
                }
            }
            "-d" => daemon_mode = true,
            "-q" => query_only = true,
            "-v" => verbose = true,
            "-h" | "--help" => {
                print_usage();
                return;
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                print_usage();
                process::exit(1);
            }
        }
        i += 1;
    }
    
    if verbose {
        println!("NTP Client for NexaOS");
        println!("Server: {}", server);
        println!("Mode: {}", if daemon_mode { "daemon" } else { "one-shot" });
    }
    
    if daemon_mode {
        run_daemon(&server, verbose);
    } else {
        // One-shot mode: use retry mechanism
        match sync_time_with_retry(&server, verbose, query_only) {
            Ok(()) => {
                if !query_only {
                    println!("Time synchronization complete.");
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                process::exit(1);
            }
        }
    }
}
