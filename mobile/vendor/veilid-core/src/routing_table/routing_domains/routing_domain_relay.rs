use super::*;

/// The routing domain's record of the output of the relay management task
/// containing everything the rest of the system needs to know about a single relay
#[derive(Debug, Clone)]
pub struct RoutingDomainRelay {
    pub relay_id: NodeId,
    pub relay_node: FilteredNodeRef,
    pub relay_kind: RelayKind,
    pub pings: Vec<RelayPing>,
    pub dial_info_details: Vec<DialInfoDetail>,
}

impl RoutingDomainRelay {
    /// Checks if this relay is configured the same way as another relay
    pub fn equivalent(&self, other: &Self) -> bool {
        self.relay_node.equivalent(&other.relay_node)
            && self.relay_kind == other.relay_kind
            && self.pings.len() == other.pings.len()
            && self
                .pings
                .iter()
                .zip(other.pings.iter())
                .all(|(a, b)| a.equivalent(b))
            && self.dial_info_details == other.dial_info_details
    }
}

impl RoutingDomainRelay {
    pub fn new(routing_domain: RoutingDomain, relay_node: NodeRef, relay_kind: RelayKind) -> Self {
        let relay_node =
            relay_node.custom_filtered(NodeRefFilter::new().with_routing_domain(routing_domain));
        let relay_id = relay_node.best_node_id();
        RoutingDomainRelay {
            relay_id,
            relay_node,
            relay_kind,
            dial_info_details: vec![],
            pings: vec![],
        }
    }
}

impl fmt::Display for RoutingDomainRelay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:#}: {}\nPings:\n{}\nDial Info Details:\n{}",
            self.relay_kind,
            self.relay_node,
            indent_all_string(
                f.to_multiline_indexed_string(self.pings.iter().map(|p| f.to_string(p)))
            ),
            indent_all_string(f.to_multiline_indexed_string(
                self.dial_info_details.iter().map(|d| f.to_string(d))
            )),
        )
    }
}

/// The record of a node that needs to be pinged to keep a relay alive
#[derive(Debug, Clone)]
pub struct RelayPing {
    pub node_ref: NodeRef,
    pub dial_info: DialInfo,
}

impl RelayPing {
    pub fn equivalent(&self, other: &Self) -> bool {
        self.node_ref.equivalent(&other.node_ref) && self.dial_info == other.dial_info
    }
}

impl fmt::Display for RelayPing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}@{}",
            f.to_string(&self.node_ref),
            f.to_string(&self.dial_info)
        )
    }
}

/// The routing domain's state tracking for a single relay
/// Keeps track of when the last keepalive was sent to the relay
/// and when the last attempt to optimize the relay was made
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct RoutingDomainRelayState {
    pub last_keepalive: Timestamp,
    pub last_optimized: Timestamp,
}

impl fmt::Display for RoutingDomainRelayState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Keepalive:{}\nOptimized:{}",
            f.to_string(self.last_keepalive),
            f.to_string(self.last_optimized)
        )
    }
}
