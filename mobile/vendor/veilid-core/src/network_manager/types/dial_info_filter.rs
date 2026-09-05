use super::*;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct DialInfoFilter {
    protocol_type_set: ProtocolTypeSet,
    address_type_set: AddressTypeSet,
}

impl fmt::Display for DialInfoFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            let mut mods = vec![];
            let pts = self.protocol_type_set;
            if pts.is_empty() {
                mods.push("!PROT".to_string());
            } else if pts != ProtocolTypeSet::all() {
                mods.extend(pts.iter().map(|x| f.to_string(x)));
            }
            let ats = self.address_type_set;
            if ats.is_empty() {
                mods.push("!ADDR".to_string());
            } else if ats != AddressTypeSet::all() {
                mods.extend(ats.iter().map(|x| f.to_string(x)));
            }
            let mods: String = mods.join("/");

            write!(f, "{}", mods)
        } else {
            let mut mods = vec![];
            let pts = self.protocol_type_set;
            if pts.is_empty() {
                mods.push("no-protocol-type".to_string());
            } else if pts != ProtocolTypeSet::all() {
                mods.extend(pts.iter().map(|x| x.to_string()));
            }
            let ats = self.address_type_set;
            if ats.is_empty() {
                mods.push("no-address-type".to_string());
            } else if ats != AddressTypeSet::all() {
                mods.extend(ats.iter().map(|x| x.to_string()));
            }
            let mods: String = mods.join("/");

            write!(f, "{}", mods)
        }
    }
}

impl Default for DialInfoFilter {
    fn default() -> Self {
        Self {
            protocol_type_set: ProtocolTypeSet::all(),
            address_type_set: AddressTypeSet::all(),
        }
    }
}

impl DialInfoFilter {
    pub fn all() -> Self {
        Self {
            protocol_type_set: ProtocolTypeSet::all(),
            address_type_set: AddressTypeSet::all(),
        }
    }

    pub fn protocol_type_set(&self) -> ProtocolTypeSet {
        self.protocol_type_set
    }

    pub fn address_type_set(&self) -> AddressTypeSet {
        self.address_type_set
    }

    pub fn contains_transport(&self, transport: TransportType) -> bool {
        self.protocol_type_set.contains(transport.protocol_type())
            && self.address_type_set.contains(transport.address_type())
    }

    pub fn with_protocol_type(mut self, protocol_type: ProtocolType) -> Self {
        self.protocol_type_set = ProtocolTypeSet::only(protocol_type);
        self
    }
    pub fn with_protocol_type_set(mut self, protocol_set: ProtocolTypeSet) -> Self {
        self.protocol_type_set = protocol_set;
        self
    }
    pub fn with_address_type(mut self, address_type: AddressType) -> Self {
        self.address_type_set = AddressTypeSet::only(address_type);
        self
    }
    pub fn with_address_type_set(mut self, address_set: AddressTypeSet) -> Self {
        self.address_type_set = address_set;
        self
    }
    pub fn filtered(mut self, other_dif: DialInfoFilter) -> Self {
        self.protocol_type_set &= other_dif.protocol_type_set;
        self.address_type_set &= other_dif.address_type_set;
        self
    }
    pub fn is_dead(&self) -> bool {
        self.protocol_type_set.is_empty() || self.address_type_set.is_empty()
    }
    pub fn apply_sequencing(self, sequencing: Sequencing) -> (SequenceOrdering, DialInfoFilter) {
        // Get first filtered dialinfo
        match sequencing {
            Sequencing::PreferUnordered => (SequenceOrdering::Unordered, self),
            Sequencing::PreferOrdered => (SequenceOrdering::Ordered, self),
            Sequencing::EnsureOrdered => (
                SequenceOrdering::Ordered,
                self.filtered(
                    DialInfoFilter::all().with_protocol_type_set(ProtocolType::all_ordered_set()),
                ),
            ),
        }
        // return ordered sort and filter with ensure applied
    }
    pub fn is_ordered_only(&self) -> bool {
        for pt in self.protocol_type_set {
            if !matches!(pt.sequence_ordering(), SequenceOrdering::Ordered) {
                return false;
            }
        }
        true
    }
}

impl FromStr for DialInfoFilter {
    type Err = VeilidAPIError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_ascii_lowercase();

        if s.is_empty() || s == "all" {
            return Ok(DialInfoFilter::all());
        }

        let mut ptset = ProtocolTypeSet::empty();
        let mut ptnone = false;
        let mut atset = AddressTypeSet::empty();
        let mut atnone = false;
        for m in s.split('/') {
            if let Ok(pt) = ProtocolType::set_from_str(m) {
                ptset |= pt;
            } else if let Ok(at) = AddressType::set_from_str(m) {
                atset |= at;
            } else if "no-protocol-type".starts_with(m) && m.len() >= 4 {
                ptnone = true;
            } else if "no-address-type".starts_with(m) && m.len() >= 4 {
                atnone = true;
            } else {
                return Err(VeilidAPIError::parse_error(
                    "DialInfoFilter::from_str failed",
                    s,
                ));
            }
        }
        if ptnone {
            if !ptset.is_empty() {
                return Err(VeilidAPIError::parse_error(
                    "Invalid ProtocolType set in DialInfoFilter::from_str",
                    s,
                ));
            }
        } else if ptset.is_empty() {
            ptset = ProtocolTypeSet::all();
        }

        if atnone {
            if !atset.is_empty() {
                return Err(VeilidAPIError::parse_error(
                    "Invalid AddressType set in DialInfoFilter::from_str",
                    s,
                ));
            }
        } else if atset.is_empty() {
            atset = AddressTypeSet::all();
        }

        Ok(DialInfoFilter::all()
            .with_protocol_type_set(ptset)
            .with_address_type_set(atset))
    }
}

impl fmt::Debug for DialInfoFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let mut out = String::new();
        if self.protocol_type_set != ProtocolTypeSet::all() {
            out += &format!("+{:?}", self.protocol_type_set);
        } else {
            out += "*";
        }
        if self.address_type_set != AddressTypeSet::all() {
            out += &format!("+{:?}", self.address_type_set);
        } else {
            out += "*";
        }
        write!(f, "[{}]", out)
    }
}

impl From<ProtocolType> for DialInfoFilter {
    fn from(other: ProtocolType) -> Self {
        Self {
            protocol_type_set: ProtocolTypeSet::from(other),
            address_type_set: AddressTypeSet::all(),
        }
    }
}

impl From<AddressType> for DialInfoFilter {
    fn from(other: AddressType) -> Self {
        Self {
            protocol_type_set: ProtocolTypeSet::all(),
            address_type_set: AddressTypeSet::from(other),
        }
    }
}

impl From<Flow> for DialInfoFilter {
    fn from(other: Flow) -> Self {
        Self {
            protocol_type_set: ProtocolTypeSet::from(other.protocol_type()),
            address_type_set: AddressTypeSet::from(other.address_type()),
        }
    }
}

impl From<TransportType> for DialInfoFilter {
    fn from(other: TransportType) -> Self {
        Self {
            protocol_type_set: ProtocolTypeSet::from(other.protocol_type()),
            address_type_set: AddressTypeSet::from(other.address_type()),
        }
    }
}

pub trait MatchesDialInfoFilter {
    fn matches_filter(&self, filter: &DialInfoFilter) -> bool;
}
