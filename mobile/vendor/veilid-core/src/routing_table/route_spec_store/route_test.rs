use super::*;

/// Number of direct-target candidates probed in parallel per route test cycle
const ROUTE_TEST_DIRECT_CANDIDATE_COUNT: usize = 3;

/// Pinned future returning (is_loopback, status_result) for one route-test probe.
type RouteProbeFuture = PinBoxFutureStatic<(bool, Result<StatusResult, RPCError>)>;

/// Information about a route that can be used for testing
pub struct AllocatedRouteTestInfo {
    /// The best route set key
    pub key: PublicKey,
    /// The hops for the route
    pub hops: Vec<NodeRef>,
    /// Sequence orderings the route supports (drives keepalive sequencing coverage)
    pub orderings: SequenceOrderingSet,
}

/// The purpose of a ping validation request
pub enum RoutePingValidationPurpose {
    /// A test of the route's deliverability
    Test,
    /// A keepalive of the route to keep it alive
    Keepalive,
}

/// One route's test or keepalive schedule: send a loopback for each ordering listed.
pub struct RoutePingValidationRequest {
    pub route_id: AllocatedRouteSetId,
    pub orderings: SequenceOrderingSet,
    pub purpose: RoutePingValidationPurpose,
}

impl RouteSpecStore {
    /// Get the test info for an allocated route
    pub fn get_allocated_route_test_info(
        &self,
        route_id: &AllocatedRouteSetId,
    ) -> Option<AllocatedRouteTestInfo> {
        let cache = self.cache.read();
        let arce = cache.get_allocated_route_by_id(route_id)?;
        let key = arce.best_route_set_key()?;
        let hops = arce.hop_node_refs();
        let orderings = arce.orderings();

        Some(AllocatedRouteTestInfo {
            key,
            hops,
            orderings,
        })
    }

    /// Test an allocated route for continuity. Only used during allocation, not by keepalives.
    /// Can test remote routes but currently is not used for that.
    /// Sends one loopback ping plus up to ROUTE_TEST_DIRECT_CANDIDATE_COUNT direct-target
    /// pings concurrently. Route is considered healthy if loopback succeeds AND
    /// (at least one direct-target probe succeeds OR no direct candidates were available).
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip(self), ret, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn test_route(&self, id: RouteId) -> VeilidAPIResult<Option<bool>> {
        let is_remote = self.is_route_id_remote(&id);
        if is_remote {
            Box::pin(self.test_remote_route(RemoteRouteSetId::from_route_id(id))).await
        } else {
            Box::pin(self.test_allocated_route(AllocatedRouteSetId::from_route_id(id))).await
        }
    }

    /// Pick up to `count` direct-target test destinations for an allocated route.
    /// Selection mirrors route-allocation eligibility, excludes the route's own hops,
    /// and verifies the route's last hop has a viable contact method to each candidate.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip(self), fields(
            __VEILID_LOG_KEY = self.log_key(),
            arg_route_id = %route_id,
            arg_count = count,
            ret_chosen_count = tracing::field::Empty,
            candidates_count = tracing::field::Empty,
            outcome = tracing::field::Empty,
        ))
    )]
    pub fn route_get_testing_destinations(
        &self,
        route_id: &AllocatedRouteSetId,
        count: usize,
    ) -> Vec<Destination> {
        if count == 0 {
            #[cfg(feature = "instrument")]
            tracing::Span::current().record("outcome", "zero_count");
            return Vec::new();
        }

        let (route_hop_node_ids, last_hop_pi, crypto_kinds) = {
            let cache = self.cache.read();
            let Some(arce) = cache.get_allocated_route_by_id(route_id) else {
                veilid_log!(self debug "route_get_testing_destinations: route_id {} not found", route_id);
                #[cfg(feature = "instrument")]
                tracing::Span::current().record("outcome", "route_not_found");
                return Vec::new();
            };
            let hop_refs = arce.hop_node_refs();
            let route_hop_node_ids: Vec<NodeId> = hop_refs
                .iter()
                .flat_map(|nr| nr.node_ids().iter().cloned().collect::<Vec<_>>())
                .collect();
            let last_hop_pi = hop_refs
                .last()
                .and_then(|nr| nr.get_peer_info(RoutingDomain::PublicInternet));
            let crypto_kinds: Vec<CryptoKind> =
                arce.route_set_keys().iter().map(|k| k.kind()).collect();
            (route_hop_node_ids, last_hop_pi, crypto_kinds)
        };
        let Some(last_hop_pi) = last_hop_pi else {
            veilid_log!(self debug "route_get_testing_destinations: route_id {} has no last-hop PeerInfo on PublicInternet", route_id);
            #[cfg(feature = "instrument")]
            tracing::Span::current().record("outcome", "no_last_hop_pi");
            return Vec::new();
        };

        let routing_table = self.routing_table();
        let Some(published_peer_info) =
            routing_table.get_published_peer_info(RoutingDomain::PublicInternet)
        else {
            veilid_log!(self debug "route_get_testing_destinations: no published peer info on PublicInternet");
            #[cfg(feature = "instrument")]
            tracing::Span::current().record("outcome", "no_published_peer_info");
            return Vec::new();
        };

        let cur_ts = Timestamp::now();
        let snapshot = routing_table.snapshot_entries(cur_ts, BucketEntryState::Unreliable);

        let mut filter = self.make_route_eligible_node_filter(
            &crypto_kinds,
            Sequencing::PreferOrdered,
            &route_hop_node_ids,
            published_peer_info.clone(),
        );

        let candidates: Vec<BucketEntrySnapshot> = snapshot
            .entries()
            .iter()
            .filter(|e| {
                let some_e = Some((*e).clone());
                filter(&some_e, cur_ts)
            })
            .cloned()
            .collect();

        let n = candidates.len();
        #[cfg(feature = "instrument")]
        tracing::Span::current().record("candidates_count", n);
        if n == 0 {
            veilid_log!(self debug "route_get_testing_destinations: no eligible candidates for route_id {}", route_id);
            #[cfg(feature = "instrument")]
            tracing::Span::current().record("outcome", "no_candidates");
            return Vec::new();
        }

        let offset = self
            .test_destination_rotation_counter
            .fetch_add(1, Ordering::Relaxed);

        let mut chosen = Vec::with_capacity(count);
        for i in 0..n {
            if chosen.len() >= count {
                break;
            }
            let candidate = &candidates[(offset + i) % n];
            let Some(cand_pi) = candidate.get_peer_info(RoutingDomain::PublicInternet) else {
                continue;
            };
            let cm = routing_table.get_best_contact_method(
                RoutingDomain::PublicInternet,
                ContactMethodRequest {
                    peer_a: last_hop_pi.clone(),
                    peer_a_published: true,
                    peer_b: cand_pi.clone(),
                    dial_info_filter: DialInfoFilter::all(),
                    sequencing: Sequencing::PreferOrdered,
                },
            );
            if cm.is_none() {
                continue;
            }

            let safety_spec = SafetySpec {
                preferred_route: Some(route_id.clone().into()),
                hop_count: 1,
                stability: Stability::Reliable,
                sequencing: Sequencing::PreferOrdered,
            };
            chosen.push(Destination::Direct {
                node: candidate
                    .node_ref
                    .routing_domain_filtered(RoutingDomain::PublicInternet),
                safety_selection: SafetySelection::Safe(safety_spec),
            });
        }

        if chosen.is_empty() {
            veilid_log!(self debug "route_get_testing_destinations: route_id {} had {} candidates but none reachable from last hop", route_id, n);
            #[cfg(feature = "instrument")]
            tracing::Span::current().record("outcome", "all_unreachable");
        } else {
            #[cfg(feature = "instrument")]
            {
                tracing::Span::current().record("ret_chosen_count", chosen.len());
                tracing::Span::current().record("outcome", "ok");
            }
        }
        chosen
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip(self), ret, err, fields(
            __VEILID_LOG_KEY = self.log_key(),
            arg_route_id = %id,
            safety_route_pubkey = tracing::field::Empty,
            loopback_pr_is_stub = tracing::field::Empty,
            hop_count = tracing::field::Empty,
        ))
    )]
    async fn test_allocated_route(&self, id: AllocatedRouteSetId) -> VeilidAPIResult<Option<bool>> {
        // Resolve route key + hop refs
        let (key, hops) = {
            let cache = self.cache.read();
            let Some(arce) = cache.get_allocated_route_by_id(&id) else {
                return Ok(Some(false));
            };
            let Some(key) = arce.best_route_set_key() else {
                apibail_internal!("no best key to test allocated route");
            };
            let hops = arce.hop_node_refs();
            (key, hops)
        };

        // Assemble the loopback private route
        let private_route = match self.assemble_single_private_route(&key, None).await {
            Ok(v) => v,
            Err(VeilidAPIError::InvalidTarget { message: _ }) => {
                return Ok(Some(false));
            }
            Err(VeilidAPIError::TryAgain { message: _ }) => {
                return Ok(None);
            }
            Err(e) => {
                return Err(e);
            }
        };

        let hop_count = hops.len();

        #[cfg(feature = "instrument")]
        {
            let span = tracing::Span::current();
            span.record("safety_route_pubkey", tracing::field::display(&key));
            span.record("loopback_pr_is_stub", private_route.is_stub());
            span.record("hop_count", hop_count);
        }

        let loopback_dest = Destination::PrivateRoute {
            private_route,
            safety_selection: SafetySelection::Safe(SafetySpec {
                preferred_route: Some(id.clone().into()),
                hop_count,
                stability: Stability::Reliable,
                sequencing: Sequencing::PreferOrdered,
            }),
        };

        let direct_dests =
            self.route_get_testing_destinations(&id, ROUTE_TEST_DIRECT_CANDIDATE_COUNT);
        let direct_count = direct_dests.len();

        let registry = self.registry();

        // Fire loopback + direct probes concurrently
        let mut loopback_ok = false;
        let mut any_direct_ok = false;
        let mut probes: FuturesUnordered<RouteProbeFuture> = FuturesUnordered::new();

        {
            let registry = registry.clone();
            let dest = loopback_dest;
            probes.push(Box::pin(async move {
                let res = Box::pin(registry.rpc_processor().rpc_call_status(dest)).await;
                (true, res)
            }));
        }

        for dest in direct_dests {
            let registry = registry.clone();
            probes.push(Box::pin(async move {
                let res = Box::pin(registry.rpc_processor().rpc_call_status(dest)).await;
                (false, res)
            }));
        }

        let mut probe_transport: Option<TransportType> = None;
        while let Some((is_loopback, res)) = probes.next().await {
            match res? {
                StatusResult::Answer {
                    send_data_result, ..
                } => {
                    if probe_transport.is_none() {
                        probe_transport = send_data_result.opt_transport_type();
                    }
                    if is_loopback {
                        loopback_ok = true;
                    } else {
                        any_direct_ok = true;
                    }
                }
                StatusResult::Failed(sdr) => {
                    if probe_transport.is_none() {
                        probe_transport = sdr.opt_transport_type();
                    }
                }
                StatusResult::NotSent(_) => {
                    // Couldn't even send; no transport info recorded.
                }
            }
        }

        let healthy = loopback_ok && (any_direct_ok || direct_count == 0);
        if !healthy {
            if let Some(probe_transport) = probe_transport {
                for hop in hops {
                    hop.report_failed_route_test(probe_transport);
                }
            }
            return Ok(Some(false));
        }

        Ok(Some(true))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip(self), ret, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn test_remote_route(&self, id: RemoteRouteSetId) -> VeilidAPIResult<Option<bool>> {
        // Make private route test
        let dest = {
            // Get the route to test
            let Some(private_route) = self.best_remote_private_route(&id) else {
                apibail_internal!("no best key to test remote route");
            };

            // Always test routes with safety routes that are more likely to succeed
            let stability = Stability::Reliable;
            // Routes should test with the most likely to succeed sequencing they are capable of
            let sequencing = Sequencing::PreferOrdered;

            // Get a safety route that is good enough
            let safety_spec = SafetySpec {
                preferred_route: None,
                hop_count: self.get_default_route_hop_count_safe(),
                stability,
                sequencing,
            };

            let safety_selection = SafetySelection::Safe(safety_spec);

            Destination::PrivateRoute {
                private_route,
                safety_selection,
            }
        };

        // Test with double-round trip ping to self
        match Box::pin(self.rpc_processor().rpc_call_status(dest)).await? {
            StatusResult::Answer { .. } => Ok(Some(true)),
            StatusResult::Failed(_) | StatusResult::NotSent(_) => Ok(Some(false)),
        }
    }
}
