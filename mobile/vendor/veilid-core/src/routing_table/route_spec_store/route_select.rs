use super::*;

#[derive(Clone, Debug)]
pub struct RouteSelectParams {
    pub crypto_kind: CryptoKind,
    pub preferred_route: Option<AllocatedRouteSetId>,
    pub hop_count: usize,
    pub stability: Stability,
    pub sequencing: Sequencing,
    pub directions: DirectionSet,
    pub avoid_nodes: Vec<NodeId>,
    pub is_destination_safe: bool,
}

#[derive(Clone, Debug)]
struct FirstAvailableRouteParams {
    pub crypto_kind: CryptoKind,
    pub min_hop_count: usize,
    pub max_hop_count: usize,
    pub stability: Stability,
    pub sequencing: Sequencing,
    pub directions: DirectionSet,
    pub avoid_nodes: Vec<NodeId>,
}

#[derive(Clone, Debug)]
pub struct RouteIdAndKeys {
    pub route_id: AllocatedRouteSetId,
    pub route_set_keys: PublicKeyGroup,
}

impl RouteSpecStore {
    /// Get a single allocated route that matches a particular safety spec
    /// Returns the public key associated with a single allocated route
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip_all, fields(
            __VEILID_LOG_KEY = self.log_key(),
            arg_crypto_kind = ?params.crypto_kind,
            arg_preferred_route = tracing::field::debug(&params.preferred_route),
            arg_requested_hop_count = params.hop_count,
            arg_stability = %params.stability,
            arg_sequencing = %params.sequencing,
            arg_is_destination_safe = params.is_destination_safe,
            arg_avoid_nodes_len = params.avoid_nodes.len(),
            actual_hop_count = tracing::field::Empty,
            doubled = tracing::field::Empty,
            selected_via = tracing::field::Empty,
            selected_route_id = tracing::field::Empty,
        ))
    )]
    pub async fn select_single_route(
        &self,
        mut params: RouteSelectParams,
    ) -> VeilidAPIResult<RouteIdAndKeys> {
        #[cfg(feature = "instrument")]
        let requested_hop_count = params.hop_count;

        // Ensure the total hop count isn't too long for our config
        if params.hop_count == 0 {
            apibail_invalid_argument!(
                "safety route hop count is zero",
                "hop_count",
                params.hop_count
            );
        }

        if params.hop_count > self.get_max_route_hop_count() {
            apibail_invalid_argument!(
                "safety route hop count too long",
                "hop_count",
                params.hop_count
            );
        }

        // Increase hop count if too short when targeting unsafe destinations
        if !params.is_destination_safe
            && params.hop_count < self.get_default_route_hop_count_unsafe()
        {
            params.hop_count = (params.hop_count * 2).min(self.get_max_route_hop_count());
        };

        #[cfg(feature = "instrument")]
        {
            let span = tracing::Span::current();
            span.record("actual_hop_count", params.hop_count);
            span.record("doubled", params.hop_count != requested_hop_count);
        }

        let first_available_route_params = FirstAvailableRouteParams {
            crypto_kind: params.crypto_kind,
            min_hop_count: params.hop_count,
            max_hop_count: params.hop_count,
            stability: params.stability,
            sequencing: params.sequencing,
            directions: params.directions,
            avoid_nodes: params.avoid_nodes,
        };

        let opt_allocate_route_lock_guard = {
            let cache = self.cache.read();

            // See if the preferred route is already available
            if let Some(preferred_route) = &params.preferred_route {
                if let Some(preferred_arce) = cache.get_allocated_route_by_id(preferred_route) {
                    // Only use the preferred route if it has the desired crypto kind
                    let public_keys = preferred_arce.route_set_keys();
                    if public_keys.contains_kind(params.crypto_kind) {
                        // Only use the preferred route if it doesn't contain the avoid nodes
                        if !preferred_arce.contains_nodes(&first_available_route_params.avoid_nodes)
                        {
                            #[cfg(feature = "instrument")]
                            {
                                let span = tracing::Span::current();
                                span.record("selected_via", "preferred");
                                span.record(
                                    "selected_route_id",
                                    tracing::field::display(preferred_route),
                                );
                            }
                            return Ok(RouteIdAndKeys {
                                route_id: preferred_route.clone(),
                                route_set_keys: public_keys.clone(),
                            });
                        }
                    }
                }
            }

            // Select a safety route from the pool or make one if we don't have one that matches
            // Try this outside of the allocate lock to see if we can do this lock-free first
            if let Some(sr_route_id_and_public_keys) = Self::first_available_route_inner(
                &cache,
                &first_available_route_params,
                &self.route_selection_counter,
            ) {
                // Found a route to use
                #[cfg(feature = "instrument")]
                {
                    let span = tracing::Span::current();
                    span.record("selected_via", "first_available");
                    span.record(
                        "selected_route_id",
                        tracing::field::display(&sr_route_id_and_public_keys.route_id),
                    );
                }
                return Ok(sr_route_id_and_public_keys);
            }

            // No matching route found so allocate one

            // Trade locks to get the first available allocate lock
            self.allocate_route_lock.try_lock()

            // Drop inner read lock because it is synchronous
        };

        let allocate_route_lock_guard = match opt_allocate_route_lock_guard {
            Some(g) => {
                // No need to re-check first available route because try_lock means no contention
                g
            }
            None => {
                // Get the first available allocate lock
                let g = self.allocate_route_lock.lock().await;

                // Must re-check first available route to avoid race condition due to await
                // Because during the time we didn't hold the allocate lock, the first available route may have been allocated
                let cache = self.cache.read();
                if let Some(sr_route_id_and_public_keys) = Self::first_available_route_inner(
                    &cache,
                    &first_available_route_params,
                    &self.route_selection_counter,
                ) {
                    // Found a route to use
                    #[cfg(feature = "instrument")]
                    {
                        let span = tracing::Span::current();
                        span.record("selected_via", "first_available_post_lock");
                        span.record(
                            "selected_route_id",
                            tracing::field::display(&sr_route_id_and_public_keys.route_id),
                        );
                    }
                    return Ok(sr_route_id_and_public_keys);
                }

                g
            }
        };

        // Note: `crypto_kind` and `directions` from params is only for selection, not for allocation.
        // Automatic routes should always use all crypto kinds and be bidirectional.
        let params = AllocateRouteParams {
            crypto_kinds: VALID_CRYPTO_KINDS.to_vec(),
            hop_count: params.hop_count,
            stability: params.stability,
            sequencing: params.sequencing,
            directions: DirectionSet::all(),
            avoid_nodes: first_available_route_params.avoid_nodes,
            automatic: true,
        };
        let allocated = self
            .allocate_route_inner(&allocate_route_lock_guard, params)
            .await;
        #[cfg(feature = "instrument")]
        {
            let span = tracing::Span::current();
            span.record("selected_via", "newly_allocated");
            if let Ok(rk) = &allocated {
                span.record("selected_route_id", tracing::field::display(&rk.route_id));
            }
        }
        allocated
    }

    /// Find first matching unpublished route that fits into the selection criteria
    /// Don't pick any routes that have failed and haven't been tested yet
    /// Round-robins among top-tier routes to distribute load across safety routes
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip_all, fields(__VEILID_LOG_KEY = cache.log_key()))
    )]
    fn first_available_route_inner(
        cache: &RouteSpecStoreCache,
        params: &FirstAvailableRouteParams,
        route_selection_counter: &AtomicUsize,
    ) -> Option<RouteIdAndKeys> {
        let cur_ts = Timestamp::now();

        let mut routes = Vec::new();

        // Get all valid routes, allow routes that need testing
        // but definitely prefer routes that have been recently tested
        for (id, arce) in cache.iter_allocated_routes() {
            if arce.is_live_sequencing_match(params.sequencing)
                && arce.hop_count() >= params.min_hop_count
                && arce.hop_count() <= params.max_hop_count
                && arce.directions().is_superset(params.directions)
                && arce
                    .route_set_keys()
                    .iter()
                    .any(|x| x.kind() == params.crypto_kind)
                && !arce.is_published()
                && !arce.contains_nodes(&params.avoid_nodes)
            {
                // snapshot stats (interior mutability) to avoid race conditions and sort instability
                let stats = arce.with_stats(|stats| stats.clone());

                routes.push((id, arce, stats));
            }
        }

        // Sort the routes by preference
        routes.sort_unstable_by(|a, b| {
            // NOTE: do not rank or exclude on RouteStatsDisposition here.
            // Blame oscillates (fanouts charge routes for dead targets and any
            // answer clears it), so it penalizes routes exactly while they
            // carry traffic and prefers idle/untested ones. Validated 2026-07:
            // both gating and sort-preference on disposition break routed
            // answers on the public network. Disposition drives testing only.

            // Prefer routes that don't need testing
            let a_needs_testing = a.2.needs_testing(a.1.orderings(), cur_ts);
            let b_needs_testing = b.2.needs_testing(b.1.orderings(), cur_ts);
            if !a_needs_testing && b_needs_testing {
                return cmp::Ordering::Less;
            }
            if !b_needs_testing && a_needs_testing {
                return cmp::Ordering::Greater;
            }

            // Prefer routes that meet the stability selection
            let a_meets_stability = a.1.stability() >= params.stability;
            let b_meets_stability = b.1.stability() >= params.stability;
            if a_meets_stability && !b_meets_stability {
                return cmp::Ordering::Less;
            }
            if b_meets_stability && !a_meets_stability {
                return cmp::Ordering::Greater;
            }

            // Prefer faster routes
            let a_latency = a.2.latency_stats().average;
            let b_latency = b.2.latency_stats().average;

            a_latency.cmp(&b_latency)
        });

        // Count top-tier routes (don't need testing and meet stability requirement)
        let good_count = routes
            .iter()
            .take_while(|(_, rssd, stats)| {
                let needs_testing = stats.needs_testing(rssd.orderings(), cur_ts);
                let meets_stability = rssd.stability() >= params.stability;
                !needs_testing && meets_stability
            })
            .count();

        if good_count > 1 {
            // Round-robin among top-tier routes to distribute load
            let idx = route_selection_counter.fetch_add(1, Ordering::Relaxed) % good_count;
            routes.get(idx).map(|r| RouteIdAndKeys {
                route_id: r.0.clone(),
                route_set_keys: r.1.route_set_keys().clone(),
            })
        } else {
            // Only one or zero good routes, just return the best available
            routes.first().map(|r| RouteIdAndKeys {
                route_id: r.0.clone(),
                route_set_keys: r.1.route_set_keys().clone(),
            })
        }
    }
}
