use super::*;

// Ordering here matters, IPV6 is preferred to IPV4 in dial info sorts
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Ord, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Address {
    IPV6(Ipv6Addr),
    IPV4(Ipv4Addr),
}

impl Default for Address {
    fn default() -> Self {
        Address::IPV4(Ipv4Addr::new(0, 0, 0, 0))
    }
}

impl Address {
    pub fn from_socket_addr(sa: SocketAddr) -> Address {
        match sa {
            SocketAddr::V4(v4) => Address::IPV4(*v4.ip()),
            SocketAddr::V6(v6) => Address::IPV6(*v6.ip()),
        }
    }
    pub fn from_ip_addr(addr: IpAddr) -> Address {
        match addr {
            IpAddr::V4(v4) => Address::IPV4(v4),
            IpAddr::V6(v6) => Address::IPV6(v6),
        }
    }
    pub fn address_type(&self) -> AddressType {
        match self {
            Address::IPV4(_) => AddressType::IPV4,
            Address::IPV6(_) => AddressType::IPV6,
        }
    }
    pub fn is_unspecified(&self) -> bool {
        match self {
            Address::IPV4(v4) => ipv4addr_is_unspecified(v4),
            Address::IPV6(v6) => ipv6addr_is_unspecified(v6),
        }
    }
    pub fn is_global(&self) -> bool {
        match self {
            Address::IPV4(v4) => ipv4addr_is_global(v4) && !ipv4addr_is_multicast(v4),
            Address::IPV6(v6) => ipv6addr_is_unicast_global(v6),
        }
    }
    pub fn is_local(&self) -> bool {
        match self {
            Address::IPV4(v4) => {
                ipv4addr_is_private(v4)
                    || ipv4addr_is_link_local(v4)
                    || ipv4addr_is_shared(v4)
                    || ipv4addr_is_ietf_protocol_assignment(v4)
            }
            Address::IPV6(v6) => {
                ipv6addr_is_unicast_site_local(v6)
                    || ipv6addr_is_unicast_link_local(v6)
                    || ipv6addr_is_unique_local(v6)
            }
        }
    }
    /// Indicates the address is on a network where the local host is behind a carrier-grade NAT
    /// or DS-Lite tunnel — RFC 6598 shared address space (100.64.0.0/10) or RFC 6333 IETF
    /// protocol assignments (192.0.0.0/24). When seen on a local interface, inbound from the
    /// public internet to this host is not reachable, even if peers report a globally routable
    /// external address (because the carrier owns the public range and uses it for CGNAT).
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
    pub fn is_cgnat_indicator(&self) -> bool {
        match self {
            Address::IPV4(v4) => ipv4addr_is_shared(v4) || ipv4addr_is_ietf_protocol_assignment(v4),
            Address::IPV6(_) => false,
        }
    }

    /// Translator / encapsulation source address whose value does not represent real network membership.
    ///
    /// When seen as a flow's local address, the routing domain of the flow is determined by the
    /// remote address's domain rather than by classifying the local side. Covers:
    /// - IPv4 `192.0.0.0/29` CLAT46 plat (RFC 7335; subset of IETF protocol assignment)
    /// - IPv4 `100.64.0.0/10` CGNAT shared (RFC 6598)
    /// - IPv6 `64:ff9b::/96` NAT64 well-known prefix (RFC 6052)
    /// - IPv6 `2001::/32` Teredo (RFC 4380)
    /// - IPv6 `2002::/16` 6to4 (RFC 3056)
    pub fn is_synthetic_local(&self) -> bool {
        match self {
            Address::IPV4(v4) => ipv4addr_is_shared(v4) || ipv4addr_is_ietf_protocol_assignment(v4),
            Address::IPV6(v6) => {
                ipv6addr_is_nat64_well_known(v6) || ipv6addr_is_teredo(v6) || ipv6addr_is_6to4(v6)
            }
        }
    }

    /// IPv4 link-local / APIPA `169.254.0.0/16` (RFC 3927) — DHCP-failed fallback.
    pub fn is_ipv4_link_local(&self) -> bool {
        matches!(self, Address::IPV4(v4) if ipv4addr_is_link_local(v4))
    }

    /// IPv6 link-local `fe80::/10` (RFC 4291).
    pub fn is_ipv6_link_local(&self) -> bool {
        matches!(self, Address::IPV6(v6) if ipv6addr_is_unicast_link_local(v6))
    }

    /// Link-local addresses
    pub fn is_link_local(&self) -> bool {
        match self {
            Address::IPV4(v4) => ipv4addr_is_link_local(v4),
            Address::IPV6(v6) => ipv6addr_is_unicast_link_local(v6),
        }
    }

    /// Loopback addresses
    pub fn is_loopback(&self) -> bool {
        match self {
            Address::IPV4(v4) => ipv4addr_is_loopback(v4),
            Address::IPV6(v6) => ipv6addr_is_loopback(v6),
        }
    }

    /// Routable addresses are anything that can be used beyond the local link
    /// This does not include loopback or link-local addresses, but does include
    /// 'weird' addresses like test and benchmark networks that can be routed internally
    /// to organizations and thinks like point-to-point links and NAT, as well as
    /// private-use addresses ranges that are not -publicly- routable, but are routable
    /// over local networks.
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
    pub fn is_routable(&self) -> bool {
        // Loopback is not routable
        if self.is_loopback() {
            return false;
        }

        // Link-local is not routable
        if self.is_link_local() {
            return false;
        }

        // Everything else is routable in some context
        true
    }

    pub fn ip_addr(&self) -> IpAddr {
        match self {
            Self::IPV4(a) => IpAddr::V4(*a),
            Self::IPV6(a) => IpAddr::V6(*a),
        }
    }
    pub fn socket_addr(&self, port: u16) -> SocketAddr {
        SocketAddr::new(self.ip_addr(), port)
    }
    pub fn canonical(&self) -> Address {
        match self {
            Address::IPV4(v4) => Address::IPV4(*v4),
            Address::IPV6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => Address::IPV4(v4),
                None => Address::IPV6(*v6),
            },
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Address::IPV4(v4) => write!(f, "{}", v4),
            Address::IPV6(v6) => write!(f, "{}", v6),
        }
    }
}

impl FromStr for Address {
    type Err = VeilidAPIError;
    fn from_str(host: &str) -> VeilidAPIResult<Address> {
        if let Ok(addr) = Ipv4Addr::from_str(host) {
            Ok(Address::IPV4(addr))
        } else if let Ok(addr) = Ipv6Addr::from_str(host) {
            Ok(Address::IPV6(addr))
        } else {
            Err(VeilidAPIError::parse_error(
                "Address::from_str failed",
                host,
            ))
        }
    }
}
