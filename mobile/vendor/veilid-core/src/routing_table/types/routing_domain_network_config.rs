use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutingDomainNetworkConfig {
    /// The protocols that are supported for outbound connections from this domain
    pub outbound_protocols: ProtocolTypeSet,
    /// The protocols that are supported for inbound connections to this domain
    pub inbound_protocols: ProtocolTypeSet,
    /// The types of addresses that are supported by this domain
    pub address_types: AddressTypeSet,
    /// The capabilities that are supported by this domain
    pub capabilities: BTreeSet<VeilidCapability>,
}

impl RoutingDomainNetworkConfig {
    pub fn new(
        outbound_protocols: ProtocolTypeSet,
        inbound_protocols: ProtocolTypeSet,
        address_types: AddressTypeSet,
        capabilities: BTreeSet<VeilidCapability>,
    ) -> Self {
        Self {
            outbound_protocols,
            inbound_protocols,
            address_types,
            capabilities,
        }
    }
}

impl fmt::Display for RoutingDomainNetworkConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let caps = self
            .capabilities
            .iter()
            .map(|c| f.to_string(c))
            .collect::<Vec<_>>()
            .join(",")
            .string_if_empty("None");
        if f.alternate() {
            write!(
                f,
                "Outbound Protocols: {:#}\nInbound Protocols: {:#}\nAddress Types: {:#}\nCapabilities: {:#}",
                self.outbound_protocols,
                self.inbound_protocols,
                self.address_types,
                caps
            )
        } else {
            write!(
                f,
                "out: {}, in: {}, addr: {}, caps: {}",
                self.outbound_protocols, self.inbound_protocols, self.address_types, caps
            )
        }
    }
}
