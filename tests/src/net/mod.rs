//! Network stack tests
//!
//! This module contains all network protocol tests including:
//! - Ethernet frame handling
//! - IPv4 address/packet operations
//! - ARP protocol
//! - UDP datagrams
//! - TCP state machine
//! - DNS protocol
//! - Comprehensive protocol stack tests

mod arp;
mod checksum_validation;
mod comprehensive;
mod dns;
mod ethernet;
mod ipv4;
mod ipv4_validation;
mod tcp_edge_cases;
mod tcp_states;
mod udp;
mod udp_helper;
