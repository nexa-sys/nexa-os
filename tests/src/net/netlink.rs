//! Netlink Protocol Tests
//!
//! Tests for netlink socket types and message structures.

#[cfg(test)]
mod tests {
    use crate::net::netlink::{
        NlMsgHdr, IfInfoMsg, RtAttr, IfAddrMsg, RtMsg, NetlinkSocket,
        NLMSG_DONE, NLMSG_ERROR,
        RTM_GETLINK, RTM_GETADDR, RTM_NEWLINK, RTM_NEWADDR, RTM_NEWROUTE,
        IFLA_IFNAME, IFLA_MTU, IFLA_OPERSTATE, IFLA_ADDRESS,
        IFA_ADDRESS, IFA_LABEL,
        RTA_DST, RTA_OIF, RTA_GATEWAY,
    };

    // =========================================================================
    // Message Type Constants Tests
    // =========================================================================

    #[test]
    fn test_nlmsg_types() {
        assert_eq!(NLMSG_ERROR, 2);
        assert_eq!(NLMSG_DONE, 3);
    }

    #[test]
    fn test_rtm_message_types() {
        // RTM constants should follow Linux values
        assert_eq!(RTM_NEWLINK, 16);
        assert_eq!(RTM_GETLINK, 18);
        assert_eq!(RTM_NEWADDR, 20);
        assert_eq!(RTM_GETADDR, 22);
        assert_eq!(RTM_NEWROUTE, 24);
    }

    #[test]
    fn test_rtm_types_distinct() {
        let types = [RTM_GETLINK, RTM_GETADDR, RTM_NEWLINK, RTM_NEWADDR, RTM_NEWROUTE];
        for i in 0..types.len() {
            for j in (i+1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    // =========================================================================
    // Interface Attribute Constants Tests
    // =========================================================================

    #[test]
    fn test_ifla_attributes() {
        assert_eq!(IFLA_ADDRESS, 1);
        assert_eq!(IFLA_IFNAME, 3);
        assert_eq!(IFLA_MTU, 4);
        assert_eq!(IFLA_OPERSTATE, 17);
    }

    #[test]
    fn test_ifa_attributes() {
        assert_eq!(IFA_ADDRESS, 1);
        assert_eq!(IFA_LABEL, 3);
    }

    #[test]
    fn test_rta_attributes() {
        assert_eq!(RTA_DST, 1);
        assert_eq!(RTA_OIF, 4);
        assert_eq!(RTA_GATEWAY, 5);
    }

    // =========================================================================
    // NlMsgHdr Structure Tests
    // =========================================================================

    #[test]
    fn test_nlmsghdr_size() {
        let size = core::mem::size_of::<NlMsgHdr>();
        // nlmsg_len(4) + nlmsg_type(2) + nlmsg_flags(2) + nlmsg_seq(4) + nlmsg_pid(4) = 16
        assert_eq!(size, 16);
    }

    #[test]
    fn test_nlmsghdr_alignment() {
        let align = core::mem::align_of::<NlMsgHdr>();
        assert!(align >= 4);
    }

    #[test]
    fn test_nlmsghdr_create() {
        let hdr = NlMsgHdr {
            nlmsg_len: 32,
            nlmsg_type: RTM_GETLINK,
            nlmsg_flags: 0x300, // NLM_F_REQUEST | NLM_F_DUMP
            nlmsg_seq: 1,
            nlmsg_pid: 1000,
        };
        assert_eq!(hdr.nlmsg_len, 32);
        assert_eq!(hdr.nlmsg_type, RTM_GETLINK);
        assert_eq!(hdr.nlmsg_pid, 1000);
    }

    #[test]
    fn test_nlmsghdr_copy() {
        let hdr1 = NlMsgHdr {
            nlmsg_len: 16,
            nlmsg_type: NLMSG_DONE,
            nlmsg_flags: 0,
            nlmsg_seq: 5,
            nlmsg_pid: 0,
        };
        let hdr2 = hdr1;
        assert_eq!(hdr1.nlmsg_type, hdr2.nlmsg_type);
    }

    #[test]
    fn test_nlmsghdr_clone() {
        let hdr1 = NlMsgHdr {
            nlmsg_len: 20,
            nlmsg_type: NLMSG_ERROR,
            nlmsg_flags: 0x100,
            nlmsg_seq: 10,
            nlmsg_pid: 500,
        };
        let hdr2 = hdr1.clone();
        assert_eq!(hdr1.nlmsg_seq, hdr2.nlmsg_seq);
    }

    // =========================================================================
    // IfInfoMsg Structure Tests
    // =========================================================================

    #[test]
    fn test_ifinfomsg_size() {
        let size = core::mem::size_of::<IfInfoMsg>();
        // ifi_family(1) + pad(1) + ifi_type(2) + ifi_index(4) + ifi_flags(4) + ifi_change(4) = 16
        assert_eq!(size, 16);
    }

    #[test]
    fn test_ifinfomsg_create() {
        let msg = IfInfoMsg {
            ifi_family: 0, // AF_UNSPEC
            __pad: 0,
            ifi_type: 1,    // ARPHRD_ETHER
            ifi_index: 2,
            ifi_flags: 0x1043, // IFF_UP | IFF_BROADCAST | IFF_RUNNING | IFF_MULTICAST
            ifi_change: 0xFFFFFFFF,
        };
        assert_eq!(msg.ifi_family, 0);
        assert_eq!(msg.ifi_index, 2);
    }

    #[test]
    fn test_ifinfomsg_copy() {
        let msg1 = IfInfoMsg {
            ifi_family: 2, // AF_INET
            __pad: 0,
            ifi_type: 1,
            ifi_index: 1,
            ifi_flags: 0,
            ifi_change: 0,
        };
        let msg2 = msg1;
        assert_eq!(msg1.ifi_family, msg2.ifi_family);
    }

    // =========================================================================
    // RtAttr Structure Tests
    // =========================================================================

    #[test]
    fn test_rtattr_size() {
        let size = core::mem::size_of::<RtAttr>();
        // rta_len(2) + rta_type(2) = 4
        assert_eq!(size, 4);
    }

    #[test]
    fn test_rtattr_create() {
        let attr = RtAttr {
            rta_len: 8,
            rta_type: IFLA_IFNAME,
        };
        assert_eq!(attr.rta_len, 8);
        assert_eq!(attr.rta_type, IFLA_IFNAME);
    }

    #[test]
    fn test_rtattr_copy() {
        let attr1 = RtAttr {
            rta_len: 12,
            rta_type: IFLA_MTU,
        };
        let attr2 = attr1;
        assert_eq!(attr1.rta_type, attr2.rta_type);
    }

    // =========================================================================
    // IfAddrMsg Structure Tests
    // =========================================================================

    #[test]
    fn test_ifaddrmsg_size() {
        let size = core::mem::size_of::<IfAddrMsg>();
        // ifa_family(1) + ifa_prefixlen(1) + ifa_flags(1) + ifa_scope(1) + ifa_index(4) = 8
        assert_eq!(size, 8);
    }

    #[test]
    fn test_ifaddrmsg_create() {
        let msg = IfAddrMsg {
            ifa_family: 2,     // AF_INET
            ifa_prefixlen: 24, // /24 network
            ifa_flags: 0,
            ifa_scope: 0,      // RT_SCOPE_UNIVERSE
            ifa_index: 1,
        };
        assert_eq!(msg.ifa_family, 2);
        assert_eq!(msg.ifa_prefixlen, 24);
        assert_eq!(msg.ifa_index, 1);
    }

    #[test]
    fn test_ifaddrmsg_copy() {
        let msg1 = IfAddrMsg {
            ifa_family: 2,
            ifa_prefixlen: 16,
            ifa_flags: 0x80,
            ifa_scope: 0,
            ifa_index: 2,
        };
        let msg2 = msg1;
        assert_eq!(msg1.ifa_prefixlen, msg2.ifa_prefixlen);
    }

    // =========================================================================
    // RtMsg Structure Tests
    // =========================================================================

    #[test]
    fn test_rtmsg_size() {
        let size = core::mem::size_of::<RtMsg>();
        // 8 u8 fields + 1 u32 field = 8 + 4 = 12
        assert_eq!(size, 12);
    }

    #[test]
    fn test_rtmsg_create() {
        let msg = RtMsg {
            rtm_family: 2,     // AF_INET
            rtm_dst_len: 0,    // Default route
            rtm_src_len: 0,
            rtm_tos: 0,
            rtm_table: 254,    // RT_TABLE_MAIN
            rtm_protocol: 4,   // RTPROT_STATIC
            rtm_scope: 0,      // RT_SCOPE_UNIVERSE
            rtm_type: 1,       // RTN_UNICAST
            rtm_flags: 0,
        };
        assert_eq!(msg.rtm_family, 2);
        assert_eq!(msg.rtm_table, 254);
    }

    #[test]
    fn test_rtmsg_copy() {
        let msg1 = RtMsg {
            rtm_family: 2,
            rtm_dst_len: 24,
            rtm_src_len: 0,
            rtm_tos: 0,
            rtm_table: 254,
            rtm_protocol: 2, // RTPROT_KERNEL
            rtm_scope: 253,  // RT_SCOPE_LINK
            rtm_type: 1,
            rtm_flags: 0,
        };
        let msg2 = msg1;
        assert_eq!(msg1.rtm_protocol, msg2.rtm_protocol);
    }

    // =========================================================================
    // NetlinkSocket Structure Tests
    // =========================================================================

    #[test]
    fn test_netlink_socket_size() {
        let size = core::mem::size_of::<NetlinkSocket>();
        // Should be reasonable size for socket structure
        assert!(size > 0);
        assert!(size < 256);
    }

    // =========================================================================
    // Netlink Flag Constants Tests
    // =========================================================================

    #[test]
    fn test_nlm_flags() {
        // NLM_F_* flags
        const NLM_F_REQUEST: u16 = 0x0001;
        const NLM_F_MULTI: u16 = 0x0002;
        const NLM_F_ACK: u16 = 0x0004;
        const NLM_F_ECHO: u16 = 0x0008;
        const NLM_F_DUMP: u16 = 0x0300;
        
        assert_eq!(NLM_F_REQUEST, 1);
        assert_eq!(NLM_F_MULTI, 2);
        assert_eq!(NLM_F_ACK, 4);
        assert_eq!(NLM_F_ECHO, 8);
        
        // DUMP is combination of ROOT | MATCH
        assert_eq!(NLM_F_DUMP, 0x0300);
    }

    // =========================================================================
    // Interface Flags Tests
    // =========================================================================

    #[test]
    fn test_interface_flags() {
        // IFF_* flags (Linux compatible)
        const IFF_UP: u32 = 0x0001;
        const IFF_BROADCAST: u32 = 0x0002;
        const IFF_LOOPBACK: u32 = 0x0008;
        const IFF_RUNNING: u32 = 0x0040;
        const IFF_MULTICAST: u32 = 0x1000;
        
        let eth0_flags = IFF_UP | IFF_BROADCAST | IFF_RUNNING | IFF_MULTICAST;
        let lo_flags = IFF_UP | IFF_LOOPBACK | IFF_RUNNING;
        
        assert!(eth0_flags & IFF_UP != 0);
        assert!(eth0_flags & IFF_BROADCAST != 0);
        assert!(eth0_flags & IFF_LOOPBACK == 0);
        
        assert!(lo_flags & IFF_LOOPBACK != 0);
        assert!(lo_flags & IFF_BROADCAST == 0);
    }

    // =========================================================================
    // Address Family Constants Tests
    // =========================================================================

    #[test]
    fn test_address_families() {
        const AF_UNSPEC: u8 = 0;
        const AF_INET: u8 = 2;
        const AF_INET6: u8 = 10;
        const AF_NETLINK: u8 = 16;
        
        assert_eq!(AF_UNSPEC, 0);
        assert_eq!(AF_INET, 2);
        assert_eq!(AF_INET6, 10);
        assert_eq!(AF_NETLINK, 16);
    }
}
