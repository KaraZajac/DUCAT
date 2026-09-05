use super::*;

impl_veilid_log_facility!("rtab");

/// The current node's relaying requirements for the routing domain
/// Used by the relay management task to determine what relays are needed
/// The output of that task is the list of RoutingDomainRelay and RoutingDomainRelayState
#[derive(Debug)]
pub struct RelayRequirements {
    /// Routing domain this is for
    pub routing_domain: RoutingDomain,
    /// Low level port info for this node
    /// This is which ports are mapped externally that we may need keepalive pings for
    pub low_level_port_info: LowLevelPortInfo,
    /// This node's outbound dial info filter
    /// Used to determine if a relay's dialinfo is directly reachable
    pub dial_info_filter: DialInfoFilter,
    /// All transport types requiring inbound relays for this node
    pub need_relay_transports: HashSet<TransportType>,
    /// Ordering modes we still need for relaying, per address type
    pub need_relay_orderings: HashSet<(SequenceOrdering, AddressType)>,
    /// All the low level protocols and ports that require nat keepalive pings
    pub need_nat_keepalives: LowLevelProtocolPorts,
}

impl fmt::Display for RelayRequirements {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Low Level Port Info:\n{}\nDial Info Filter: {}\nNeed Relay Transports: {}\nNeed Relay Orderings: {}\nNeed NAT Keepalives: {}",
            indent_all_string(f.to_string(&self.low_level_port_info)),
            f.to_string(self.dial_info_filter),
            self.need_relay_transports.iter().map(|tt| f.to_string(tt)).collect::<Vec<_>>().join(", ").string_if_empty("None"),
            self.need_relay_orderings.iter().map(|(om,at)| format!("{}:{}", f.to_string(om), f.to_string(at))).collect::<Vec<_>>().join(", ").string_if_empty("None"),
            self.need_nat_keepalives.iter().map(|(lpt,at,p)| format!("{}:{}:{}", f.to_string(lpt), f.to_string(at), p)).collect::<Vec<_>>().join(", ").string_if_empty("None"),
        )
    }
}

impl RelayRequirements {
    pub fn new(rdd: &dyn RoutingDomainDetail) -> Arc<Self> {
        let outbound_protocols = rdd.outbound_protocols();
        let address_types = rdd.address_types();
        let routing_domain = rdd.routing_domain();
        let translated_address_types = rdd.translated_address_types();
        let low_level_port_info = rdd.get_low_level_port_info();
        let dial_info_filter = DialInfoFilter::all()
            .with_protocol_type_set(outbound_protocols)
            .with_address_type_set(address_types);

        // Get the dial info list in preferred deterministic order
        let mut dial_info_list = rdd.dial_info_details().clone();
        dial_info_list.sort_unstable_by(DialInfoDetail::ordered_sequencing_sort);

        // Start with all possible combinations we might need to have relays for
        let mut need_relay_transports = HashSet::<TransportType>::new();
        let mut need_relay_orderings = HashSet::<(SequenceOrdering, AddressType)>::new();
        for at in AddressTypeSet::all() {
            for pt in ProtocolTypeSet::all() {
                need_relay_transports.insert(TransportType::new(pt, at));
                need_relay_orderings.insert((pt.sequence_ordering(), at));
            }
        }

        // Figure out which dial info combinations we have that are direct-capable
        let mut need_nat_keepalives = LowLevelProtocolPorts::new();
        for did in dial_info_list {
            let pt = did.dial_info.protocol_type();
            let at = did.dial_info.address_type();

            // If this address type is NAT'd, then we need a relay for its
            // protocols even if it is directly reachable
            let wants_relay = did.class.requires_signal() || translated_address_types.contains(at);

            // Remove this protocol+address type combination from our requirements if we don't want a relay for it
            if !wants_relay {
                need_relay_transports.remove(&TransportType::new(pt, at));
                need_relay_orderings.remove(&(pt.sequence_ordering(), at));
            }

            // If this dial info class wants a NAT keepalive, then we need to keep a track of it
            if did.class.wants_nat_keepalive() {
                need_nat_keepalives.insert((
                    pt.low_level_protocol_type(),
                    at,
                    did.dial_info.port(),
                ));
            }
        }

        Arc::new(RelayRequirements {
            routing_domain,
            low_level_port_info,
            dial_info_filter,
            need_relay_transports,
            need_relay_orderings,
            need_nat_keepalives,
        })
    }

    /// Check if we need relays at all to satisfy these requirements
    pub fn needs_relays(&self) -> bool {
        !self.need_relay_transports.is_empty()
            || !self.need_relay_orderings.is_empty()
            || !self.need_nat_keepalives.is_empty()
    }

    /// Check if this relay requirements is equivalent to another
    pub fn equivalent(&self, other: &RelayRequirements) -> bool {
        self.routing_domain == other.routing_domain
            && self.low_level_port_info == other.low_level_port_info
            && self.dial_info_filter == other.dial_info_filter
            && self.need_relay_transports == other.need_relay_transports
            && self.need_relay_orderings == other.need_relay_orderings
            && self.need_nat_keepalives == other.need_nat_keepalives
    }

    /// Make a relay compiler for these relay requirements
    /// Starts off with no relays. Add relays to the builder and it tells you when it is satisfied.
    pub fn make_relay_compiler(self: Arc<Self>) -> RelayCompiler {
        RelayCompiler {
            requirements: self.clone(),
            want_relay_transports: self.need_relay_transports.clone(),
            want_relay_orderings: self.need_relay_orderings.clone(),
            want_nat_keepalives: self.need_nat_keepalives.clone(),
            relays: vec![],
        }
    }
}

/// Builder for a list of relays that satisfy the requirements
pub struct RelayCompiler {
    /// Relay requirements we are trying to satisfy
    pub requirements: Arc<RelayRequirements>,
    /// All transport types requiring inbound relays for this node
    pub want_relay_transports: HashSet<TransportType>,
    /// Ordering modes we still need for relaying, per address type
    pub want_relay_orderings: HashSet<(SequenceOrdering, AddressType)>,
    /// All the low level protocols and ports that require nat keepalive pings
    pub want_nat_keepalives: LowLevelProtocolPorts,
    /// All of the relays and their configuration currently included in our requirements
    pub relays: Vec<RoutingDomainRelay>,
}

impl fmt::Display for RelayCompiler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Requirements:\n{}\nWant Relay Transports: {}\nWant Relay Orderings: {}\nWant NAT Keepalives: {}\nRelays:\n{}",
            indent_all_string(f.to_string(&self.requirements)),
            self.want_relay_transports.iter().map(|tt| f.to_string(tt)).collect::<Vec<_>>().join(", ").string_if_empty("None"),
            self.want_relay_orderings.iter().map(|(om,at)| format!("{}:{}", f.to_string(om), f.to_string(at))).collect::<Vec<_>>().join(", ").string_if_empty("None"),
            self.want_nat_keepalives.iter().map(|(lpt,at,p)| format!("{}:{}:{}", f.to_string(lpt), f.to_string(at), p)).collect::<Vec<_>>().join(", ").string_if_empty("None"),
            indent_all_string(f.to_multiline_indexed_string(self.relays.iter()).string_if_empty("None"))
        )
    }
}

impl RelayCompiler {
    /// Remove a relay's capabilities from our current requirements and determine which
    /// pings should be performed.
    /// Returns true if the relay met some requirements, or false if applying the relay had no effect
    pub fn apply_relay(&mut self, mut relay: RoutingDomainRelay) -> bool {
        // Make sure this relay is the correct routing domain and has peer info
        let Some(relay_peer_info) = relay
            .relay_node
            .get_peer_info(self.requirements.routing_domain)
        else {
            return false;
        };

        // Clear out the dial info details and the pings because we'll add new ones
        relay.dial_info_details.clear();
        relay.pings.clear();

        // For all for the relay's dial info, see if it matches a protocol+address type we need covered
        let mut dial_info_list = relay_peer_info.node_info().dial_info_detail_list().to_vec();
        dial_info_list.sort_unstable_by(DialInfoDetail::ordered_sequencing_sort);

        // Determine for this relay, if there are dialinfo that are reachable with our node's
        // dialinfo filter, and which ordering modes can be satisfied by those flows

        let mut possible_ordering_modes = SequenceOrderingSet::new();
        for did in &dial_info_list {
            if did.class.requires_signal() {
                continue;
            }
            // If this dial info can be contacted directly, then it can be used for receiving
            // relaying and satsifying an ordering mode
            if did
                .dial_info
                .matches_filter(&self.requirements.dial_info_filter)
            {
                possible_ordering_modes.insert(did.dial_info.protocol_type().sequence_ordering());
            }
        }

        // If we did not get a single ordering mode we need for relaying, then this relay is disqualified
        // because we can't connect to it with our outbound protocols/address types directly
        if possible_ordering_modes.is_empty() {
            return false;
        }

        let mut useful = false;

        // Determine relay dial infos we can use from this relay out of our set of needed relay combinations
        // Builds up a set of needed ordering modes to keep flows open for the dial infos we are getting relayed
        for did in &dial_info_list {
            if did.class.requires_signal() {
                continue;
            }
            let didtt =
                TransportType::new(did.dial_info.protocol_type(), did.dial_info.address_type());

            if self.want_relay_transports.remove(&didtt) {
                // Still needed this transport type
                useful = true;

                // Mark this dial info as one we're using
                relay.dial_info_details.push(did.clone());

                // Mark this ordering mode as satisfied
                self.want_relay_orderings.remove(&(
                    didtt.protocol_type().sequence_ordering(),
                    didtt.address_type(),
                ));
            }
        }

        // Collect pings we can use from this relay
        for did in &dial_info_list {
            if did.class.requires_signal() {
                continue;
            }
            let didtt =
                TransportType::new(did.dial_info.protocol_type(), did.dial_info.address_type());

            // If this dial info can be contacted directly, then it is a ping candidate
            if did
                .dial_info
                .matches_filter(&self.requirements.dial_info_filter)
            {
                // See if we should add this ping
                let mut add_ping = false;

                // See if we should add the ping for ordering mode coverage
                let ordering = didtt.protocol_type().sequence_ordering();
                add_ping |= possible_ordering_modes.remove(ordering);

                // See if we should add the ping for low level port mapping coverage
                if let Some((llpt, port)) = self
                    .requirements
                    .low_level_port_info
                    .protocol_to_port
                    .get(&didtt)
                    .copied()
                {
                    let wnk = (llpt, didtt.address_type(), port);
                    add_ping |= self.want_nat_keepalives.remove(&wnk);
                }

                // Add the ping if we determined we could use it
                if add_ping {
                    relay.pings.push(RelayPing {
                        node_ref: relay.relay_node.unfiltered(),
                        dial_info: did.dial_info.clone(),
                    });
                }
            }
        }

        // Add a relay info to our list if it turned out to be useful
        if useful {
            self.relays.push(relay);
        }

        useful
    }

    /// Check if we want more relays
    /// Beyond the bare minimum ordering mode relays, having relays to handle
    /// each protocol+address type combination we can't accept directly are also wanted
    pub fn want_more_relays(&self) -> bool {
        // If we want more keepalives for NAT, we want more relays
        if !self.want_nat_keepalives.is_empty() {
            return true;
        }
        // If there are any protocol/address type combinations we need
        // relaying for still, we want more relays
        !self.want_relay_transports.is_empty()
    }

    /// Check if we can publish the relays we have
    /// This is a looser check than want_more_relays() because the bare minimum
    /// relays we need to publish are a subset of the relays we want, one relay per ordering mode
    /// per address type is enough to publish.
    fn can_publish_relays(&self) -> bool {
        // If we want more keepalives for NAT, we need more relays and can't publish yet
        if !self.want_nat_keepalives.is_empty() {
            return false;
        }

        // If we have any address types that still have ordering modes they need
        // relays for, then we can't publish yet
        self.want_relay_orderings.is_empty()
    }

    /// Get the list of relays we have built up so far
    /// Keeps the ordering of the relays we added them in so multiple builds can be consistent
    /// If not enough relays have been added to satisfy publication requirements, None is returned
    pub fn compile(&self) -> Option<RelayCompilation> {
        if self.can_publish_relays() {
            Some(RelayCompilation {
                requirements: self.requirements.clone(),
                relays: self.relays.clone().into(),
            })
        } else {
            None
        }
    }
}

/// The final list of relays that satisfies a set of relay requirements
#[derive(Debug, Clone)]
pub struct RelayCompilation {
    /// The relay requirements that were used to build this list
    pub requirements: Arc<RelayRequirements>,
    /// The list of relays that satisfies the requirements
    pub relays: Arc<[RoutingDomainRelay]>,
}

impl fmt::Display for RelayCompilation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Requirements:\n{}\nRelays:\n{}",
            indent_all_string(f.to_string(&self.requirements)),
            indent_all_string(
                f.to_multiline_indexed_string(self.relays.iter())
                    .string_if_empty("None")
            )
        )
    }
}

impl RelayCompilation {
    /// Check if this relay compilation is equivalent to another
    pub fn equivalent(&self, other: &RelayCompilation) -> bool {
        self.requirements.equivalent(&other.requirements)
            && self.relays.len() == other.relays.len()
            && self
                .relays
                .iter()
                .zip(other.relays.iter())
                .all(|(a, b)| a.equivalent(b))
    }
}
