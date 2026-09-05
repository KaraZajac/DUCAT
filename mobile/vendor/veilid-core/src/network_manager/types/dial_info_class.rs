use super::*;

// Keep member order appropriate for sorting < preference
#[derive(Debug, Ord, PartialOrd, Hash, Serialize, Deserialize, EnumSetType)]
pub(crate) enum DialInfoClass {
    Direct = 0, // D = Directly reachable with public IP and no firewall, with statically configured port
    Mapped = 1, // M = Directly reachable with via portmap behind any NAT or firewalled with fixed or negotiated port
    FullConeNAT = 2, // F = Directly reachable device without portmap behind full-cone NAT (or manually mapped firewall port with no configuration change)
    Blocked = 3,     // B = Inbound blocked at firewall but may hole punch with public address
    AddressRestrictedNAT = 4, // A = Device without portmap behind address-only restricted NAT
    PortRestrictedNAT = 5, // P = Device without portmap behind address-and-port restricted NAT
}

impl DialInfoClass {
    /// Is the address/port translation of this dial info class handled by a NAT router dynamically?
    pub fn is_dynamically_translated(&self) -> bool {
        matches!(
            self,
            Self::FullConeNAT | Self::AddressRestrictedNAT | Self::PortRestrictedNAT
        )
    }

    /// Is a signal required to do an inbound hole-punch or reverse connection?
    pub fn requires_signal(&self) -> bool {
        matches!(
            self,
            Self::Blocked | Self::AddressRestrictedNAT | Self::PortRestrictedNAT
        )
    }

    /// For full cone NAT, the relay itself may not be used but the keepalive sent to it
    /// is required to keep the NAT mapping valid in the router state table
    pub fn wants_nat_keepalive(&self) -> bool {
        matches!(self, Self::FullConeNAT)
    }
}

#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
pub(crate) type DialInfoClassSet = EnumSet<DialInfoClass>;
