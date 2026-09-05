use super::*;

/// The lifecycle stage of the routing domain's inbound connectivity
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum RoutingDomainInboundStage {
    /// No network setup (no inbound/outbound protocols, address types, or capabilities). No dialinfo confirmation.
    #[default]
    Invalid,
    /// Network setup, but no dialinfo confirmation.
    NeedsDialInfoConfirmation,
    /// Network setup, dialinfo confirmation, but no address types or outbound protocols are enabled.
    Unusable,
    /// Network setup, dialinfo confirmation, address types + protocol types are valid, but 1+ relays must be selected because of missing dialinfo.
    NeedsRelays,
    /// Network setup, dialinfo confirmation, address types + protocol types are valid, and all relays needed are selected.
    ReadyToPublish,
}

impl fmt::Display for RoutingDomainInboundStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                RoutingDomainInboundStage::Invalid => write!(f, "Invalid"),
                RoutingDomainInboundStage::NeedsDialInfoConfirmation => {
                    write!(f, "NeedsDialInfoConfirmation")
                }
                RoutingDomainInboundStage::Unusable => write!(f, "Unusable"),
                RoutingDomainInboundStage::NeedsRelays => write!(f, "NeedsRelays"),
                RoutingDomainInboundStage::ReadyToPublish => write!(f, "ReadyToPublish"),
            }
        } else {
            match self {
                RoutingDomainInboundStage::Invalid => write!(f, "Invalid"),
                RoutingDomainInboundStage::NeedsDialInfoConfirmation => {
                    write!(f, "Needs Dial Info Confirmation")
                }
                RoutingDomainInboundStage::Unusable => write!(f, "Unusable"),
                RoutingDomainInboundStage::NeedsRelays => write!(f, "Needs Relays"),
                RoutingDomainInboundStage::ReadyToPublish => write!(f, "Ready To Publish"),
            }
        }
    }
}

/// The lifecycle stage of the routing domain's outbound connectivity
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum RoutingDomainOutboundStage {
    /// No network setup (no inboundoutbound protocols, address types, or capabilities). No dialinfo confirmation.
    #[default]
    Invalid,
    /// Network setup but routing table is is too small, bootstrap is needed.
    NeedsBootstrap,
    /// Needs to have published peer info to allocate safety routes because the nodes that we contact need full connectivity
    NeedsPublishedPeerInfo,
    /// Needs to have more tested nodes to allocate routes because the nodes that we contact need full connectivity
    NeedsMoreTestedNodes,
    /// Needs background safety routes to be allocated so we can do routing operations
    NeedsSafetyRoutes,
    /// Ready to perform all operations
    ReadyToOperate,
}

impl fmt::Display for RoutingDomainOutboundStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                RoutingDomainOutboundStage::Invalid => write!(f, "Invalid"),
                RoutingDomainOutboundStage::NeedsBootstrap => write!(f, "NeedsBootstrap"),
                RoutingDomainOutboundStage::NeedsPublishedPeerInfo => {
                    write!(f, "NeedsPublishedPeerInfo")
                }
                RoutingDomainOutboundStage::NeedsMoreTestedNodes => {
                    write!(f, "NeedsMoreTestedNodes")
                }
                RoutingDomainOutboundStage::NeedsSafetyRoutes => write!(f, "NeedsSafetyRoutes"),
                RoutingDomainOutboundStage::ReadyToOperate => write!(f, "ReadyToOperate"),
            }
        } else {
            match self {
                RoutingDomainOutboundStage::Invalid => write!(f, "Invalid"),
                RoutingDomainOutboundStage::NeedsBootstrap => write!(f, "Needs Bootstrap"),
                RoutingDomainOutboundStage::NeedsPublishedPeerInfo => {
                    write!(f, "Needs Published Peer Info")
                }
                RoutingDomainOutboundStage::NeedsMoreTestedNodes => {
                    write!(f, "Needs More Tested Nodes")
                }
                RoutingDomainOutboundStage::NeedsSafetyRoutes => write!(f, "Needs Safety Routes"),
                RoutingDomainOutboundStage::ReadyToOperate => write!(f, "Ready To Operate"),
            }
        }
    }
}

/// Combined state information for the routing domain
#[derive(Debug)]
pub struct RoutingDomainState {
    /// The lifecycle stage of the routing domain's inbound connectivity
    pub inbound_stage: RoutingDomainInboundStage,
    /// The lifecycle stage of the routing domain's outbound connectivity
    pub outbound_stage: RoutingDomainOutboundStage,
    /// The relay requirements of the routing domain
    pub relay_requirements: Arc<RelayRequirements>,
    /// The current relay compilation of the routing domain
    pub opt_relay_compilation: Option<RelayCompilation>,
    /// The current peer info of the routing domain
    pub current_peer_info: Arc<PeerInfo>,
    /// The entry snapshot used to calculate this routing domain's state
    pub entry_summary: Arc<EntrySummary>,
    /// The low water mark of the routing domain
    pub low_water_mark: Arc<LowWaterMark>,
    /// If this routing domain is ready for inbound use (dialinfo, relays, peer info, etc)
    pub is_ready_inbound: bool,
    /// If this routing domain is ready for outbound use (dialinfo, bootstrapped, safety routes, etc)
    pub is_ready_outbound: bool,
}

impl fmt::Display for RoutingDomainState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Inbound Stage: {}\nOutbound Stage: {}\nRelay Requirements:\n{}\nRelay Compilation:\n{}\nCurrent Peer Info:\n{}\nEntry Summary:\n{}\nLow Water Mark:\n{}",
            f.to_string(self.inbound_stage),
            f.to_string(self.outbound_stage),
            indent_all_string(f.to_string(&self.relay_requirements)),
            indent_all_string(f.to_string_opt(self.opt_relay_compilation.as_ref())),
            indent_all_string(f.to_string(&self.current_peer_info)),
            indent_all_string(f.to_string(&self.entry_summary)),
            indent_all_string(f.to_string(&self.low_water_mark))
        )
    }
}
