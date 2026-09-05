use super::*;

impl_veilid_log_facility!("rtab");

/// Context for calculating contact method
pub struct ContactMethodContext {
    pub request: ContactMethodRequest,
    pub best_ck: CryptoKind,
    pub node_b_id: NodeId,
    pub a_to_b_direct_dids: Vec<DialInfoDetail>,
    pub a_to_b_signalled_dids: Vec<DialInfoDetail>,
    pub b_to_a_direct_dids: Vec<DialInfoDetail>,
    pub b_to_a_signalled_dids: Vec<DialInfoDetail>,
    pub node_b_reachable_relays: Vec<(RelayInfo, Vec<DialInfoDetail>)>,
    pub should_have_existing_connection: bool,
}
impl fmt::Debug for ContactMethodContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContactMethodContext")
            .field("request", &self.request)
            .field("best_ck", &self.best_ck)
            .field("node_b_id", &self.node_b_id)
            .field("a_to_b_direct_dids", &self.a_to_b_direct_dids)
            .field("a_to_b_signalled_dids", &self.a_to_b_signalled_dids)
            .field("b_to_a_direct_dids", &self.b_to_a_direct_dids)
            .field("b_to_a_signalled_dids", &self.b_to_a_signalled_dids)
            .field("node_b_relays", &self.node_b_reachable_relays)
            .field(
                "should_have_existing_connection",
                &self.should_have_existing_connection,
            )
            .finish()
    }
}
impl ContactMethodContext {
    pub fn node_a(&self) -> &NodeInfo {
        self.request.peer_a.node_info()
    }
    // pub fn node_a_ids(&self) -> &NodeIdGroup {
    //     self.request.peer_a.node_ids()
    // }
    // pub fn node_b(&self) -> &NodeInfo {
    //     self.request.peer_b.node_info()
    // }
    pub fn node_b_ids(&self) -> &NodeIdGroup {
        self.request.peer_b.node_ids()
    }
}

impl PublicInternetRoutingDomainController {
    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rtab", skip(self), fields(__VEILID_LOG_KEY = self.log_key()), ret))]
    pub fn get_contact_methods(&self, request: ContactMethodRequest) -> Vec<ContactMethod> {
        // Fill out the context with useful information for this request
        let Some(ctx) = self.make_contact_method_context(request) else {
            return vec![];
        };

        // Get all viable contact methods from node A to node B given the request constraints
        let mut out = Vec::<ContactMethod>::new();

        // Try 'existing' contact methods first, because if two nodes should already have a connection,
        // then we should prioritize those over creating a new one
        if ctx.should_have_existing_connection {
            // We only check for existing connections in the 'received' direction, because
            // if we just said 'existing' for the 'sent' direction, we would never be able to initiate a
            // connection, because we would be assuming the connection must already exist.
            // The 'sent' direction also supports -creating- a connection, and we have special casing for that
            // in the send_data_cm_* functions, to account for making connections with specific transports.
            out.push(ContactMethod::Existing);
        }

        // Try direct methods without signaling
        for target_did in &ctx.a_to_b_direct_dids {
            out.push(ContactMethod::Direct {
                target_di: target_did.dial_info.clone(),
            });
        }

        // Signalling methods require published peer info
        if ctx.request.peer_a_published {
            // If node A is direct-inbound capable, try reverse connections
            if !ctx.b_to_a_direct_dids.is_empty() {
                // Try reverse connections
                self.get_reverse_connection_contact_methods(&ctx, &mut out);
            }

            // If node A is signalling-inbound capable, try hole punching
            if !ctx.b_to_a_signalled_dids.is_empty() {
                // Try hole punching
                self.get_hole_punch_contact_methods(&ctx, &mut out);
            }
        }

        // Try inbound relaying
        self.get_inbound_relay_contact_methods(&ctx, &mut out);

        // Try outbound relaying
        self.get_outbound_relay_contact_methods(&ctx, &mut out);

        out
    }

    // Get dial info between nodes that don't have the same IP address regardless of port
    // The generic non-routing-domain-specific 'get_dial_info_details_between_nodes' only
    // filters out same ip-address-and-port brcause we want to avoid self-dialing due to
    // stale peerinfo from nodes at the same exact IP address.
    // Nodes on the PublicInternet routing domain will either be:
    // 1. Different public IP addresses (good, direct methods are fine)
    // 2. Same public IP addresses and same port (bad, never allow this)
    // 3. Same public IP addresses but different port:
    //   a. Same machine but different node (these connect via inbound relaying, so filter out here)
    //   b. Different machine but behind the same NAT (also uses inbound relaying, so filter out here)
    // That leaves us with filtering out any dial info with the same IP address, regardless of port.
    fn get_dial_info_details_between_different_address_nodes(
        &self,
        from_node: &NodeInfo,
        to_node: &dyn HasDialInfoDetailList,
        dial_info_filter: DialInfoFilter,
        sequencing: Sequencing,
    ) -> Vec<DialInfoDetail> {
        let from_node_addresses = from_node
            .dial_info_detail_list()
            .iter()
            .map(|did| did.dial_info.address())
            .collect::<BTreeSet<_>>();

        let mut direct_dids = self.get_dial_info_details_between_nodes(
            from_node,
            to_node,
            dial_info_filter,
            sequencing,
        );
        direct_dids.retain(|did| !from_node_addresses.contains(&did.dial_info.address()));
        direct_dids
    }

    // Precalculate everything we can to get all the contact methods quickly
    fn make_contact_method_context(
        &self,
        request: ContactMethodRequest,
    ) -> Option<ContactMethodContext> {
        // Get the nodeinfos for convenience
        let node_a = request.peer_a.node_info();
        let node_b = request.peer_b.node_info();

        // Get the node ids that would be used between these peers
        let cck = common_crypto_kinds(
            &request.peer_a.node_ids().kinds(),
            &request.peer_b.node_ids().kinds(),
        );
        let Some(best_ck) = cck.first().copied() else {
            // No common crypto kinds between these nodes, can't contact
            return None;
        };

        let node_b_id = request.peer_b.node_ids().get(best_ck).unwrap_or_log();

        // Get all dial info details between node A and node B,
        // and split list into direct and signalled dial info.
        // Uses the requested dial info filter and sequencing requirement.
        let mut a_to_b_direct_dids = self.get_dial_info_details_between_different_address_nodes(
            node_a,
            node_b,
            request.dial_info_filter,
            request.sequencing,
        );

        let a_to_b_signalled_dids = a_to_b_direct_dids
            .extract_if(.., |did| did.class.requires_signal())
            .collect();

        // Get all dial info details between node B and node A,
        // and split list into direct and signalled dial info.
        // Uses an open dial info filter because any transports that meet the
        // sequencing requirement can be used by the remote node.
        let mut b_to_a_direct_dids = self.get_dial_info_details_between_different_address_nodes(
            node_b,
            node_a,
            DialInfoFilter::all(),
            request.sequencing,
        );
        let b_to_a_signalled_dids = b_to_a_direct_dids
            .extract_if(.., |did| did.class.requires_signal())
            .collect();

        // Process all of node B's relays and determine any of them are
        // node A, and filter out any that don't meet the sequencing requirement
        let mut should_have_existing_connection = false;
        let node_b_reachable_relays = node_b
            .relay_info_list()
            .iter()
            .cloned()
            .filter_map(|node_b_relay| {
                // Skip relays that don't support the best crypto kind between the two nodes
                if !node_b_relay.node_ids().contains_kind(best_ck) {
                    // No best relay id
                    return None;
                };

                // Check if node_b_relay is node_a, in which case a connection should already exist
                // But we have to check if the sequencing matches our requirement before we can use it
                if node_b_relay
                    .node_ids()
                    .contains_any_from_iter(request.peer_a.node_ids().iter())
                    && node_b_relay.has_sequencing_matched_dial_info(request.sequencing)
                {
                    // A suitable existing connection should already exist
                    should_have_existing_connection = true;
                    return None;
                }

                // This relay is an inbound relaying candidate
                // For all of node B's relays figure out which can be contacted directly by node A.
                // We defensively strip any dialinfo that require a signal, but RelayInfo should never
                // have that, as it is also stripped when encode/decoded an never included in the first place.
                let node_b_relay_dids = self
                    .get_dial_info_details_between_different_address_nodes(
                        node_a,
                        &node_b_relay,
                        request.dial_info_filter,
                        request.sequencing,
                    )
                    .into_iter()
                    .filter(|did| !did.class.requires_signal())
                    .collect::<Vec<_>>();

                Some((node_b_relay, node_b_relay_dids))
            })
            .collect();

        let ctx = ContactMethodContext {
            request,
            best_ck,
            //node_a_id,
            node_b_id,
            a_to_b_direct_dids,
            a_to_b_signalled_dids,
            b_to_a_direct_dids,
            b_to_a_signalled_dids,
            node_b_reachable_relays,
            should_have_existing_connection,
        };

        Some(ctx)
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rtab", skip(self), fields(__VEILID_LOG_KEY = self.log_key()), ret))]
    fn get_reverse_connection_contact_methods(
        &self,
        ctx: &ContactMethodContext,
        out: &mut Vec<ContactMethod>,
    ) {
        // Can node A reach Node B's inbound relay(s) directly?
        for (_node_b_relay, node_b_relay_dids) in ctx.node_b_reachable_relays.iter() {
            // If so, there will be at last one relay dialinfo that meets the sequencing requirement
            for node_b_relay_did in node_b_relay_dids.iter() {
                // Reverse connection is possible from this node b relay
                // via at least one dialinfo that meets the sequencing requirement
                out.push(ContactMethod::SignalReverse {
                    relay_di: node_b_relay_did.dial_info.clone(),
                });
            }
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rtab", skip(self), fields(__VEILID_LOG_KEY = self.log_key()), ret))]
    fn get_hole_punch_contact_methods(
        &self,
        ctx: &ContactMethodContext,
        out: &mut Vec<ContactMethod>,
    ) {
        // Gather signalling dialinfo from node B's reachable relays
        let mut signalling_dids = Vec::new();

        // Can node A reach Node B's inbound relay(s) directly?
        for (_node_b_relay, node_b_relay_dids) in ctx.node_b_reachable_relays.iter() {
            // If so, there will be at last one relay dialinfo that meets the sequencing requirement
            for node_b_relay_did in node_b_relay_dids.iter() {
                // Add a signalling dialinfo to the list
                signalling_dids.push(node_b_relay_did.clone());
            }
        }

        // Collect the low-level udp dialinfo from node A to node B that require signal
        let mut target_udp_dids = ctx
            .a_to_b_signalled_dids
            .iter()
            .filter(|did| {
                did.dial_info.protocol_type().low_level_protocol_type() == LowLevelProtocolType::UDP
            })
            .collect::<Vec<_>>();
        let target_udp_transport_types = target_udp_dids
            .iter()
            .map(|did| did.dial_info.transport_type())
            .collect::<HashSet<_>>();

        // Collect the low-level udp dialinfo from node B to node A that require signal
        let mut reverse_udp_dids = ctx
            .b_to_a_signalled_dids
            .iter()
            .filter(|did| {
                did.dial_info.protocol_type().low_level_protocol_type() == LowLevelProtocolType::UDP
            })
            .collect::<Vec<_>>();
        let reverse_udp_transport_types = reverse_udp_dids
            .iter()
            .map(|did| did.dial_info.transport_type())
            .collect::<HashSet<_>>();

        // Get the best forward and reverse dial info that have the same protocol type and address type
        let intersected_udp_transport_types = target_udp_transport_types
            .intersection(&reverse_udp_transport_types)
            .copied()
            .collect::<HashSet<_>>();

        if !intersected_udp_transport_types.is_empty() {
            target_udp_dids.retain(|did| {
                intersected_udp_transport_types.contains(&did.dial_info.transport_type())
            });
            reverse_udp_dids.retain(|did| {
                intersected_udp_transport_types.contains(&did.dial_info.transport_type())
            });

            // Add holepunch-capable pairs
            for target_udp_did in target_udp_dids.iter() {
                for reverse_udp_did in reverse_udp_dids.iter() {
                    // Ensure both dialinfo have the same transport type
                    if target_udp_did.dial_info.transport_type()
                        != reverse_udp_did.dial_info.transport_type()
                    {
                        continue;
                    }

                    // The target and ourselves have a udp dialinfo that they can reach,
                    // so add them for each signalling dialinfo
                    for signalling_did in signalling_dids.iter() {
                        out.push(ContactMethod::SignalHolePunch {
                            relay_di: signalling_did.dial_info.clone(),
                            hole_punch_di: target_udp_did.dial_info.clone(),
                            reverse_hole_punch_di: reverse_udp_did.dial_info.clone(),
                        });
                    }
                }
            }
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rtab", skip(self), fields(__VEILID_LOG_KEY = self.log_key()), ret))]
    fn get_inbound_relay_contact_methods(
        &self,
        ctx: &ContactMethodContext,
        out: &mut Vec<ContactMethod>,
    ) {
        // Can node A reach Node B's inbound relay(s) directly?
        for (_node_b_relay, node_b_relay_dids) in ctx.node_b_reachable_relays.iter() {
            // If so, there will be at last one relay dialinfo that meets the sequencing requirement
            for node_b_relay_did in node_b_relay_dids.iter() {
                // Add this inbound relay since it meets our requirements
                out.push(ContactMethod::InboundRelay {
                    relay_di: node_b_relay_did.dial_info.clone(),
                });
            }
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rtab", skip(self), fields(__VEILID_LOG_KEY = self.log_key()), ret))]
    fn get_outbound_relay_contact_methods(
        &self,
        ctx: &ContactMethodContext,
        out: &mut Vec<ContactMethod>,
    ) {
        for node_a_relay in ctx.node_a().relay_info_list() {
            // Ensure this is an outbound relay
            if !matches!(node_a_relay.relay_kind(), RelayKind::Outbound) {
                continue;
            }

            // Ensure it's not our relay we're trying to reach
            if ctx
                .node_b_ids()
                .contains_any_from_iter(node_a_relay.node_ids().iter())
            {
                continue;
            }

            // Get direct dial info to outbound relay (must be directly reachable)
            for relay_target_did in self.get_dial_info_details_between_nodes(
                ctx.node_a(),
                node_a_relay,
                ctx.request.dial_info_filter,
                ctx.request.sequencing,
            ) {
                // Skip anything that requires a signal
                if relay_target_did.class.requires_signal() {
                    continue;
                }

                // Add this outbound relay since it meets our requirements
                out.push(ContactMethod::OutboundRelay {
                    relay_di: relay_target_did.dial_info,
                });
            }
        }
    }
}
