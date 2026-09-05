use super::*;

// Keep member order appropriate for sorting < preference
#[derive(Debug, Clone, PartialEq, PartialOrd, Ord, Eq, Hash, Serialize, Deserialize)]
pub struct DialInfoDetail {
    pub class: DialInfoClass,
    pub dial_info: DialInfo,
}

impl fmt::Display for DialInfoDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}:{}", self.class, self.dial_info)
    }
}

impl MatchesDialInfoFilter for DialInfoDetail {
    fn matches_filter(&self, filter: &DialInfoFilter) -> bool {
        self.dial_info.matches_filter(filter)
    }
}

pub type DialInfoDetailSort<'a> =
    dyn Fn(&DialInfoDetail, &DialInfoDetail) -> core::cmp::Ordering + Send + Sync + 'a;

impl DialInfoDetail {
    pub fn get_ordering_sort(
        ordering: SequenceOrdering,
    ) -> Option<Box<DialInfoDetailSort<'static>>> {
        match ordering {
            SequenceOrdering::Unordered => None,
            SequenceOrdering::Ordered => Some(Box::new(Self::ordered_sequencing_sort)),
        }
    }
    pub fn ordered_sequencing_sort(a: &DialInfoDetail, b: &DialInfoDetail) -> core::cmp::Ordering {
        let c = DialInfo::ordered_sequencing_sort(&a.dial_info, &b.dial_info);
        if c != core::cmp::Ordering::Equal {
            return c;
        }
        a.class.cmp(&b.class)
    }

    pub const NO_SORT: std::option::Option<&DialInfoDetailSort<'_>> = None::<_>;
}

pub trait HasDialInfoDetailList: fmt::Debug {
    /// Returns the dial info detail list, exclusive of relays
    fn dial_info_detail_list(&self) -> &[DialInfoDetail];

    /// The first dial info detail from the list in sorted order that matches the filter
    fn first_filtered_dial_info_detail(
        &self,
        sort: Option<&DialInfoDetailSort>,
        filter: &dyn Fn(&DialInfoDetail) -> bool,
    ) -> Option<DialInfoDetail> {
        if let Some(sort) = sort {
            let mut dids = self.dial_info_detail_list().to_vec();
            dids.sort_unstable_by(sort);
            for did in dids {
                if filter(&did) {
                    return Some(did);
                }
            }
        } else {
            for did in self.dial_info_detail_list() {
                if filter(did) {
                    return Some(did.clone());
                }
            }
        };
        None
    }

    /// The list of dial info details in sorted order that match the filter
    fn filtered_dial_info_details(
        &self,
        sort: Option<&DialInfoDetailSort>,
        filter: &dyn Fn(&DialInfoDetail) -> bool,
    ) -> Vec<DialInfoDetail> {
        let mut dial_info_detail_list = Vec::new();

        if let Some(sort) = sort {
            let mut dids = self.dial_info_detail_list().to_vec();
            dids.sort_unstable_by(sort);
            for did in dids {
                if filter(&did) {
                    dial_info_detail_list.push(did);
                }
            }
        } else {
            for did in self.dial_info_detail_list() {
                if filter(did) {
                    dial_info_detail_list.push(did.clone());
                }
            }
        };
        dial_info_detail_list
    }

    /// Does this node has some dial info
    fn has_dial_info(&self) -> bool {
        !self.dial_info_detail_list().is_empty()
    }

    /// Can direct connections be made
    fn is_fully_direct_inbound(&self) -> bool {
        // Do any of our dial info require signalling? if so it is not fully direct
        for did in self.dial_info_detail_list() {
            if did.class.requires_signal() {
                return false;
            }
        }
        true
    }

    /// Does this appear on the same network within the routing domain?
    /// The notion of 'ipblock' is a single external IP address for ipv4, and a fixed prefix for ipv6.
    /// This is used to rate-limiting the number of nodes coming from the same ip 'block' within a
    /// specific amount of time for the address filter, and to improve network diversity for route allocation.
    fn is_on_same_ipblock(
        &self,
        other: &dyn HasDialInfoDetailList,
        ip6_prefix_size: usize,
    ) -> bool {
        let our_ip_blocks = self
            .dial_info_detail_list()
            .iter()
            .map(|did| ip_to_ipblock(ip6_prefix_size, did.dial_info.to_socket_addr().ip()))
            .collect::<HashSet<_>>();

        for did in other.dial_info_detail_list() {
            let ipblock = ip_to_ipblock(ip6_prefix_size, did.dial_info.to_socket_addr().ip());
            if our_ip_blocks.contains(&ipblock) {
                return true;
            }
        }
        false
    }

    /// Get set of ipblocks for this node
    fn get_ipblocks(&self, ip6_prefix_size: usize) -> HashSet<IpAddr> {
        self.dial_info_detail_list()
            .iter()
            .map(|did| ip_to_ipblock(ip6_prefix_size, did.dial_info.to_socket_addr().ip()))
            .collect::<HashSet<_>>()
    }

    /// Build a map of protocols to low level ports
    /// This way we can get the set of protocols required to keep our NAT mapping alive for keepalive pings
    /// Only one protocol per low level protocol/port combination is required
    /// For example, if WS/WSS and TCP protocols are on the same low-level TCP port, only TCP keepalives will be required
    /// and we do not need to do WS/WSS keepalive as well. If they are on different ports, then we will need WS/WSS keepalives too.
    fn get_low_level_port_info(&self) -> LowLevelPortInfo {
        let mut low_level_protocol_ports =
            BTreeSet::<(LowLevelProtocolType, AddressType, u16)>::new();
        let mut protocol_to_port = BTreeMap::<TransportType, (LowLevelProtocolType, u16)>::new();

        for did in self.dial_info_detail_list() {
            low_level_protocol_ports.insert((
                did.dial_info.protocol_type().low_level_protocol_type(),
                did.dial_info.address_type(),
                did.dial_info.socket_address().port(),
            ));
            protocol_to_port.insert(
                TransportType::new(did.dial_info.protocol_type(), did.dial_info.address_type()),
                (
                    did.dial_info.protocol_type().low_level_protocol_type(),
                    did.dial_info.socket_address().port(),
                ),
            );
        }
        LowLevelPortInfo {
            low_level_protocol_ports,
            protocol_to_port,
        }
    }

    /// Get geolocation info of just this object
    #[cfg(feature = "geolocation")]
    fn get_country_code(&self) -> Option<CountryCode> {
        let country_codes = self
            .dial_info_detail_list()
            .iter()
            .map(|did| match &did.dial_info {
                DialInfo::UDP(di) => di.socket_address.ip_addr(),
                DialInfo::TCP(di) => di.socket_address.ip_addr(),
                DialInfo::WS(di) => di.socket_address.ip_addr(),
                #[cfg(feature = "enable-protocol-wss")]
                DialInfo::WSS(di) => di.socket_address.ip_addr(),
            })
            .map(geolocation::query_country_code)
            .collect::<Vec<_>>();

        if country_codes.is_empty() {
            return None;
        }

        // Indexing cannot panic, guarded by a check above
        let cc0 = country_codes[0];

        if !country_codes.iter().all(|cc| cc.is_some() && *cc == cc0) {
            // Lookup failed for some address or results are different
            return None;
        }

        cc0
    }
}
