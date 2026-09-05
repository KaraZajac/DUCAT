use super::*;

impl_veilid_log_facility!("net");

impl NetworkManager {
    /// Direct bootstrap request handler (separate fallback mechanism from cheaper TXT bootstrap mechanism)
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", skip(self), ret, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn handle_boot_v0_request(&self, flow: Flow) -> EyreResult<NetworkResult<()>> {
        // Get a bunch of nodes with a range of crypto kinds, protocols and capabilities
        let bootstrap_nodes = self.find_bootstrap_nodes_filtered(2);

        // Serialize out peer info
        let bootstrap_peerinfo: Vec<Arc<PeerInfo>> = bootstrap_nodes
            .iter()
            .filter_map(|nr| nr.get_peer_info(RoutingDomain::PublicInternet))
            .collect();
        let json_bytes: Bytes = serialize_json(bootstrap_peerinfo).into();

        veilid_log!(self trace "BOOT reponse: {}", String::from_utf8_lossy(&json_bytes));

        // Reply with a chunk of signed routing table
        let net = self.net();
        match pin_future_closure!(net.send_data_to_existing_flow(flow, json_bytes)).await? {
            SendDataToExistingFlowResult::Sent(_) => {
                // Bootstrap reply was sent
                Ok(NetworkResult::value(()))
            }
            SendDataToExistingFlowResult::NotSent(_) => Ok(NetworkResult::no_connection_other(
                "bootstrap reply could not be sent",
            )),
        }
    }

    /// Retrieve up to N of each type of protocol capable nodes for a single crypto kind
    fn find_bootstrap_nodes_filtered_per_crypto_kind(
        &self,
        crypto_kind: CryptoKind,
        max_per_type: usize,
    ) -> Vec<NodeRef> {
        let protocol_types = [
            ProtocolType::UDP,
            ProtocolType::TCP,
            ProtocolType::WS,
            #[cfg(feature = "enable-protocol-wss")]
            ProtocolType::WSS,
        ];

        let protocol_types_len = protocol_types.len();
        let mut nodes_proto_v4 = [0usize, 0usize, 0usize, 0usize];
        let mut nodes_proto_v6 = [0usize, 0usize, 0usize, 0usize];

        let filter = Box::new(
            move |opt_snap: &Option<BucketEntrySnapshot>, _cur_ts: Timestamp| {
                let snap = opt_snap.as_ref().unwrap_or_log();

                // skip nodes on our local network here
                if snap
                    .routing_domain_set()
                    .contains(RoutingDomain::LocalNetwork)
                {
                    return false;
                }

                // Ensure crypto kind is supported
                if !snap.node_ids.kinds().contains(&crypto_kind) {
                    return false;
                }

                // Only nodes with direct publicinternet node info
                let Some(pi) = snap.get_peer_info(RoutingDomain::PublicInternet) else {
                    return false;
                };
                let node_info = pi.node_info();

                // Bootstraps must have -only- inbound capable network class and direct dialinfo
                if !node_info.is_fully_direct_inbound() {
                    return false;
                }

                // Must have connectivity capabilities
                if !node_info.has_all_capabilities(CONNECTIVITY_CAPABILITIES) {
                    return false;
                }

                // Check for direct dialinfo and a good mix of protocol and address types
                let mut keep = false;
                for did in node_info.dial_info_detail_list() {
                    if matches!(did.dial_info.address_type(), AddressType::IPV4) {
                        for (n, protocol_type) in protocol_types.iter().enumerate() {
                            if nodes_proto_v4[n] < max_per_type
                                && did.dial_info.protocol_type() == *protocol_type
                            {
                                nodes_proto_v4[n] += 1;
                                keep = true;
                            }
                        }
                    } else if matches!(did.dial_info.address_type(), AddressType::IPV6) {
                        for (n, protocol_type) in protocol_types.iter().enumerate() {
                            if nodes_proto_v6[n] < max_per_type
                                && did.dial_info.protocol_type() == *protocol_type
                            {
                                nodes_proto_v6[n] += 1;
                                keep = true;
                            }
                        }
                    }
                }
                keep
            },
        ) as RoutingTableEntryFilter;

        let filters = VecDeque::from([filter]);

        self.routing_table().get_preferred_fastest_nodes(
            protocol_types_len * 2 * max_per_type,
            filters,
            |opt_snap: Option<BucketEntrySnapshot>| opt_snap.unwrap_or_log().node_ref.clone(),
        )
    }

    /// Retrieve up to N of each type of protocol capable nodes for all crypto kinds
    fn find_bootstrap_nodes_filtered(&self, max_per_type: usize) -> Vec<NodeRef> {
        let mut out =
            self.find_bootstrap_nodes_filtered_per_crypto_kind(VALID_CRYPTO_KINDS[0], max_per_type);

        // Merge list of nodes so we don't have duplicates
        for crypto_kind in &VALID_CRYPTO_KINDS[1..] {
            let nrs =
                self.find_bootstrap_nodes_filtered_per_crypto_kind(*crypto_kind, max_per_type);
            'nrloop: for nr in nrs {
                for nro in &out {
                    if nro.same_entry(&nr) {
                        continue 'nrloop;
                    }
                }
                out.push(nr);
            }
        }
        out
    }
}
