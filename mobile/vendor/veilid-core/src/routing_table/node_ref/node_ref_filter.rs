use super::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeRefFilter {
    routing_domain_set: RoutingDomainSet,
    dial_info_filter: DialInfoFilter,
}

impl Default for NodeRefFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeRefFilter {
    pub fn new() -> Self {
        Self {
            routing_domain_set: RoutingDomainSet::all(),
            dial_info_filter: DialInfoFilter::all(),
        }
    }

    pub fn routing_domain_set(&self) -> RoutingDomainSet {
        self.routing_domain_set
    }

    pub fn dial_info_filter(&self) -> DialInfoFilter {
        self.dial_info_filter
    }

    pub fn contains_transport(&self, transport: TransportType) -> bool {
        self.dial_info_filter.contains_transport(transport)
    }

    pub fn with_routing_domain(mut self, routing_domain: RoutingDomain) -> Self {
        self.routing_domain_set = routing_domain.into();
        self
    }
    pub fn with_routing_domain_set(mut self, routing_domain_set: RoutingDomainSet) -> Self {
        self.routing_domain_set = routing_domain_set;
        self
    }
    pub fn with_dial_info_filter(mut self, dial_info_filter: DialInfoFilter) -> Self {
        self.dial_info_filter = dial_info_filter;
        self
    }
    pub fn with_protocol_type(mut self, protocol_type: ProtocolType) -> Self {
        self.dial_info_filter = self.dial_info_filter.with_protocol_type(protocol_type);
        self
    }
    pub fn with_protocol_type_set(mut self, protocol_set: ProtocolTypeSet) -> Self {
        self.dial_info_filter = self.dial_info_filter.with_protocol_type_set(protocol_set);
        self
    }
    pub fn with_address_type(mut self, address_type: AddressType) -> Self {
        self.dial_info_filter = self.dial_info_filter.with_address_type(address_type);
        self
    }
    #[expect(dead_code)]
    pub fn with_address_type_set(mut self, address_set: AddressTypeSet) -> Self {
        self.dial_info_filter = self.dial_info_filter.with_address_type_set(address_set);
        self
    }
    pub fn filtered(mut self, other_filter: NodeRefFilter) -> Self {
        self.routing_domain_set &= other_filter.routing_domain_set;
        self.dial_info_filter = self
            .dial_info_filter
            .filtered(other_filter.dial_info_filter);
        self
    }
    #[expect(dead_code)]
    pub fn is_dead(&self) -> bool {
        self.dial_info_filter.is_dead() || self.routing_domain_set.is_empty()
    }
    pub fn apply_sequencing(mut self, sequencing: Sequencing) -> (SequenceOrdering, Self) {
        let (ordering, dif) = self.dial_info_filter.apply_sequencing(sequencing);
        self.dial_info_filter = dif;
        (ordering, self)
    }
}

impl From<RoutingDomain> for NodeRefFilter {
    fn from(other: RoutingDomain) -> Self {
        Self {
            routing_domain_set: other.into(),
            dial_info_filter: DialInfoFilter::all(),
        }
    }
}

impl From<RoutingDomainSet> for NodeRefFilter {
    fn from(other: RoutingDomainSet) -> Self {
        Self {
            routing_domain_set: other,
            dial_info_filter: DialInfoFilter::all(),
        }
    }
}

impl From<DialInfoFilter> for NodeRefFilter {
    fn from(other: DialInfoFilter) -> Self {
        Self {
            routing_domain_set: RoutingDomainSet::all(),
            dial_info_filter: other,
        }
    }
}

impl From<ProtocolType> for NodeRefFilter {
    fn from(other: ProtocolType) -> Self {
        Self {
            routing_domain_set: RoutingDomainSet::all(),
            dial_info_filter: DialInfoFilter::from(other),
        }
    }
}

impl From<AddressType> for NodeRefFilter {
    fn from(other: AddressType) -> Self {
        Self {
            routing_domain_set: RoutingDomainSet::all(),
            dial_info_filter: DialInfoFilter::from(other),
        }
    }
}

impl From<Flow> for NodeRefFilter {
    fn from(other: Flow) -> Self {
        Self {
            routing_domain_set: RoutingDomainSet::all(),
            dial_info_filter: DialInfoFilter::from(other),
        }
    }
}

impl fmt::Display for NodeRefFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            let mut mods = vec![];
            let pts = self.dial_info_filter.protocol_type_set();
            if pts.is_empty() {
                mods.push("!PROT".to_string());
            } else if pts != ProtocolTypeSet::all() {
                mods.extend(pts.iter().map(|x| f.to_string(x)));
            }
            let ats = self.dial_info_filter.address_type_set();
            if ats.is_empty() {
                mods.push("!ADDR".to_string());
            } else if ats != AddressTypeSet::all() {
                mods.extend(ats.iter().map(|x| f.to_string(x)));
            }
            if self.routing_domain_set.is_empty() {
                mods.push("!ROUT".to_string());
            } else if self.routing_domain_set != RoutingDomainSet::all() {
                mods.extend(self.routing_domain_set.iter().map(|x| f.to_string(x)))
            }
            let mods: String = mods.join("/");

            write!(f, "{}", mods)
        } else {
            let mut mods = vec![];
            let pts = self.dial_info_filter.protocol_type_set();
            if pts.is_empty() {
                mods.push("no-protocol-type".to_string());
            } else if pts != ProtocolTypeSet::all() {
                mods.extend(pts.iter().map(|x| x.to_string()));
            }
            let ats = self.dial_info_filter.address_type_set();
            if ats.is_empty() {
                mods.push("no-address-type".to_string());
            } else if ats != AddressTypeSet::all() {
                mods.extend(ats.iter().map(|x| x.to_string()));
            }
            if self.routing_domain_set.is_empty() {
                mods.push("no-routing-domain".to_string());
            } else if self.routing_domain_set != RoutingDomainSet::all() {
                mods.extend(self.routing_domain_set.iter().map(|x| x.to_string()))
            }
            let mods: String = mods.join("/");

            write!(f, "{}", mods)
        }
    }
}

impl FromStr for NodeRefFilter {
    type Err = VeilidAPIError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_ascii_lowercase();

        if s.is_empty() || s == "all" {
            return Ok(NodeRefFilter::new());
        }

        let mut ptset = ProtocolTypeSet::empty();
        let mut ptnone = false;
        let mut atset = AddressTypeSet::empty();
        let mut atnone = false;
        let mut rdset = RoutingDomainSet::empty();
        let mut rdnone = false;
        for m in s.split('/') {
            if let Ok(pt) = ProtocolType::set_from_str(m) {
                ptset |= pt;
            } else if let Ok(at) = AddressType::set_from_str(m) {
                atset |= at;
            } else if let Ok(rd) = RoutingDomain::set_from_str(m) {
                rdset |= rd;
            } else if "no-protocol-type".starts_with(m) && m.len() >= 4 {
                ptnone = true;
            } else if "no-address-type".starts_with(m) && m.len() >= 4 {
                atnone = true;
            } else if "no-routing-domain".starts_with(m) && m.len() >= 4 {
                rdnone = true;
            } else {
                return Err(VeilidAPIError::parse_error(
                    "NodeRefFilter::from_str failed",
                    s,
                ));
            }
        }
        if ptnone {
            if !ptset.is_empty() {
                return Err(VeilidAPIError::parse_error(
                    "Invalid ProtocolType set in NodeRefFilter::from_str",
                    s,
                ));
            }
        } else if ptset.is_empty() {
            ptset = ProtocolTypeSet::all();
        }

        if atnone {
            if !atset.is_empty() {
                return Err(VeilidAPIError::parse_error(
                    "Invalid AddressType set in NodeRefFilter::from_str",
                    s,
                ));
            }
        } else if atset.is_empty() {
            atset = AddressTypeSet::all();
        }

        if rdnone {
            if !rdset.is_empty() {
                return Err(VeilidAPIError::parse_error(
                    "Invalid RoutingDomain set in NodeRefFilter::from_str",
                    s,
                ));
            }
        } else if rdset.is_empty() {
            rdset = RoutingDomainSet::all();
        }

        Ok(NodeRefFilter {
            routing_domain_set: rdset,
            dial_info_filter: DialInfoFilter::all()
                .with_protocol_type_set(ptset)
                .with_address_type_set(atset),
        })
    }
}
