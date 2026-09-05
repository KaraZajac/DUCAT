//////////////////////////////////////////////////////////////////////
// Routing Table Health Metrics

use super::*;

impl RoutingTable {
    pub fn touch_recent_peer(&self, node_id: NodeId, last_connection: Flow) {
        self.recent_peers
            .lock()
            .insert(node_id, RecentPeersEntry { last_connection });
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn get_recent_peers(&self) -> Vec<(NodeId, RecentPeersEntry)> {
        let mut recent_peers_locked = self.recent_peers.lock();

        // collect all recent peers
        let mut dead_peers = Vec::new();
        let mut out = Vec::new();

        // look up each node and make sure the connection is still live
        // (uses same logic as send_data, ensuring last_connection works for UDP)
        for node_id in recent_peers_locked.iter().map(|(k, _v)| k.clone()) {
            let mut dead = true;
            if let Ok(Some(nr)) = self.lookup_node_id(node_id.clone()) {
                if let Some(last_connection) = nr.last_flow() {
                    out.push((node_id.clone(), RecentPeersEntry { last_connection }));
                    dead = false;
                }
            }
            if dead {
                dead_peers.push(node_id);
            }
        }

        // purge dead recent peers
        for d in dead_peers {
            recent_peers_locked.remove(&d);
        }

        out
    }

    /// Return the last cached routing table health
    pub fn get_routing_table_health(&self) -> Arc<RoutingTableHealth> {
        self.routing_table_health.lock().clone()
    }

    /// Get the relative performance of a node in the routing table
    /// Returns None if no comparable nodes can be found
    /// Returns Some(NodeRelativePerformance) if comparable nodes can be found with the given filter
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", skip(self, filter, metric), ret, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(in crate::routing_table) fn get_node_relative_performance(
        &self,
        node_id: NodeId,
        snapshot: &RoutingTableSnapshot,
        filter: impl Fn(&BucketEntrySnapshot) -> bool,
        metric: impl Fn(&LatencyStats) -> TimestampDuration,
    ) -> Option<NodeRelativePerformance> {
        // Go through all entries and find all entries that matches filter function
        let mut all_filtered_nodes: Vec<&BucketEntrySnapshot> = snapshot
            .entries()
            .iter()
            .filter(|snap| filter(snap))
            .collect();

        // Sort by fastest tm90 reliable
        all_filtered_nodes
            .sort_unstable_by(|a, b| BucketEntrySnapshot::cmp_fastest_reliable(a, b, &metric));

        // Get position in list of requested node
        let node_count = all_filtered_nodes.len();
        let node_index = all_filtered_nodes
            .iter()
            .position(|snap| snap.node_ids.contains(&node_id))?;

        // Print faster node stats
        #[cfg(feature = "verbose-tracing")]
        for nl in 0..node_index {
            let snap = &all_filtered_nodes[nl];
            if let Some(node_id) = snap.best_node_id() {
                if let Some(latency) = &snap.peer_stats.latency {
                    veilid_log!(self debug "Better relay {}: {}: {}", nl, node_id, latency);
                }
            }
        }

        // Return 'percentile' position. Fastest node is 100%.
        Some(NodeRelativePerformance {
            percentile: 100.0f32 - ((node_index * 100) as f32) / (node_count as f32),
            node_index,
            node_count,
        })
    }

    /// Called to periodically refresh the various summaries we calculate about the routing table:
    ///
    /// - Entry summaries per routing domain and crypto kind
    /// - Low water marks per routing domain
    ///   - Optionally reset the low water mark for specific domains
    /// - Total and live entry counts
    /// - Routing table health summary
    pub(in crate::routing_table) fn refresh_summaries(
        &self,
        reset_low_water_mark_domains: RoutingDomainSet,
    ) {
        // Take a single snapshot for all the entry summaries, including -all- entries (dead and punished as well)
        let entry_snapshot = self.snapshot_entries(Timestamp::now(), BucketEntryState::Punished);

        // Update the routing table health
        let mut routing_table_health = RoutingTableHealth::default();
        for entry in entry_snapshot.entries().iter() {
            routing_table_health.total_entry_count += 1;
            *routing_table_health
                .per_state_entry_count
                .entry(entry.state)
                .or_insert(0) += 1;
        }

        // Same ipv6 prefix used by route allocation to identify nodes on our own network
        let ip6_prefix_size = self
            .config()
            .internal()
            .network
            .max_connections_per_ip6_prefix_size as usize;

        // Update the entry summarie per routing domain and routing domain health
        for routing_domain in RoutingDomainSet::all() {
            // Own peer info for this domain — used to identify NAT-sibling entries that can't
            // serve as safety-route hops (excluded from live_external and the low water mark)
            let own_peer_info = self.get_current_peer_info(routing_domain);

            let entry_is_own_network = |snap: &BucketEntrySnapshot| -> bool {
                snap.get_peer_info(routing_domain)
                    .map(|their_pi| {
                        own_peer_info
                            .node_info()
                            .is_any_node_on_same_ipblock(their_pi.node_info(), ip6_prefix_size)
                    })
                    .unwrap_or(false)
            };

            // Make entry summary for this routing domain
            let mut entry_summary = EntrySummary::new();

            for entry in entry_snapshot.entries().iter() {
                if let Some(pi) = entry.get_peer_info(routing_domain) {
                    // Only consider entries that have valid signed node info in this domain
                    if !pi.signatures().is_empty() {
                        entry_summary.add_entry(routing_domain, entry, entry_is_own_network(entry));
                    }
                }
            }

            // Make a low water mark
            let mut low_water_mark = LowWaterMark::new();

            for ck in VALID_CRYPTO_KINDS {
                let mut count = CapabilityCounts::new();
                for entry in entry_snapshot.entries().iter() {
                    if entry.state >= BucketEntryState::Unreliable
                        && entry.routing_domain_set().contains(routing_domain)
                        && entry.crypto_kinds().contains(&ck)
                        && !entry_is_own_network(entry)
                    {
                        count.add_entry(routing_domain, entry);
                    }
                }
                low_water_mark.set(ck, count);
            }

            {
                let rdc = self.get_routing_domain_controller(routing_domain);

                {
                    let rdd = rdc.read_dyn();

                    // Store cached entry summary (must come before get_health, because that uses the summary)
                    rdd.set_entry_summary(Arc::new(entry_summary));

                    if reset_low_water_mark_domains.contains(routing_domain) {
                        rdd.reset_low_water_mark();
                    }

                    // Update low water mark
                    rdd.update_low_water_mark(Arc::new(low_water_mark));
                }

                // Update routing domain health in the routing table health
                let health = rdc.get_health();
                routing_table_health
                    .routing_domain_health
                    .insert(routing_domain, health);
            }
        }

        // Fold a network observation into the estimator histogram.
        {
            let mut bucket_counts: BTreeMap<CryptoKind, Vec<usize>> = BTreeMap::new();
            for ck in VALID_CRYPTO_KINDS {
                bucket_counts.insert(ck, self.bucket_counts(ck));
            }

            let mut pairwise_intersection_counts: BTreeMap<(CryptoKind, CryptoKind), usize> =
                BTreeMap::new();
            for entry in entry_snapshot.entries().iter() {
                let kinds = entry.crypto_kinds();
                for i in 0..kinds.len() {
                    for j in (i + 1)..kinds.len() {
                        let k1 = kinds[i];
                        let k2 = kinds[j];
                        let pair = if k1 < k2 { (k1, k2) } else { (k2, k1) };
                        *pairwise_intersection_counts.entry(pair).or_insert(0) += 1;
                    }
                }
            }

            self.network_estimator.lock().record_observation(
                Timestamp::now(),
                &bucket_counts,
                &pairwise_intersection_counts,
            );
        }

        // Update health cache
        let routing_table_health = Arc::new(routing_table_health);
        let current_routing_table_health = routing_table_health.clone();
        let last_routing_table_health = {
            let mut rth_lock = self.routing_table_health.lock();
            core::mem::replace(&mut *rth_lock, routing_table_health)
        };

        let mut routing_domain_ready_changes =
            BTreeMap::<RoutingDomain, (DirectionSet, DirectionSet)>::new();
        for routing_domain in RoutingDomainSet::all() {
            let last_inbound_ready = last_routing_table_health
                .routing_domain_health
                .get(&routing_domain)
                .map(|h| h.is_ready_inbound);
            let last_outbound_ready = last_routing_table_health
                .routing_domain_health
                .get(&routing_domain)
                .map(|h| h.is_ready_outbound);
            let current_inbound_ready = current_routing_table_health
                .routing_domain_health
                .get(&routing_domain)
                .map(|h| h.is_ready_inbound);
            let current_outbound_ready = current_routing_table_health
                .routing_domain_health
                .get(&routing_domain)
                .map(|h| h.is_ready_outbound);

            if last_inbound_ready != current_inbound_ready
                || last_outbound_ready != current_outbound_ready
            {
                if let (Some(is_ready_inbound), Some(is_ready_outbound)) =
                    (current_inbound_ready, current_outbound_ready)
                {
                    let old_is_ready_inbound = last_inbound_ready.unwrap_or(false);
                    let old_is_ready_outbound = last_outbound_ready.unwrap_or(false);

                    let mut old_is_ready = DirectionSet::empty();
                    if old_is_ready_inbound {
                        old_is_ready.insert(Direction::In);
                    }
                    if old_is_ready_outbound {
                        old_is_ready.insert(Direction::Out);
                    }

                    let mut new_is_ready = DirectionSet::empty();
                    if is_ready_inbound {
                        new_is_ready.insert(Direction::In);
                    }
                    if is_ready_outbound {
                        new_is_ready.insert(Direction::Out);
                    }

                    if old_is_ready != new_is_ready {
                        routing_domain_ready_changes
                            .insert(routing_domain, (old_is_ready, new_is_ready));
                    }

                    if let Err(e) = self.event_bus().post(RoutingDomainReadyEvent {
                        routing_domain,
                        is_ready_inbound,
                        is_ready_outbound,
                    }) {
                        veilid_log!(self warn "failed to post RoutingDomainReadyEvent: {}", e);
                    }
                }
            }
        }

        if !routing_domain_ready_changes.is_empty() {
            let changes = routing_domain_ready_changes
                .iter()
                .map(|(rd, (old_is_ready, new_is_ready))| {
                    let old_is_ready_string = Direction::set_pretty_string(old_is_ready);
                    let new_is_ready_string = Direction::set_pretty_string(new_is_ready);
                    format!(
                        "{:#}({} -> {})",
                        rd, old_is_ready_string, new_is_ready_string
                    )
                })
                .collect::<Vec<_>>();
            veilid_log!(self debug "Ready changed: {}", changes.join(", "));
        }

        if debug_target_enabled!("rtab::health")
            && last_routing_table_health.live_entry_count()
                != current_routing_table_health.live_entry_count()
            || !routing_domain_ready_changes.is_empty()
        {
            veilid_log!(self debug target: "rtab::health", "Routing Table Health:\n{}", indent_all_string(format!("{:#}", &*current_routing_table_health)));
        }
    }
}
