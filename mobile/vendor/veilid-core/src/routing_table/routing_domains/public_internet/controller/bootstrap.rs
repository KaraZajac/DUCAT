use super::*;

impl_veilid_log_facility!("rtab");

impl PublicInternetRoutingDomainController {
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn bootstrap_task_routine(
        &self,
        stop_token: StopToken,
        _last_ts: Timestamp,
        _cur_ts: Timestamp,
    ) -> EyreResult<()> {
        // Don't bother if the state changed and we don't need to bootstrap
        let nodes_needed = self.nodes_needed();
        if nodes_needed.needs_bootstrap.is_empty() {
            return Ok(());
        }
        let crypto_kinds = nodes_needed.needs_bootstrap;

        // Don't bother if bootstraps aren't configured
        let routing_table = self.routing_table();
        let bootstraps = self.config().network.routing_table.bootstrap.clone();
        if bootstraps.is_empty() {
            return Ok(());
        }

        // Reset the low water mark for this routing domain
        routing_table.refresh_summaries(RoutingDomain::PublicInternet.into());

        // See if we are specifying a direct dialinfo for bootstrap, if so use the direct mechanism
        // Otherwise treat them as txt names for the normal dns-based bootstrap mechanism
        let dial_info_converter = BootstrapDialInfoConverter::default();

        let mut bootstrap_dialinfos = Vec::<DialInfo>::new();
        let mut bootstrap_txt_names = Vec::<String>::new();
        for bootstrap in bootstraps {
            if let Ok(bootstrap_di_vec) = dial_info_converter.try_vec_from_url(&bootstrap) {
                for bootstrap_di in bootstrap_di_vec {
                    bootstrap_dialinfos.push(bootstrap_di);
                }
            } else {
                bootstrap_txt_names.push(bootstrap);
            }
        }

        // Process bootstraps into peers lists
        let network_manager = self.network_manager();
        let mut unord = FuturesUnordered::<PinBoxFuture<EyreResult<Vec<Arc<PeerInfo>>>>>::new();

        // Get a peer list from bootstraps to process
        for bsdi in bootstrap_dialinfos {
            // Direct bootstrap
            unord.push(pin_dyn_future!(async {
                let bsdi = bsdi;
                veilid_log!(self debug "Direct bootstrap with: {}", bsdi);
                pin_future!(network_manager.direct_bootstrap(bsdi)).await
            }));
        }
        for bstn in bootstrap_txt_names {
            // TXT bootstrap
            unord.push(pin_dyn_future!(async {
                let bstn = bstn;
                veilid_log!(self debug "TXT bootstrap with: {}", bstn);
                pin_future!(network_manager.txt_bootstrap(bstn)).await
            }));
        }

        let mut bootstrapped_peer_id_set = HashSet::<NodeId>::new();
        let mut bootstrapped_peers = vec![];
        loop {
            match unord.next().timeout_at(stop_token.clone()).await {
                Ok(Some(res)) => match res {
                    Ok(peers) => {
                        for peer in peers {
                            let peer_node_ids = peer.node_ids();
                            let routing_domain = peer.routing_domain();
                            if routing_domain != RoutingDomain::PublicInternet {
                                continue;
                            }
                            if !peer_node_ids
                                .iter()
                                .any(|x| bootstrapped_peer_id_set.contains(x))
                            {
                                if routing_table.matches_own_node_id(peer_node_ids) {
                                    veilid_log!(self debug "Ignoring own node in bootstrap response");
                                } else {
                                    for nid in peer.node_ids().iter().cloned() {
                                        bootstrapped_peer_id_set.insert(nid);
                                    }
                                    bootstrapped_peers.push(peer);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        veilid_log!(self debug "Bootstrap error: {}", e);
                    }
                },
                Ok(None) => {
                    // Done
                    break;
                }
                Err(_) => {
                    // Cancelled
                    return Ok(());
                }
            }
        }

        self.read_dyn().clear_bootstrap_peers();

        if self
            .unlocked_inner
            .bootstrap_wait_detector
            .fetch_update(|opt_ts| {
                if opt_ts.is_none() {
                    Some(Some(Timestamp::now()))
                } else {
                    None
                }
            })
            .is_ok()
        {
            veilid_log!(self info "Waiting for bootstrap...");
        }
        let valid_bootstraps = self
            .bootstrap_with_peer_list(bootstrapped_peers.clone(), crypto_kinds.clone(), stop_token)
            .await?;
        if valid_bootstraps.is_empty() {
            veilid_log!(self debug "No external bootstrap peers");

            // Leave bootstrap wait detector set so we don't spam logs
            return Ok(());
        }
        veilid_log!(self debug "External bootstrap peers: {:#?}", valid_bootstraps);

        // Get all node ids that have a peerinfo in this routing domain
        let mut rd_peer_ids = BTreeSet::new();
        for peer in bootstrapped_peers.iter() {
            if peer.routing_domain() == RoutingDomain::PublicInternet {
                for nid in peer.node_ids().iter().cloned() {
                    rd_peer_ids.insert(nid);
                }
            }
        }

        // Add valid bootstrap peers to routing domain
        for bootstrap_peer in valid_bootstraps.iter().cloned() {
            let mut add = false;
            if let Some(pi) = bootstrap_peer.get_peer_info(RoutingDomain::PublicInternet) {
                for nid in pi.node_ids().iter() {
                    if rd_peer_ids.contains(nid) {
                        add = true;
                        break;
                    }
                }
            }
            if add {
                self.read_dyn().add_bootstrap_peer(bootstrap_peer);
            }
        }
        let routing_table = self.routing_table();
        routing_table.flush().await;

        // Reset the low water mark for this routing domain
        routing_table.refresh_summaries(RoutingDomain::PublicInternet.into());

        if let Some(old_ts) = self.unlocked_inner.bootstrap_wait_detector.swap(None) {
            let end_ts = Timestamp::now();
            let duration = end_ts.duration_since(old_ts);
            veilid_log!(self info "Bootstrap finished in {:#} via {}", duration, valid_bootstraps.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","));
        }

        Ok(())
    }

    pub fn bootstrap_with_peer(
        &self,
        crypto_kinds: Vec<CryptoKind>,
        peer_info: Arc<PeerInfo>,
        existing_node_ids: Arc<HashSet<NodeId>>,
        unord: &FuturesUnordered<PinBoxFutureStatic<Option<NodeRef>>>,
    ) {
        veilid_log!(self trace
            "--- bootstrapping {} with {:?}",
            peer_info.node_ids(),
            peer_info.node_info().dial_info_detail_list()
        );

        let routing_domain = peer_info.routing_domain();
        let routing_table = self.routing_table();

        let nr = match routing_table.register_node_with_peer_info(peer_info, true) {
            Ok(nr) => nr,
            Err(e) => {
                veilid_log!(self error "failed to register bootstrap peer info: {}", e);
                return;
            }
        };

        // Add this our futures to process in parallel
        for crypto_kind in crypto_kinds {
            // Bootstrap this crypto kind
            let nr = nr.unfiltered();
            let existing_node_ids = existing_node_ids.clone();
            unord.push(Box::pin(
                async move {
                    let network_manager = nr.network_manager();
                    let routing_table = nr.routing_table();

                    // Get what contact method would be used for contacting the bootstrap
                    let bsdi = match network_manager
                        .get_node_contact_method(nr.sequencing_filtered(Sequencing::PreferOrdered))
                    {
                        Ok(NodeContactMethodResult::Resolved(ncm)) => {
                            let Some(di) = ncm.direct_dial_info() else {
                                veilid_log!(nr debug "invalid contact method for bootstrap, ignoring peer: {}", NodeContactMethodResult::Resolved(ncm));
                                return None;
                            };
                            di
                        }
                        Ok(v) => {
                            veilid_log!(nr debug "invalid contact method for bootstrap, ignoring peer: {}", v);
                            return None;
                        }
                        Err(e) => {
                            veilid_log!(nr warn "unable to bootstrap: {}", e);
                            return None;
                        }
                    };

                    // Need VALID signed peer info, so ask bootstrap to find_node of itself
                    // which will ensure it has the bootstrap's signed peer info as part of the response
                    let _ = Box::pin(routing_table.find_nodes_close_to_node_ref(crypto_kind, nr.sequencing_filtered(Sequencing::PreferOrdered), vec![])).await;

                    // Ensure we got the signed peer info
                    if !nr.peer_info_has_valid_signature(routing_domain) {
                        veilid_log!(nr debug "bootstrap server is not responding for dialinfo: {}", bsdi);
                        None
                    } else {
                        // otherwise this bootstrap is valid, lets ask it to find ourselves now
                        // We should prefer nodes that have relaying, signaling, routing and validation of dial info
                        // for our initial nodes in our routing table because we need these things in order to
                        // properly attach to the network

                        // Find nodes close to ourself
                        let close_node_refs = network_result_value_or_log!(nr match pin_future!(
                                routing_table.find_new_nodes_close_to_self(
                                    crypto_kind,
                                    &existing_node_ids,
                                    nr.routing_domain_filtered(RoutingDomain::PublicInternet).with_sequencing(Sequencing::PreferOrdered),
                                    CONNECTIVITY_CAPABILITIES.to_vec())
                                ).await {
                            Err(e) => {
                                veilid_log!(nr debug "failed to find nodes close to self: {}", e);
                                NetworkResult::value(vec![])
                            }
                            Ok(v) => v,
                        } => {
                            vec![]
                        });
                        veilid_log!(nr debug target:"network_result", "bootstrap: found {} nodes close to self for {} in {} with {:?}", close_node_refs.len(), crypto_kind, RoutingDomain::PublicInternet, CONNECTIVITY_CAPABILITIES);

                        // Now find nodes widely across the network
                        let wide_node_refs = match pin_future!(routing_table.find_new_nodes_wide(
                                crypto_kind,
                                &existing_node_ids,
                                RoutingDomain::PublicInternet,
                                None,
                                CONNECTIVITY_CAPABILITIES.to_vec())
                            ).await {
                            Err(e) => {
                                veilid_log!(nr debug "failed to find nodes wide: {}", e);
                                vec![]
                            }
                            Ok(v) => v,
                        };

                        veilid_log!(nr debug target:"network_result", "bootstrap: found {} nodes wide for {} in {} with {:?}", wide_node_refs.len(), crypto_kind, RoutingDomain::PublicInternet, CONNECTIVITY_CAPABILITIES);

                        veilid_log!(nr debug "Bootstrap of {} successful via {} with {} close nodes and {} wide nodes", crypto_kind, nr, close_node_refs.len(), wide_node_refs.len());
                        Some(nr)
                    }
                }
                .instrument(Span::current()),
            ));
        }
    }

    /// Takes in a list of bootstrap peer info, and attempts bootstrapping
    /// A list of valid bootstrap peer noderefs is returned
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn bootstrap_with_peer_list(
        &self,
        bootstrap_peers: Vec<Arc<PeerInfo>>,
        crypto_kinds: Vec<CryptoKind>,
        stop_token: StopToken,
    ) -> EyreResult<Vec<NodeRef>> {
        if bootstrap_peers.is_empty() {
            veilid_log!(self debug "No peers suitable for bootstrap");
            return Ok(vec![]);
        }

        veilid_log!(self debug "Bootstrap peers: {:?}", &bootstrap_peers);

        // Snapshot once and extract existing node ids for dedup filtering
        let existing_node_ids = Arc::new(
            self.routing_table()
                .snapshot_entries(Timestamp::now(), BucketEntryState::Initial)
                .existing_node_ids(None),
        );

        // Run all bootstrap operations concurrently
        let mut unord = FuturesUnordered::<PinBoxFutureStatic<Option<NodeRef>>>::new();
        for peer in bootstrap_peers {
            let mut peer_crypto_kinds = vec![];
            for ck in crypto_kinds.iter().copied() {
                if peer.node_ids().get(ck).is_some() {
                    peer_crypto_kinds.push(ck);
                }
            }
            if peer_crypto_kinds.is_empty() {
                continue;
            }

            veilid_log!(self debug "Attempting bootstrap: {}", peer.node_ids());
            self.bootstrap_with_peer(peer_crypto_kinds, peer, existing_node_ids.clone(), &unord);
        }

        // Wait for all bootstrap operations to complete before we complete the singlefuture
        let mut valid_bootstraps = vec![];
        while let Ok(Some(res)) = unord.next().timeout_at(stop_token.clone()).await {
            if let Some(valid_bootstrap) = res {
                valid_bootstraps.push(valid_bootstrap);
            }
        }
        Ok(valid_bootstraps)
    }
}
