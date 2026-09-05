use super::*;

impl_veilid_log_facility!("rtab");

/// Local Network routing domain internals
pub struct LocalNetworkRoutingDomainDetail {
    /// Registry accessor
    registry: VeilidComponentRegistry,
    /// The interface networks that are in this domain
    interface_addresses: Vec<IfAddr>,
    /// Common implementation for all routing domains
    common: RoutingDomainDetailCommon,
    /// Last relay requirements for this routing domain
    opt_last_relay_requirements: ArcSwapOption<RelayRequirements>,
}

impl fmt::Debug for LocalNetworkRoutingDomainDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalNetworkRoutingDomainDetail")
            // .field("registry", &self.registry)
            .field("interface_addresses", &self.interface_addresses)
            .field("common", &self.common)
            .field(
                "opt_last_relay_requirements",
                &self.opt_last_relay_requirements,
            )
            .finish()
    }
}

impl fmt::Display for LocalNetworkRoutingDomainDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Interface Addresses:\n{}\nLast Relay Requirements:\n{}\n{}",
            indent_all_string(
                f.to_multiline_indexed_string(
                    self.interface_addresses
                        .iter()
                        .map(|intf_addr| f.to_string(intf_addr))
                )
                .string_if_empty("None")
            ),
            indent_all_string(f.to_string_opt(self.opt_last_relay_requirements.load().as_ref())),
            f.to_string(&self.common)
        )
    }
}
impl_veilid_component_accessors!(LocalNetworkRoutingDomainDetail);

impl LocalNetworkRoutingDomainDetail {
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            interface_addresses: Default::default(),
            common: RoutingDomainDetailCommon::new(registry.clone(), RoutingDomain::LocalNetwork),
            opt_last_relay_requirements: ArcSwapOption::empty(),
            registry,
        }
    }
}

impl LocalNetworkRoutingDomainDetail {
    #[expect(dead_code)]
    pub fn interface_addresses(&self) -> Vec<IfAddr> {
        self.interface_addresses.clone()
    }

    pub fn set_interface_addresses(&mut self, mut interface_addresses: Vec<IfAddr>) -> bool {
        // Filter out any networks that are publicly routable as the routing domains should not overlap
        interface_addresses.retain(|x| Address::from_ip_addr(x.ip()).is_local());
        interface_addresses.sort_unstable();
        if interface_addresses == self.interface_addresses {
            return false;
        }
        self.interface_addresses = interface_addresses;
        true
    }
}

impl RoutingDomainDetailAccessors for LocalNetworkRoutingDomainDetail {
    fn common(&self) -> &RoutingDomainDetailCommon {
        &self.common
    }
    fn common_mut(&mut self) -> &mut RoutingDomainDetailCommon {
        &mut self.common
    }
}

impl RoutingDomainDetail for LocalNetworkRoutingDomainDetail {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
        self
    }
    fn routing_domain(&self) -> RoutingDomain {
        RoutingDomain::LocalNetwork
    }

    fn relay_requirements(&self) -> Arc<RelayRequirements> {
        {
            let opt_relay_status_guard = self.opt_last_relay_requirements.load();
            if let Some(relay_status) = opt_relay_status_guard.as_ref().map(|x| x.clone()) {
                return relay_status;
            }
        }

        let relay_status = RelayRequirements::new(self);
        self.opt_last_relay_requirements
            .store(Some(relay_status.clone()));
        relay_status
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
        RoutingDomain::LocalNetwork.into()
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
        if address.is_global() {
            return false;
        }

        let ip = address.ip_addr();
        for localnet in &self.interface_addresses {
            if ipaddr_in_network(ip, localnet.network().ip(), localnet.netmask()) {
                return true;
            }
        }

        // Explicitly allow loopback addresses in local network routing domain to permit proxying
        if ipaddr_is_loopback(&ip) {
            return true;
        }

        false
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
