use super::*;

impl_veilid_log_facility!("rtab");

/// How frequently we optimize relays
const RELAY_OPTIMIZATION_INTERVAL: TimestampDuration = TimestampDuration::new_secs(10);
/// What percentile to keep our relays optimized to
const RELAY_OPTIMIZATION_PERCENTILE: f32 = 66.0;
/// What percentile to choose our relays from (must be greater than RELAY_OPTIMIZATION_PERCENTILE)
const RELAY_SELECTION_PERCENTILE: f32 = 85.0;

impl PublicInternetRoutingDomainController {
    /// Check is a relay is still valid is this routing domain
    /// Does not require access to the routing domain controller itself
    fn check_relay_valid(
        routing_table: &RoutingTable,
        cur_ts: Timestamp,
        relay: &RoutingDomainRelay,
        mut relay_state: RoutingDomainRelayState,
        relay_node_filter: &impl Fn(&BucketEntrySnapshot) -> bool,
        snapshot: &RoutingTableSnapshot,
    ) -> Option<RoutingDomainRelayState> {
        let relay_node = relay.relay_node.clone();
        let state_reason = relay_node.state_reason(cur_ts);

        // No best node id
        let relay_node_id = relay_node.best_node_id();

        // Relay node is dead or no longer needed
        if matches!(
            state_reason,
            BucketEntryStateReason::Dead(_) | BucketEntryStateReason::Punished(_)
        ) {
            veilid_log!(routing_table debug "Relay node is now {:?}, dropping relay {}", state_reason, relay_node);
            return None;
        }

        // Relay node no longer can relay. Tolerate transient unresponsiveness (Missing from a
        // single lost question): truly dead relays are dropped by the state check above, and
        // dropping on every blip reselects relays until the flap detector trips.
        let relay_snap = relay_node
            .entry()
            .snapshot(routing_table.registry(), cur_ts);
        let relay_filter_passes = relay_node_filter(&relay_snap);
        if !relay_filter_passes {
            if relay_snap.state.is_responsive() {
                veilid_log!(routing_table debug
                    "Relay node can no longer relay, dropping relay {}",
                    relay.relay_node
                );
                return None;
            }
            veilid_log!(routing_table debug
                "Relay node temporarily unresponsive, keeping relay {}",
                relay.relay_node
            );
        }

        // Pathological: protected connections die too fast on this relay.
        if let Some(drop_stats) = relay_node.connection_stats().protected_drop_span {
            if drop_stats.tm90 < RELAY_MIN_PROTECTED_CONNECTION_TM90 {
                veilid_log!(routing_table debug
                    "Relay protected connection lifetime tm90={} < {}, dropping relay {}",
                    drop_stats.tm90,
                    RELAY_MIN_PROTECTED_CONNECTION_TM90,
                    relay_node
                );
                return None;
            }
        }

        // See if our relay was optimized last long enough ago to consider getting a new one
        // if it is no longer fast enough. A kept-but-unresponsive relay is excluded from the
        // relative-performance set by the same filter, so optimizing it here would drop it as
        // "unmeasurable" and undo the retention above every interval; let it recover or die
        // via the Dead state check instead.
        let last_optimized_duration = cur_ts.duration_since(relay_state.last_optimized);
        if relay_filter_passes && last_optimized_duration > RELAY_OPTIMIZATION_INTERVAL {
            // See what our relay's current percentile is
            if let Some(relay_relative_performance) = routing_table.get_node_relative_performance(
                relay_node_id,
                snapshot,
                relay_node_filter,
                |ls| ls.tm90,
            ) {
                // Get latency numbers
                let latency_stats = if let Some(latency) = relay_node.peer_stats().latency {
                    latency.to_string()
                } else {
                    "[no stats]".to_owned()
                };

                // Get current relay reliability
                let state_reason = relay_node.state_reason(cur_ts);

                if relay_relative_performance.percentile < RELAY_OPTIMIZATION_PERCENTILE {
                    // Drop the current relay so we can get the best new one
                    veilid_log!(routing_table debug
                        "Relay tm90 is ({:.2}% < {:.2}%) ({} out of {}) (latency {}, {:?}) optimizing relay {}",
                        relay_relative_performance.percentile,
                        RELAY_OPTIMIZATION_PERCENTILE,
                        relay_relative_performance.node_index,
                        relay_relative_performance.node_count,
                        latency_stats,
                        state_reason,
                        relay_node
                    );
                    return None;
                } else {
                    // Note that we tried to optimize the relay but it was good
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(routing_table debug
                        "Relay tm90 is ({:.2}% >= {:.2}%) ({} out of {}) (latency {}, {:?}) keeping {}",
                        relay_relative_performance.percentile,
                        RELAY_OPTIMIZATION_PERCENTILE,
                        relay_relative_performance.node_index,
                        relay_relative_performance.node_count,
                        latency_stats,
                        state_reason,
                        relay_node
                    );
                    relay_state.last_optimized = cur_ts;
                }
            } else {
                // Drop the current relay because it could not be measured
                veilid_log!(routing_table debug
                    "Relay relative performance not found {}",
                    relay_node
                );
                return None;
            }
        }

        Some(relay_state)
    }

    /// Keep relays assigned and accessible
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn relay_management_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        cur_ts: Timestamp,
    ) -> EyreResult<()> {
        let routing_table = self.routing_table();

        // Get relay requirements if we've reached the state where we can allocate relays meaningfully
        let state = self.state();
        let relay_requirements = match state.inbound_stage {
            RoutingDomainInboundStage::Invalid
            | RoutingDomainInboundStage::NeedsDialInfoConfirmation
            | RoutingDomainInboundStage::Unusable => return Ok(()),
            RoutingDomainInboundStage::NeedsRelays | RoutingDomainInboundStage::ReadyToPublish => {
                state.relay_requirements.clone()
            }
        };

        let (state_updates, opt_new_relay_compilation) = if !relay_requirements.needs_relays() {
            // If the relay requirements don't need relays, let's just drop what we have
            (vec![], None)
        } else {
            // Pull everything we need from the routing-domain detail under a brief read lock,
            // then drop it before any routing-table or bucket-entry work to avoid a
            // lock-order inversion with last_flows → routing_domain_for_flow.
            let (opt_last_relay_compilation, relay_node_filter, prefetched_relay_states) = {
                let detail = self.read();
                let opt_last_relay_compilation = detail.relay_compilation();
                let relay_node_filter = detail.make_relay_node_filter();
                let prefetched_relay_states: BTreeMap<NodeId, RoutingDomainRelayState> =
                    opt_last_relay_compilation
                        .as_ref()
                        .map(|c| {
                            c.relays
                                .iter()
                                .filter_map(|r| {
                                    detail
                                        .relay_state(r.relay_id.clone())
                                        .map(|s| (r.relay_id.clone(), s))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                (
                    opt_last_relay_compilation,
                    relay_node_filter,
                    prefetched_relay_states,
                )
            };

            let relay_snapshot =
                routing_table.snapshot_entries(cur_ts, BucketEntryState::Unreliable);

            let mut valid_relays = vec![];
            let mut state_updates = vec![];
            if let Some(last_relay_compilation) = opt_last_relay_compilation {
                for relay in last_relay_compilation.relays.iter() {
                    let Some(relay_state) = prefetched_relay_states.get(&relay.relay_id).copied()
                    else {
                        veilid_log!(self error "relay state not found for relay {}", relay.relay_id);
                        continue;
                    };
                    if let Some(updated_relay_state) = Self::check_relay_valid(
                        &routing_table,
                        cur_ts,
                        relay,
                        relay_state,
                        &relay_node_filter,
                        &relay_snapshot,
                    ) {
                        if updated_relay_state != relay_state {
                            state_updates.push((relay.relay_id.clone(), updated_relay_state));
                        }
                        valid_relays.push(relay.clone());
                    }
                }
            }

            // Start a new relay compilation
            let mut new_relay_compiler = relay_requirements.make_relay_compiler();

            // Handle outbound relay
            let mut has_outbound_relay = false;
            let mut attempted_relays = HashSet::<NodeId>::new();
            if let Some(outbound_relay_peerinfo) =
                routing_table.get_outbound_relay_peer(RoutingDomain::PublicInternet)
            {
                // Drop outbound relay if it changed
                valid_relays.retain(|rdr| {
                    if matches!(rdr.relay_kind, RelayKind::Outbound) {
                        if rdr
                            .relay_node
                            .node_ids()
                            .contains_any_from_iter(outbound_relay_peerinfo.node_ids().iter())
                        {
                            has_outbound_relay = true;
                            true
                        } else {
                            false
                        }
                    } else {
                        true
                    }
                });

                // Allocate outbound relay if one is needed
                if !has_outbound_relay {
                    match routing_table.register_node_with_peer_info(outbound_relay_peerinfo, false)
                    {
                        Ok(relay_node) => {
                            let outbound_relay = RoutingDomainRelay::new(
                                RoutingDomain::PublicInternet,
                                relay_node.unfiltered(),
                                RelayKind::Outbound,
                            );
                            for rid in outbound_relay.relay_node.node_ids().iter() {
                                attempted_relays.insert(rid.clone());
                            }
                            new_relay_compiler.apply_relay(outbound_relay);
                        }
                        Err(e) => {
                            veilid_log!(self error "failed to register outbound relay with peer info: {}", e);
                        }
                    };
                }
            }

            // Apply valid inbound relays to status
            for relay in valid_relays {
                for rid in relay.relay_node.node_ids().iter() {
                    attempted_relays.insert(rid.clone());
                }
                let relay_node = relay.relay_node.clone();
                if !new_relay_compiler.apply_relay(relay) {
                    // Silent compilation rejection is the remaining relay-churn source
                    veilid_log!(self debug "Valid relay no longer useful in compilation, dropping relay {}", relay_node);
                }
            }

            // Allocate new inbound relays as needed
            while new_relay_compiler.want_more_relays() {
                let next_relay_node_filter = |snap: &BucketEntrySnapshot| {
                    // Exclude any relays we have already
                    for nid in snap.node_ids.iter() {
                        if attempted_relays.contains(nid) {
                            return false;
                        }
                    }
                    relay_node_filter(snap)
                };
                // Find a node in our routing table that is an acceptable inbound relay
                if let Some(relay_node) = routing_table.get_random_fast_node(
                    &relay_snapshot,
                    next_relay_node_filter,
                    RELAY_SELECTION_PERCENTILE,
                    |ls| ls.tm90,
                ) {
                    for rid in relay_node.node_ids().iter() {
                        attempted_relays.insert(rid.clone());
                    }

                    let routing_domain_relay = RoutingDomainRelay::new(
                        RoutingDomain::PublicInternet,
                        relay_node,
                        RelayKind::Inbound,
                    );

                    new_relay_compiler.apply_relay(routing_domain_relay);
                } else {
                    // No relays left, we did our best
                    break;
                }
            }

            // Get final sorted list of relays
            let Some(new_relay_compilation) = new_relay_compiler.compile() else {
                // Can't satisfy relay requirements yet
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(self debug "can't satisfy relay requirements: {:#}", new_relay_compiler);
                return Ok(());
            };

            (state_updates, Some(new_relay_compilation))
        };

        // Apply all changes in a single editor commit
        let mut editor = self.edit();

        // Update the relay compilation
        // Unless the relay list itself changes, this will just be a no-op
        // The relay requirements can change but if the new relay list is equivalent to the existing one and satisfies the new requirements
        // then the peer info will not change. This is unlikely, but possible.
        editor.set_relay_compilation(opt_new_relay_compilation);

        // Process state changes -after- we update the relays so they get applied for the new relays
        // Otherwise the will be ignored because the relay list itself will not include the ids referenced here
        for (relay, state) in state_updates {
            editor.set_relay_state(relay, state);
        }

        editor.commit();
        self.publish_peer_info();

        Ok(())
    }
}
