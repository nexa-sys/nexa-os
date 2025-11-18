# DHCP Implementation Summary

## Overview
Implemented a userspace DHCP client (`dhcp`) for NexaOS. The client communicates with DHCP servers to obtain an IP address and other network configuration parameters.

## Components

### Userspace Client (`userspace/dhcp.rs`)
- **Socket Creation**: Uses `socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP)` to create a UDP socket.
- **Broadcast**: Sets `SO_BROADCAST` option (via `setsockopt` stub) to allow sending broadcast packets.
- **MAC Address Retrieval**: Uses Netlink (`RTM_GETLINK`) to retrieve the MAC address of the network interface.
- **DHCP State Machine**:
  1.  **DISCOVER**: Broadcasts a DHCP DISCOVER packet to port 67.
  2.  **OFFER**: Listens on port 68 for a DHCP OFFER from a server.
  3.  **REQUEST**: Broadcasts a DHCP REQUEST packet with the offered IP.
  4.  **ACK**: Waits for a DHCP ACK to confirm the lease.
- **Packet Parsing**: Custom `DhcpPacket` struct and option parsing logic.

### Build System Integration
- Modified `scripts/build-rootfs.sh` to:
  - Build `dhcp` binary using `cargo build`.
  - Copy `dhcp` to `/bin/dhcp` in the root filesystem.
  - Strip symbols to reduce size.

## Usage
Run `dhcp` from the shell:
```sh
/ # dhcp
Starting DHCP client...
Using MAC address: 52:54:00:12:34:56
Sending DHCP DISCOVER...
Waiting for DHCP OFFER...
Received DHCP OFFER: IP 10.0.2.15
Sending DHCP REQUEST...
Waiting for DHCP ACK...
DHCP Success! Assigned IP: 10.0.2.15
```

## Future Work
- **IP Configuration**: Implement `RTM_NEWADDR` netlink message or a syscall to actually configure the interface with the obtained IP.
- **Timeout/Retry**: Implement retransmission logic for lost packets.
- **Daemonization**: Run as a background service.
- **DNS Configuration**: Parse DNS servers from DHCP options and update `/etc/resolv.conf`.
