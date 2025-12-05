# NTP (Network Time Protocol) Support

NexaOS supports NTP time synchronization through the `ntpd` client daemon.

## Overview

The NTP implementation consists of:
1. **Kernel syscall**: `clock_settime` (syscall #227) to set system time
2. **System time management**: Kernel maintains time offset from boot time
3. **User-space client**: `ntpd` daemon for NTP protocol communication

## Architecture

### Kernel Components

#### System Time Management (`src/syscalls/time.rs`)

The kernel maintains system time using a boot-relative approach:
- **Boot time**: Measured via TSC (Time Stamp Counter) since boot
- **Time offset**: Stored in `TIME_OFFSET_US` atomic variable
- **Real time**: Calculated as `boot_time + offset`

```
CLOCK_REALTIME = boot_time_us + TIME_OFFSET_US
CLOCK_MONOTONIC = boot_time_us (unaffected by time changes)
```

#### System Calls

| Syscall | Number | Description |
|---------|--------|-------------|
| `clock_gettime` | 228 | Get current time from specified clock |
| `clock_settime` | 227 | Set time for CLOCK_REALTIME |

### User-space Components

#### ntpd Client (`userspace/programs/ntpd/`)

SNTPv4 client implementing RFC 4330:
- Queries NTP servers on UDP port 123
- Calculates time offset using NTP algorithm
- Sets system time via `clock_settime` syscall

## Usage

### One-shot Sync

```bash
# Sync with default server (pool.ntp.org)
ntpd

# Sync with specific server
ntpd -s time.google.com

# Query only (don't set time)
ntpd -q

# Verbose output
ntpd -v
```

### Daemon Mode

```bash
# Run as daemon (sync every hour)
ntpd -d

# Daemon with specific server
ntpd -d -s time.cloudflare.com
```

### Command-line Options

| Option | Description |
|--------|-------------|
| `-s <server>` | Use specified NTP server |
| `-d` | Run as daemon (periodic sync) |
| `-q` | Query only, don't set time |
| `-v` | Verbose output |
| `-h` | Show help message |

## NTP Protocol

### Packet Format

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|LI | VN  |Mode |    Stratum    |     Poll      |   Precision   |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                          Root Delay                           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       Root Dispersion                         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    Reference Identifier                       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                   Reference Timestamp (64)                    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                   Originate Timestamp (64)                    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    Receive Timestamp (64)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                   Transmit Timestamp (64)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### Time Calculation

Using timestamps:
- T1: Client transmit time
- T2: Server receive time
- T3: Server transmit time  
- T4: Client receive time

```
offset = ((T2 - T1) + (T3 - T4)) / 2
delay = (T4 - T1) - (T3 - T2)
```

## Default NTP Servers

| Server | Description |
|--------|-------------|
| pool.ntp.org | NTP Pool Project |
| time.google.com | Google Public NTP |
| time.cloudflare.com | Cloudflare NTP |
| time.windows.com | Microsoft NTP |

## Implementation Notes

### Timestamp Handling

NTP timestamps are 64-bit values:
- Seconds since 1900-01-01 (32 bits)
- Fraction of second (32 bits)

Conversion to Unix timestamp:
```
unix_timestamp = ntp_seconds - 2208988800
```

### Security Considerations

- NTP is unencrypted; consider network security
- Large time jumps are logged for monitoring
- Stratum 0 (kiss-of-death) packets are rejected

### Limitations

Current implementation:
- SNTPv4 only (no NTPv4 algorithms)
- No authentication support
- No leap second handling
- Single-server mode only

## Building

```bash
# Build all userspace programs including ntpd
./scripts/build.sh userspace

# Or build only ntpd
cd userspace
cargo build --release -p ntpd
```

## Files

| Path | Description |
|------|-------------|
| `src/syscalls/time.rs` | Kernel time syscalls |
| `src/syscalls/numbers.rs` | Syscall number definitions |
| `userspace/programs/ntpd/` | NTP client source |
| `userspace/nrlib/src/libc_compat/time_compat.rs` | libc time wrappers |

## Related Documentation

- [RFC 4330](https://tools.ietf.org/html/rfc4330) - SNTP Version 4
- [RFC 5905](https://tools.ietf.org/html/rfc5905) - NTPv4 Specification
- [UDP Network Stack](UDP_NETWORK_STACK.md) - UDP implementation details
