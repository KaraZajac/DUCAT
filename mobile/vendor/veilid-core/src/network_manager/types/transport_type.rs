use super::*;

/// A protocol type paired with an address type. Identifies a single
/// transport (e.g. TCP/IPv4, UDP/IPv6) that a node may speak.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct TransportType {
    pub protocol_type: ProtocolType,
    pub address_type: AddressType,
}

impl TransportType {
    pub fn new(protocol_type: ProtocolType, address_type: AddressType) -> Self {
        Self {
            protocol_type,
            address_type,
        }
    }

    pub fn protocol_type(&self) -> ProtocolType {
        self.protocol_type
    }

    pub fn address_type(&self) -> AddressType {
        self.address_type
    }

    pub fn framing_type(&self) -> FramingType {
        self.protocol_type.framing_type()
    }

    #[expect(dead_code)]
    pub fn socket_type(&self) -> SocketType {
        self.protocol_type.low_level_protocol_type().socket_type()
    }

    pub fn sequence_ordering(&self) -> SequenceOrdering {
        self.protocol_type.sequence_ordering()
    }
}

impl From<&DialInfo> for TransportType {
    fn from(di: &DialInfo) -> Self {
        Self::new(di.protocol_type(), di.address_type())
    }
}

impl From<DialInfo> for TransportType {
    fn from(di: DialInfo) -> Self {
        Self::from(&di)
    }
}

impl fmt::Display for TransportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.protocol_type, self.address_type)
    }
}
