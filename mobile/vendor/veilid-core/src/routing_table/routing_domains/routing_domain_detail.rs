use super::*;

impl_veilid_log_facility!("rtab");

/// General trait for all routing domains
pub trait RoutingDomainDetail:
    VeilidComponentRegistryAccessor + core::any::Any + core::fmt::Debug + Send + Sync
{
    /// The routing domain identifier for this routing domain detail
    #[expect(dead_code)]
    fn as_any(&self) -> &dyn core::any::Any;
    /// The routing domain identifier for this routing domain detail
    fn as_any_mut(&mut self) -> &mut dyn core::any::Any;

    /// The routing domain identifier for this routing domain detail
    fn routing_domain(&self) -> RoutingDomain;

    /// The current relay requirements of this routing domain
    fn relay_requirements(&self) -> Arc<RelayRequirements>;

    /// The current network configuration for this routing domain
    fn network_config(&self) -> &RoutingDomainNetworkConfig;
    /// What protocols are supported for outbound connections from this domain
    fn outbound_protocols(&self) -> ProtocolTypeSet;
    /// What protocols are supported for inbound connections to this domain
    fn inbound_protocols(&self) -> ProtocolTypeSet;
    /// What types of addresses are supported in this domain
    fn address_types(&self) -> AddressTypeSet;
    /// Compatible routing domains for flows involving this routing domain
    fn origin_routing_domains(&self) -> RoutingDomainSet;
    /// Whether or not the network dialinfo has been confirmed by the network manager
    fn confirmed(&self) -> bool;
    /// What Veilid capabilities are supported by RPCs in this routing domain
    fn capabilities(&self) -> BTreeSet<VeilidCapability>;
    /// What relays are configured for this domain
    fn relays(&self) -> Vec<RoutingDomainRelay>;
    /// The output of the last relay compilation for this domain
    fn relay_compilation(&self) -> Option<RelayCompilation>;
    /// The state of a particular relay in this domain
    fn relay_state(&self, relay_id: NodeId) -> Option<RoutingDomainRelayState>;
    /// What dial info details are configured for this domain
    fn dial_info_details(&self) -> &Vec<DialInfoDetail>;
    /// Whether or not the network associated with this domain has addresses that are translated (NAT)
    fn translated_address_types(&self) -> AddressTypeSet;
    /// The dial info filter for inbound connections to this domain
    fn inbound_dial_info_filter(&self) -> DialInfoFilter;
    /// The dial info filter for outbound connections from this domain
    fn outbound_dial_info_filter(&self) -> DialInfoFilter;
    /// The current peer info for this domain, regardless of publication status. May be a work in progress.
    fn get_peer_info(&self) -> Arc<PeerInfo>;

    /// Can this routing domain contain a particular address
    fn can_contain_address(&self, address: Address) -> bool;
    /// Whether or not the dial info is valid for this domain
    fn ensure_dial_info_is_valid(&self, dial_info: &DialInfo) -> bool;

    /// Refresh caches if external data changes
    fn invalidate(&self);

    /// Get the bootstrap peers that we last used in this routing domain
    /// May be empty is the node started up and did not need to bootstrap
    fn get_bootstrap_peers(&self) -> Vec<NodeRef>;
    /// Clear the bootstrap peers for this domain
    fn clear_bootstrap_peers(&self);
    /// Add a bootstrap peer to this domain
    fn add_bootstrap_peer(&self, bootstrap_peer: NodeRef);

    /// Update the low water mark of nodes since the last peer minimum refresh
    fn update_low_water_mark(&self, low_water_mark: Arc<LowWaterMark>);
    /// Reset the low water mark of nodes, will be set on the next update
    fn reset_low_water_mark(&self);
    /// Get the low water mark of nodes at the time of the last peer minimum refresh
    fn get_low_water_mark(&self) -> Arc<LowWaterMark>;

    /// Return the last set of entry summary for this domain
    fn get_entry_summary(&self) -> Arc<EntrySummary>;
    /// Update the cached entry summary for this domain
    fn set_entry_summary(&self, entry_summary: Arc<EntrySummary>);

    /// Debugging
    fn debug(&self, alt: bool) -> String;
}

impl<T: RoutingDomainDetail + ?Sized> HasDialInfoDetailList for T {
    fn dial_info_detail_list(&self) -> &[DialInfoDetail] {
        self.dial_info_details()
    }
}
