use super::*;

/// Low-level mirror of TransportType: (LowLevelProtocolType, AddressType). Used
/// as the keying unit for per-transport reliability stats so high-level protocols
/// that share a kernel socket (TCP and WS to the same addr+port) share an entry.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct LowLevelTransportType {
    pub low_level_protocol_type: LowLevelProtocolType,
    pub address_type: AddressType,
}

impl LowLevelTransportType {
    pub fn new(low_level_protocol_type: LowLevelProtocolType, address_type: AddressType) -> Self {
        Self {
            low_level_protocol_type,
            address_type,
        }
    }

    #[expect(dead_code)]
    pub fn low_level_protocol_type(&self) -> LowLevelProtocolType {
        self.low_level_protocol_type
    }

    #[expect(dead_code)]
    pub fn address_type(&self) -> AddressType {
        self.address_type
    }

    #[expect(dead_code)]
    pub fn socket_type(&self) -> SocketType {
        self.low_level_protocol_type.socket_type()
    }
}

impl From<TransportType> for LowLevelTransportType {
    fn from(t: TransportType) -> Self {
        Self::new(
            t.protocol_type().low_level_protocol_type(),
            t.address_type(),
        )
    }
}

impl From<&DialInfo> for LowLevelTransportType {
    fn from(di: &DialInfo) -> Self {
        Self::new(
            di.protocol_type().low_level_protocol_type(),
            di.address_type(),
        )
    }
}

impl From<DialInfo> for LowLevelTransportType {
    fn from(di: DialInfo) -> Self {
        Self::from(&di)
    }
}

impl fmt::Display for LowLevelTransportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.low_level_protocol_type, self.address_type)
    }
}
