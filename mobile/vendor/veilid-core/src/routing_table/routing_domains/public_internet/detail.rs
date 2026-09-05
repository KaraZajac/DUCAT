use super::*;

impl_veilid_log_facility!("rtab");

/// Drop a relay whose protected connections survive less than this on average.
pub(super) const RELAY_MIN_PROTECTED_CONNECTION_TM90: TimestampDuration =
    TimestampDuration::new_secs(20);

/// Public Internet routing domain internals
pub struct PublicInternetRoutingDomainDetail {
    /// Registry accessor
    registry: VeilidComponentRegistry,
    /// The interface networks that are in this domain
    interface_addresses: Vec<IfAddr>,
    /// Common implementation for all routing domains
    common: RoutingDomainDetailCommon,
    /// Published peer info for this routing domain
    published_peer_info: Mutex<Option<Arc<PeerInfo>>>,
    /// Last relay requirements for this routing domain
    opt_last_relay_requirements: ArcSwapOption<RelayRequirements>,
}

impl fmt::Debug for PublicInternetRoutingDomainDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublicInternetRoutingDomainDetail")
            // .field("registry", &self.registry)
            .field("interface_addresses", &self.interface_addresses)
            .field("common", &self.common)
            .field("published_peer_info", &self.published_peer_info)
            .field(
                "opt_last_relay_requirements",
                &self.opt_last_relay_requirements,
            )
            .finish()
    }
}

impl fmt::Display for PublicInternetRoutingDomainDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Interface Addresses:\n{}\nPublished Peer Info:\n{}\nLast Relay Requirements:\n{}\n{}",
            indent_all_string(
                f.to_multiline_indexed_string(
                    self.interface_addresses
                        .iter()
                        .map(|intf_addr| f.to_string(intf_addr))
                )
                .string_if_empty("None")
            ),
            indent_all_string(f.to_string_opt(self.published_peer_info.lock().as_ref())),
            indent_all_string(f.to_string_opt(self.opt_last_relay_requirements.load().as_ref())),
            f.to_string(&self.common)
        )
    }
}

impl_veilid_component_accessors!(PublicInternetRoutingDomainDetail);

impl RoutingDomainDetailAccessors for PublicInternetRoutingDomainDetail {
    fn common(&self) -> &RoutingDomainDetailCommon {
        &self.common
    }
    fn common_mut(&mut self) -> &mut RoutingDomainDetailCommon {
        &mut self.common
    }
}

impl PublicInternetRoutingDomainDetail {
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            interface_addresses: Default::default(),
            common: RoutingDomainDetailCommon::new(registry.clone(), RoutingDomain::PublicInternet),
            published_peer_info: Default::default(),
            opt_last_relay_requirements: ArcSwapOption::new(None),
            registry,
        }
    }

    #[expect(dead_code)]
    pub fn interface_addresses(&self) -> Vec<IfAddr> {
        self.interface_addresses.clone()
    }

    pub fn set_interface_addresses(&mut self, mut interface_addresses: Vec<IfAddr>) -> bool {
        // Filter out any networks that are only locally routable as the routing domains should not overlap
        interface_addresses.retain(|x| Address::from_ip_addr(x.ip()).is_global());
        interface_addresses.sort_unstable();
        if interface_addresses == self.interface_addresses {
            return false;
        }
        self.interface_addresses = interface_addresses;
        true
    }

    pub fn make_relay_node_filter(&self) -> impl Fn(&BucketEntrySnapshot) -> bool {
        let ip6_prefix_size = self
            .config()
            .internal()
            .network
            .max_connections_per_ip6_prefix_size as usize;

        // Get all our outbound protocol/address types
        let outbound_dif = self.outbound_dial_info_filter();

        // Get our own peer info
        let own_peer_info = self.get_peer_info();

        move |snap: &BucketEntrySnapshot| {
            // Defensively exclude nodes that aren't responsive
            if !snap.state.is_responsive() {
                return false;
            }

            // Ensure this node is not on the local network
            if snap
                .routing_domain_set()
                .contains(RoutingDomain::LocalNetwork)
            {
                return false;
            }

            // Exclude any nodes that don't have a 'best node id' for our enabled cryptosystems
            if snap.best_node_id().is_none() {
                return false;
            }

            // Exclude nodes whose past protected connections died too fast.
            if let Some(drop_stats) = &snap.connection_stats.protected_drop_span {
                if drop_stats.tm90 < RELAY_MIN_PROTECTED_CONNECTION_TM90 {
                    return false;
                }
            }

            // Get the public internet peer info so we can validate it
            let Some(peer_info) = snap.get_peer_info(RoutingDomain::PublicInternet) else {
                return false;
            };

            // Exclude any nodes that are relaying directly through us
            if own_peer_info
                .node_ids()
                .contains_any_from_iter(peer_info.node_info().relay_ids().iter())
            {
                return false;
            }

            // Disqualify nodes that don't have relay capability
            if !peer_info
                .node_info()
                .has_capability(VEILID_CAPABILITY_RELAY)
            {
                return false;
            }

            // Disqualify any nodes that don't speak all of the envelope versions we do
            let peer_envelope_support = peer_info.node_info().envelope_support();
            if own_peer_info
                .node_info()
                .envelope_support()
                .iter()
                .copied()
                .any(|x| !peer_envelope_support.contains(&x))
            {
                return false;
            }

            // Ensure a transport we can reach this relay through is currently healthy.
            let mut has_reachable_transport = false;
            for did in peer_info.node_info().dial_info_detail_list() {
                if did.class.requires_signal() {
                    continue;
                }
                if !did.dial_info.matches_filter(&outbound_dif) {
                    continue;
                }
                let t: TransportType = (&did.dial_info).into();
                let healthy = snap.per_transport.get(&t).is_some_and(|stats| {
                    stats.first_steady_answer_ts.is_some()
                        && stats.unreachable == 0
                        && stats.failed_to_send == 0
                        && stats.recent_lost_questions == 0
                });
                if healthy {
                    has_reachable_transport = true;
                    break;
                }
            }

            if !has_reachable_transport {
                return false;
            }

            // Exclude any nodes that have our same network block
            if own_peer_info
                .node_info()
                .is_on_same_ipblock(peer_info.node_info(), ip6_prefix_size)
            {
                return false;
            }

            true
        }
    }
}

impl RoutingDomainDetail for PublicInternetRoutingDomainDetail {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
        self
    }

    fn routing_domain(&self) -> RoutingDomain {
        RoutingDomain::PublicInternet
    }

    fn relay_requirements(&self) -> Arc<RelayRequirements> {
        {
            let opt_relay_requirements_guard = self.opt_last_relay_requirements.load();
            if let Some(relay_requirements) =
                opt_relay_requirements_guard.as_ref().map(|x| x.clone())
            {
                return relay_requirements;
            }
        }

        let relay_requirements = RelayRequirements::new(self);
        self.opt_last_relay_requirements
            .store(Some(relay_requirements.clone()));
        relay_requirements
    }

    fn network_config(&self) -> &RoutingDomainNetworkConfig {
        self.common.network_config()
    }

    fn outbound_protocols(&self) -> ProtocolTypeSet {
        self.common.outbound_protocols()
    }
    fn inbound_protocols(&self) -> ProtocolTypeSet {
        self.common.inbound_protocols()
    }
    fn address_types(&self) -> AddressTypeSet {
        self.common.address_types()
    }
    fn origin_routing_domains(&self) -> RoutingDomainSet {
        RoutingDomain::LocalNetwork | RoutingDomain::PublicInternet
    }
    fn confirmed(&self) -> bool {
        self.common.confirmed()
    }
    fn capabilities(&self) -> BTreeSet<VeilidCapability> {
        self.common.capabilities()
    }

    fn relays(&self) -> Vec<RoutingDomainRelay> {
        self.common.relays()
    }
    fn relay_compilation(&self) -> Option<RelayCompilation> {
        self.common.relay_compilation()
    }
    fn relay_state(&self, relay_id: NodeId) -> Option<RoutingDomainRelayState> {
        self.common.relay_state(relay_id)
    }

    fn dial_info_details(&self) -> &Vec<DialInfoDetail> {
        self.common.dial_info_details()
    }

    fn translated_address_types(&self) -> AddressTypeSet {
        let mut inbound_address_types = HashMap::new();
        for did in self.dial_info_details() {
            inbound_address_types.insert(did.dial_info.ip_addr(), did.dial_info.address_type());
        }
        for intf_addr in &self.interface_addresses {
            inbound_address_types.remove(&intf_addr.ip());
        }
        inbound_address_types
            .into_values()
            .fold(AddressTypeSet::new(), |acc, at| acc | at)
    }

    fn inbound_dial_info_filter(&self) -> DialInfoFilter {
        self.common.inbound_dial_info_filter()
    }
    fn outbound_dial_info_filter(&self) -> DialInfoFilter {
        self.common.outbound_dial_info_filter()
    }

    fn get_peer_info(&self) -> Arc<PeerInfo> {
        self.common.get_current_peer_info()
    }

    fn get_bootstrap_peers(&self) -> Vec<NodeRef> {
        self.common.get_bootstrap_peers()
    }
    fn clear_bootstrap_peers(&self) {
        self.common.clear_bootstrap_peers();
    }
    fn add_bootstrap_peer(&self, bootstrap_peer: NodeRef) {
        self.common.add_bootstrap_peer(bootstrap_peer);
    }

    fn update_low_water_mark(&self, low_water_mark: Arc<LowWaterMark>) {
        self.common.update_low_water_mark(low_water_mark);
    }

    fn reset_low_water_mark(&self) {
        self.common.reset_low_water_mark();
    }

    fn get_low_water_mark(&self) -> Arc<LowWaterMark> {
        self.common.get_low_water_mark()
    }

    fn get_entry_summary(&self) -> Arc<EntrySummary> {
        self.common.get_entry_summary()
    }
    fn set_entry_summary(&self, live_entry_counts: Arc<EntrySummary>) {
        self.common.set_entry_summary(live_entry_counts);
    }

    fn can_contain_address(&self, address: Address) -> bool {
        address.is_global()
    }

    fn ensure_dial_info_is_valid(&self, dial_info: &DialInfo) -> bool {
        let address = dial_info.socket_address().address();
        let can_contain_address = self.can_contain_address(address);

        if !can_contain_address {
            return false;
        }
        if !dial_info.is_valid() {
            veilid_log!(self debug
                "shouldn't be registering invalid addresses: {:?}",
                dial_info
            );
            return false;
        }
        true
    }

    ////////////////////////////////////////////////

    fn invalidate(&self) {
        self.opt_last_relay_requirements.store(None);
        self.common.clear_current_peer_info_cache();
    }

    fn debug(&self, alt: bool) -> String {
        if alt {
            format!("{:#}", self)
        } else {
            format!("{}", self)
        }
    }
}
