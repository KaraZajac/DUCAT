use super::*;

impl_veilid_log_facility!("rtab");

pub trait RoutingDomainDetailAccessors: RoutingDomainDetail {
    #[expect(dead_code)]
    fn common(&self) -> &RoutingDomainDetailCommon;
    fn common_mut(&mut self) -> &mut RoutingDomainDetailCommon;
}

pub struct RoutingDomainDetailCommon {
    /// The registry accessor for this routing domain detail
    registry: VeilidComponentRegistry,
    /// The routing domain identifier for this routing domain detail
    routing_domain: RoutingDomain,
    /// The network configuration for this routing domain
    network_config: RoutingDomainNetworkConfig,
    /// The output of the last relay compilation for this domain
    opt_relay_compilation: Option<RelayCompilation>,
    /// Relay states we are tracking for the compiled relay list
    relay_states: HashMap<NodeId, RoutingDomainRelayState>,
    /// The dial info details that are inbound reachable for this node in this domain
    dial_info_details: Vec<DialInfoDetail>,
    /// Dial info detail has been confirmed by the network manager as publishable/reachable
    confirmed: bool,
    /// The minimum number of nodes in the is routing domain since the last low water mark recording point
    /// Pair of (reset, low water mark)
    low_water_mark: Mutex<(bool, Arc<LowWaterMark>)>,
    /// The current peer info so we don't have recalculate it every time it is requested
    current_peer_info_cache: Mutex<Option<Arc<PeerInfo>>>,
    /// The last known bootstrap peers that seeded our routing table
    bootstrap_peers: Mutex<Vec<NodeRef>>,
    /// The last calculated summary of the routing table entries in this domain
    entry_summary: Mutex<Arc<EntrySummary>>,
}

impl fmt::Debug for RoutingDomainDetailCommon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoutingDomainDetailCommon")
            .field("routing_domain", &self.routing_domain)
            .field("network_config", &self.network_config)
            .field("opt_relay_compilation", &self.opt_relay_compilation)
            .field("relay_states", &self.relay_states)
            .field("dial_info_details", &self.dial_info_details)
            .field("confirmed", &self.confirmed)
            .field("low_water_mark", &self.low_water_mark)
            .field("current_peer_info_cache", &self.current_peer_info_cache)
            .field("bootstrap_peers", &self.bootstrap_peers)
            .field("entry_summary", &self.entry_summary)
            .finish()
    }
}

impl fmt::Display for RoutingDomainDetailCommon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Network Config:{}\nRelay Compilation:\n{}\nRelay States:\n{}\nDial Info Details:\n{}\nConfirmed: {}\nLow Water Mark: {}\nBootstrap Peers: {}\nEntry Summary:\n{}",
            if f.alternate() {
                format!("\n{}",indent_all_string(f.to_string(&self.network_config)))
            } else {
                format!(" {}",f.to_string(&self.network_config))
            },
            indent_all_string(f.to_string_opt(self.opt_relay_compilation.as_ref())),
            indent_all_string(f.to_multiline_indexed_string(self.relay_states.iter().map(|(k, v)| format!("{}: {}", f.to_string(k), f.to_string(v)))).string_if_empty("None")),
            indent_all_string(f.to_multiline_indexed_string(self.dial_info_details.iter().map(|d| f.to_string(d))).string_if_empty("None")),
            f.to_string(self.confirmed),
            f.to_string(self.low_water_mark.lock().1.clone()),
            self.bootstrap_peers.lock().clone().iter().map(|n| f.to_string(n)).collect::<Vec<_>>().join(",").string_if_empty("None"),
            indent_all_string(f.to_string(self.entry_summary.lock().clone()))
        )
    }
}

impl_veilid_component_accessors!(RoutingDomainDetailCommon);

impl RoutingDomainDetailCommon {
    pub fn new(registry: VeilidComponentRegistry, routing_domain: RoutingDomain) -> Self {
        Self {
            registry,
            routing_domain,
            network_config: Default::default(),
            opt_relay_compilation: None,
            relay_states: Default::default(),
            dial_info_details: Default::default(),
            confirmed: false,
            low_water_mark: Mutex::new((true, Default::default())),
            current_peer_info_cache: Mutex::new(Default::default()),
            bootstrap_peers: Mutex::new(Default::default()),
            entry_summary: Mutex::new(Default::default()),
        }
    }

    ///////////////////////////////////////////////////////////////////////
    // Accessors

    pub fn confirmed(&self) -> bool {
        self.confirmed
    }

    pub fn network_config(&self) -> &RoutingDomainNetworkConfig {
        &self.network_config
    }

    pub fn outbound_protocols(&self) -> ProtocolTypeSet {
        self.network_config.outbound_protocols
    }

    pub fn inbound_protocols(&self) -> ProtocolTypeSet {
        self.network_config.inbound_protocols
    }

    pub fn address_types(&self) -> AddressTypeSet {
        self.network_config.address_types
    }

    pub fn capabilities(&self) -> BTreeSet<VeilidCapability> {
        self.network_config.capabilities.clone()
    }

    pub fn get_bootstrap_peers(&self) -> Vec<NodeRef> {
        self.bootstrap_peers.lock().clone()
    }

    pub fn clear_bootstrap_peers(&self) {
        self.bootstrap_peers.lock().clear()
    }

    pub fn add_bootstrap_peer(&self, bootstrap_peer: NodeRef) {
        let mut bootstrap_peers = self.bootstrap_peers.lock();
        bootstrap_peers.push(bootstrap_peer);
    }

    pub fn update_low_water_mark(&self, mut low_water_mark: Arc<LowWaterMark>) {
        let mut lwm = self.low_water_mark.lock();
        let (reset, last_low_water_mark) = lwm.clone();

        if !reset {
            let mut new_low_water_mark = low_water_mark.as_ref().clone();
            new_low_water_mark.merge(&last_low_water_mark);
            low_water_mark = Arc::new(new_low_water_mark);
        }

        if last_low_water_mark != low_water_mark {
            veilid_log!(self debug "[{:#}] changed low water mark from {} to {}", self.routing_domain, last_low_water_mark, low_water_mark);
            *lwm = (false, low_water_mark);
        }
    }

    pub fn reset_low_water_mark(&self) {
        self.low_water_mark.lock().0 = true;
    }

    pub fn get_low_water_mark(&self) -> Arc<LowWaterMark> {
        self.low_water_mark.lock().1.clone()
    }

    pub fn relays(&self) -> Vec<RoutingDomainRelay> {
        self.opt_relay_compilation
            .as_ref()
            .map(|c| c.relays.to_vec())
            .unwrap_or_default()
    }

    pub fn relay_compilation(&self) -> Option<RelayCompilation> {
        self.opt_relay_compilation.clone()
    }

    pub fn relay_state(&self, relay_id: NodeId) -> Option<RoutingDomainRelayState> {
        self.relay_states.get(&relay_id).cloned()
    }

    pub fn dial_info_details(&self) -> &Vec<DialInfoDetail> {
        &self.dial_info_details
    }

    pub fn inbound_dial_info_filter(&self) -> DialInfoFilter {
        DialInfoFilter::all()
            .with_protocol_type_set(self.network_config.inbound_protocols)
            .with_address_type_set(self.network_config.address_types)
    }

    pub fn outbound_dial_info_filter(&self) -> DialInfoFilter {
        DialInfoFilter::all()
            .with_protocol_type_set(self.network_config.outbound_protocols)
            .with_address_type_set(self.network_config.address_types)
    }

    pub fn get_current_peer_info(&self) -> Arc<PeerInfo> {
        let mut cpi = self.current_peer_info_cache.lock();
        if cpi.is_none() {
            // Regenerate peer info
            let pi = self.make_current_peer_info();

            // Cache the peer info
            *cpi = Some(Arc::new(pi));
        }
        cpi.as_ref().unwrap_or_log().clone()
    }

    pub fn get_entry_summary(&self) -> Arc<EntrySummary> {
        self.entry_summary.lock().clone()
    }

    pub fn set_entry_summary(&self, entry_summary: Arc<EntrySummary>) {
        *self.entry_summary.lock() = entry_summary;
    }

    ///////////////////////////////////////////////////////////////////////
    // Mutators

    pub fn set_network_config(&mut self, network_config: RoutingDomainNetworkConfig) {
        let changed = self.network_config != network_config;
        if changed {
            self.network_config = network_config;

            self.clear_current_peer_info_cache();
        }
    }

    pub fn set_confirmed(&mut self, confirmed: bool) {
        self.confirmed = confirmed;
    }

    pub fn clear_dial_info_details(
        &mut self,
        address_types: Option<AddressTypeSet>,
        protocol_types: Option<ProtocolTypeSet>,
    ) {
        let mut changed = false;
        self.dial_info_details.retain_mut(|e| {
            let mut remove = true;
            if let Some(pt) = protocol_types {
                if !pt.contains(e.dial_info.protocol_type()) {
                    remove = false;
                }
            }
            if let Some(at) = address_types {
                if !at.contains(e.dial_info.address_type()) {
                    remove = false;
                }
            }

            changed = changed || remove;
            !remove
        });

        if changed {
            self.dial_info_details.sort_unstable();
            self.dial_info_details.dedup();

            self.clear_current_peer_info_cache();
        }
    }
    pub fn add_dial_info_detail(&mut self, did: DialInfoDetail) {
        let changed = !self.dial_info_details.contains(&did);
        if changed {
            self.dial_info_details.push(did);
            self.dial_info_details.sort_unstable();
            self.dial_info_details.dedup();

            self.clear_current_peer_info_cache();
        }
    }
    // pub fn remove_dial_info_detail(&mut self, did: DialInfoDetail) {
    //     let mut changed = false;
    //     self.dial_info_details.retain(|e| {
    //         let remove = e == &did;
    //         changed = changed || remove;
    //         !remove
    //     });
    //     if changed {
    //         self.clear_current_peer_info_cache();
    //     }
    // }

    pub fn set_relay_compilation(&mut self, opt_relay_compilation: Option<RelayCompilation>) {
        // See if the relays being set are equivalent and in the same order
        match (
            self.opt_relay_compilation.as_ref(),
            opt_relay_compilation.as_ref(),
        ) {
            (opt_existing_compilation, Some(new_compilation)) => {
                // Check if existing is equivalent to new
                if opt_existing_compilation
                    .as_ref()
                    .map(|existing_compilation| existing_compilation.equivalent(new_compilation))
                    .unwrap_or(false)
                {
                    return;
                }

                // Get the new relay ids
                let new_relay_ids = new_compilation
                    .relays
                    .iter()
                    .map(|r| r.relay_id.clone())
                    .collect::<HashSet<_>>();

                // Drop any relay states that are no longer in the new compilation, and add default ones for the new relays
                // (not really necessary for opt_existing_compilation.is_none(), but as it is harmless, we can do this for simplicity)
                self.relay_states.retain(|k, _| new_relay_ids.contains(k));

                // Add new relay states for the new relays
                let cur_ts = Timestamp::now_non_decreasing();
                for relay_id in new_relay_ids {
                    self.relay_states
                        .entry(relay_id)
                        .or_insert_with(|| RoutingDomainRelayState {
                            last_keepalive: cur_ts,
                            last_optimized: cur_ts,
                        });
                }
            }
            (None, None) => {
                // None is equivalent to None, no state changed needed
                return;
            }
            (Some(_), None) => {
                // Relays are no longer required, so clear the relay states
                self.relay_states.clear();
            }
        }

        // Save the new relays and clear the current peer info cache since we'll need to rebuild that
        self.opt_relay_compilation = opt_relay_compilation;
        self.clear_current_peer_info_cache();
    }

    // Update a single relay's state
    pub fn set_relay_state(&mut self, relay_id: NodeId, state: RoutingDomainRelayState) {
        if let Some(rc) = self.opt_relay_compilation.as_ref() {
            if rc.relays.iter().any(|r| r.relay_id == relay_id) {
                self.relay_states.insert(relay_id, state);
            }
        }
    }

    pub fn clear_current_peer_info_cache(&self) {
        *self.current_peer_info_cache.lock() = None;
    }

    //////////////////////////////////////////////////////////////////////////////
    // Internal functions

    fn make_current_peer_info(&self) -> PeerInfo {
        let routing_table = self.routing_table();
        let cur_ts = Timestamp::now_non_decreasing();
        let mut relay_info_list = vec![];
        for relay in self.relays() {
            let relay_node_ids = relay.relay_node.node_ids();
            let Some(relay_node_info) = relay.relay_node.node_info(self.routing_domain) else {
                veilid_log!(self debug "not including relay node {} in peer info for routing domain {:?}", relay_node_ids, self.routing_domain);
                continue;
            };
            let relay_info = RelayInfo::new(
                relay_node_info.timestamp(),
                relay_node_ids,
                relay_node_info.outbound_protocols(),
                relay_node_info.address_types(),
                relay.dial_info_details,
                relay.relay_kind,
            );
            relay_info_list.push(relay_info);
        }

        let keypairs = routing_table.signing_key_pairs();
        let public_keys: Vec<_> = keypairs.iter().map(|x| x.key()).collect();
        let secret_keys =
            SecretKeyGroup::from(keypairs.iter().map(|x| x.secret()).collect::<Vec<_>>());
        let crypto_info_list: Vec<_> = public_keys
            .iter()
            .map(|pk| match pk.kind() {
                CRYPTO_KIND_VLD0 => CryptoInfo::VLD0 {
                    public_key: pk.value(),
                },
                _ => {
                    unimplemented!("Must implement cryptoinfo")
                }
            })
            .collect();

        let node_info = NodeInfo::new(
            cur_ts,
            VALID_ENVELOPE_VERSIONS.to_vec(),
            crypto_info_list,
            self.network_config.capabilities.iter().copied().collect(),
            self.network_config.outbound_protocols,
            self.network_config.address_types,
            self.dial_info_details.clone(),
            relay_info_list,
        );

        PeerInfo::new_from_node_info(&routing_table, self.routing_domain, &secret_keys, node_info)
            .expect_or_log("our own peerinfo should never fail")
    }
}
