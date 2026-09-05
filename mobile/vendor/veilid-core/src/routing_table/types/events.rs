use super::*;

pub struct PeerInfoChangeEvent {
    pub routing_domain: RoutingDomain,
    #[expect(dead_code)]
    pub opt_old_peer_info: Option<Arc<PeerInfo>>,
    pub opt_new_peer_info: Option<Arc<PeerInfo>>,
}

#[derive(Clone, Copy, Debug)]
pub struct RoutingDomainCommitEvent {
    pub routing_domain: RoutingDomain,
    #[expect(dead_code)]
    pub peer_info_changed: bool,
    pub confirmation_changed: bool,
    pub dial_info_changed: bool,
    pub relays_changed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct RoutingDomainReadyEvent {
    pub routing_domain: RoutingDomain,
    pub is_ready_inbound: bool,
    pub is_ready_outbound: bool,
}

/// Posted when allocated routes are added to or removed from the route spec
/// store. Subscribers (e.g. ConnectionManager) use this to refresh their view
/// of route-related state — e.g. SR first-hop connection protections.
#[derive(Clone, Debug)]
pub struct RouteAllocationEvent {
    #[expect(
        dead_code,
        reason = "informational, may be consumed by future subscribers"
    )]
    pub allocated: Vec<RouteId>,
    #[expect(
        dead_code,
        reason = "informational, may be consumed by future subscribers"
    )]
    pub released: Vec<RouteId>,
}
