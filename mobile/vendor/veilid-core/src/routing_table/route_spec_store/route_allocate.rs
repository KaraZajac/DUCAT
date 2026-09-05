use super::*;

impl_veilid_log_facility!("rtab");

type PermReturnType = (Vec<usize>, SequenceOrderingSet);
type PermFunc<'t> = Box<dyn FnMut(&[usize]) -> Option<PermReturnType> + Send + 't>;

const ALLOCATION_HIGH_PASS_FILTER_FACTOR: f32 = 0.8;

/// Progressive relaxation for route allocation: drop ipblock diversity first
/// (small networks can't satisfy it), then allow duplicate hop sets as a last resort
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RouteAllocationPass {
    UniqueIpblockDiverse,
    Unique,
    AllowDuplicateHops,
}

/// get the route permutation at particular 'perm' index, starting at the 'start' index
/// for a set of 'hop_count' nodes. the first node is always fixed, and the maximum
/// number of permutations is (hop_count-1)! (the orderings with the first node fixed)
fn with_route_permutations(
    hop_count: usize,
    start: usize,
    f: &mut PermFunc,
) -> Option<PermReturnType> {
    if hop_count == 0 {
        return None;
    }
    // initial permutation
    let mut permutation: Vec<usize> = Vec::with_capacity(hop_count);
    for n in 0..hop_count {
        permutation.push(start + n);
    }
    // if we have one hop or two, then there's only one permutation
    if hop_count == 1 || hop_count == 2 {
        return f(&permutation);
    }

    // heaps algorithm, but skipping the first element
    fn heaps_permutation(
        permutation: &mut [usize],
        size: usize,
        f: &mut PermFunc,
    ) -> Option<PermReturnType> {
        if size == 1 {
            return f(permutation);
        }

        for i in 0..size {
            let out = heaps_permutation(permutation, size - 1, f);
            if out.is_some() {
                return out;
            }
            if size % 2 == 1 {
                permutation.swap(1, size);
            } else {
                permutation.swap(1 + i, size);
            }
        }

        None
    }

    // recurse
    heaps_permutation(&mut permutation, hop_count - 1, f)
}

#[derive(Clone, Debug)]
pub struct AllocateRouteParams {
    pub crypto_kinds: Vec<CryptoKind>,
    pub hop_count: usize,
    pub stability: Stability,
    pub sequencing: Sequencing,
    pub directions: DirectionSet,
    pub avoid_nodes: Vec<NodeId>,
    pub automatic: bool,
}

/// Tiered relay-capable hop preference for route allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RelayPolicy {
    /// Every hop must be relay-capable.
    AllHops,
    /// Only the last hop must be relay-capable.
    LastHopOnly,
    /// No relay-capable preference; any reachable route is acceptable.
    None,
}

impl RouteSpecStore {
    /// Create a new route set
    /// Prefers nodes that are not currently in use by another route
    /// The route is not yet tested for its reachability
    /// Returns Err(VeilidAPIError::TryAgain) if no route could be allocated at this time
    /// Returns other errors on failure
    /// Returns Ok(route id and public keys) on success
    #[cfg_attr(feature = "instrument", instrument(level = "trace", target="rtab::route", skip(self), ret, err(level=Level::TRACE), fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn allocate_route(
        &self,
        params: AllocateRouteParams,
    ) -> VeilidAPIResult<RouteIdAndKeys> {
        let allocate_route_lock_guard = self.allocate_route_lock.lock().await;
        self.allocate_route_inner(&allocate_route_lock_guard, params)
            .await
    }

    pub async fn allocate_route_inner(
        &self,
        _lock: &AsyncMutexGuard<'_, ()>,
        mut params: AllocateRouteParams,
    ) -> VeilidAPIResult<RouteIdAndKeys> {
        if params.hop_count < 1 {
            apibail_invalid_argument!(
                "Not allocating route less than one hop in length",
                "hop_count",
                params.hop_count
            );
        }

        if params.hop_count > self.get_max_route_hop_count() {
            apibail_invalid_argument!(
                "Not allocating route longer than max route hop count",
                "hop_count",
                params.hop_count
            );
        }
        if params.crypto_kinds.is_empty() {
            apibail_missing_argument!("No crypto kinds provided", "crypto_kinds");
        }

        // Ensure best crypto kind is first
        params.crypto_kinds.sort_unstable();

        // Get our peer info
        let cur_ts = Timestamp::now();
        let (hop_node_refs, orderings, hop_node_ids_per_crypto_kind, allocation_diagnostic) = {
            let routing_table = self.routing_table();

            let Some(published_peer_info) =
                routing_table.get_published_peer_info(RoutingDomain::PublicInternet)
            else {
                apibail_try_again!(
                    "unable to allocate route until we have a valid PublicInternet network class"
                );
            };

            // Take a snapshot to allocate from
            let snapshot = routing_table.snapshot_entries(cur_ts, BucketEntryState::Unreliable);

            // Make the node filter
            let filter =
                self.make_route_allocation_entry_filter(&params, published_peer_info.clone());

            let cache = self.cache.read();

            let filters = VecDeque::from([filter]);
            let compare = self.make_route_allocation_entry_sort(
                &cache,
                cur_ts,
                &params,
                published_peer_info.clone(),
            );
            let pre_sort_filter = self.make_route_allocation_pre_sort_filter(&params);
            let transform = |entry: Option<BucketEntrySnapshot>| -> BucketEntrySnapshot {
                entry.unwrap_or_log()
            };

            // Pull the whole routing table in sorted order
            let nodes: Vec<BucketEntrySnapshot> = snapshot.get_peers_with_sort_and_filter(
                usize::MAX,
                cur_ts,
                filters,
                pre_sort_filter,
                compare,
                transform,
            );

            // If we couldn't find enough nodes, wait until we have more nodes in the routing table
            if nodes.len() < params.hop_count {
                veilid_log!(self debug "not enough nodes to construct route at this time: ({}/{})", nodes.len(), params.hop_count);
                apibail_try_again!("not enough nodes to construct route at this time");
            }

            // Try progressively looser relay-capable preferences. Tier 0 picks
            // routes whose every hop is relay-capable; Tier 1 only the last
            // hop; Tier 2 accepts any reachable route. Cold-start small
            // networks fall through to looser tiers automatically.
            let mut route_nodes: Vec<usize> = Vec::new();
            let mut orderings = SequenceOrderingSet::new();

            // Number of contiguous hop-count windows we can start a route at.
            let window_count = nodes.len() - params.hop_count + 1;

            // Relax in stages when windows come up empty: drop ipblock diversity,
            // then allow duplicate hop sets. Relaxed passes start at a random
            // window so repeated duplicates spread out.
            let mut start_offset = 0usize;
            for pass in [
                RouteAllocationPass::UniqueIpblockDiverse,
                RouteAllocationPass::Unique,
                RouteAllocationPass::AllowDuplicateHops,
            ] {
                for policy in [
                    RelayPolicy::AllHops,
                    RelayPolicy::LastHopOnly,
                    RelayPolicy::None,
                ] {
                    let mut perm_func = self.make_route_allocation_permutation_function(
                        &cache,
                        &nodes,
                        &params,
                        published_peer_info.clone(),
                        policy,
                        pass,
                    );
                    for w in 0..window_count {
                        let start = (w + start_offset) % window_count;
                        if let Some((rn, ord)) =
                            with_route_permutations(params.hop_count, start, &mut perm_func)
                        {
                            route_nodes = rn;
                            orderings = ord;
                            break;
                        }
                    }
                    if !route_nodes.is_empty() {
                        break;
                    }
                }
                if !route_nodes.is_empty() {
                    break;
                }
                start_offset = get_random_u32() as usize % window_count;
            }

            if route_nodes.is_empty() {
                apibail_try_again!("unable to find any route at this time");
            }

            drop(cache);

            // Got a unique route, lets build the details, register it, and return it
            let hop_node_refs: Vec<NodeRef> = route_nodes
                .iter()
                .map(|k| nodes[*k].node_ref.clone())
                .collect();
            let mut hop_node_ids_per_crypto_kind = Vec::new();
            for crypto_kind in params.crypto_kinds.iter().copied() {
                let hops: Vec<NodeId> = route_nodes
                    .iter()
                    .map(|v| nodes[*v].node_ids.get(crypto_kind).unwrap_or_log())
                    .collect();
                hop_node_ids_per_crypto_kind.push((crypto_kind, hops));
            }

            // Diagnostic: capture the chosen route's per-hop contact methods so we can
            // cross-reference failing routes against allocation-time topology. Gated
            // behind verbose-tracing because the get_contact_method walk has cost.
            #[cfg(feature = "verbose-tracing")]
            let allocation_diagnostic: Option<String> = {
                let routing_table = self.routing_table();
                let pick_kind = params.crypto_kinds.first().copied();
                let hop_pis: Vec<Arc<PeerInfo>> = route_nodes
                    .iter()
                    .filter_map(|&k| nodes[k].get_peer_info(RoutingDomain::PublicInternet))
                    .collect();
                let mut entries: Vec<String> = Vec::with_capacity(hop_pis.len());
                for (i, hop_pi) in hop_pis.iter().enumerate() {
                    let node_id = pick_kind
                        .and_then(|k| hop_pi.node_ids().get(k))
                        .map(|n| format!("{}", n))
                        .unwrap_or_else(|| "?".to_string());
                    let cm_out = if params.directions.contains(Direction::Out) {
                        let prev_pi = if i == 0 {
                            published_peer_info.clone()
                        } else {
                            hop_pis[i - 1].clone()
                        };
                        routing_table.get_best_contact_method(
                            RoutingDomain::PublicInternet,
                            ContactMethodRequest {
                                peer_a: prev_pi,
                                peer_a_published: true,
                                peer_b: hop_pi.clone(),
                                dial_info_filter: DialInfoFilter::all(),
                                sequencing: params.sequencing,
                            },
                        )
                    } else {
                        None
                    };
                    let cm_in = if params.directions.contains(Direction::In) {
                        let next_pi = if i + 1 < hop_pis.len() {
                            hop_pis[i + 1].clone()
                        } else {
                            published_peer_info.clone()
                        };
                        routing_table.get_best_contact_method(
                            RoutingDomain::PublicInternet,
                            ContactMethodRequest {
                                peer_a: next_pi,
                                peer_a_published: true,
                                peer_b: hop_pi.clone(),
                                dial_info_filter: DialInfoFilter::all(),
                                sequencing: params.sequencing,
                            },
                        )
                    } else {
                        None
                    };
                    entries.push(format!(
                        "[{}] node={} cm_out={:?} cm_in={:?}",
                        i, node_id, cm_out, cm_in
                    ));
                }
                Some(entries.join(" | "))
            };
            #[cfg(not(feature = "verbose-tracing"))]
            let allocation_diagnostic: Option<String> = None;

            // Drop routing table inner read lock since we don't need it during crypto operations
            (
                hop_node_refs,
                orderings,
                hop_node_ids_per_crypto_kind,
                allocation_diagnostic,
            )
        };

        let mut route_set = BTreeMap::<PublicKey, RouteSpecDetail>::new();
        let crypto = self.crypto();
        for (crypto_kind, hops) in hop_node_ids_per_crypto_kind {
            let Some(vcrypto) = crypto.get_async(crypto_kind) else {
                apibail_invalid_argument!(
                    "no crypto system for crypto kind",
                    "crypto_kinds",
                    crypto_kind
                );
            };
            let keypair = vcrypto.generate_keypair().await;
            route_set.insert(
                keypair.key(),
                RouteSpecDetail {
                    secret_key: keypair.secret(),
                    hops,
                },
            );
        }

        if route_set.is_empty() {
            apibail_generic!("no route set available for crypto kinds provided");
        }

        let rssd = RouteSetSpecDetail::new(
            route_set,
            params.directions,
            params.stability,
            orderings,
            params.automatic,
        )?;

        // Make route id
        let route_id = self.generate_allocated_route_id(&rssd)?;

        // Get public keys to return
        let route_set_keys = rssd.get_route_set_keys();

        // Debug print the allocated route
        veilid_log!(self debug "Allocated route: id={}\n    {}\n    keys={}", route_id, rssd, route_set_keys);

        // Verbose-tracing diagnostic: per-hop contact methods at the time of allocation,
        // for cross-referencing failing routes against allocation-time topology.
        #[cfg(feature = "verbose-tracing")]
        if let Some(diag) = &allocation_diagnostic {
            veilid_log!(self debug "Allocated route detail: id={} directions={:?} hops={}", route_id, params.directions, diag);
        }
        #[cfg(not(feature = "verbose-tracing"))]
        let _ = &allocation_diagnostic;

        // Add to cache and keep route in spec store
        {
            // Careful with locking order here, we need to lock the content before the cache
            let mut content = self.content.write();
            let mut cache = self.cache.write();

            cache.add_allocated_route(route_id.clone(), &rssd, hop_node_refs)?;
            content.add_detail(route_id.clone(), rssd);
        }

        // Notify subscribers (e.g. ConnectionManager for first-hop protection refresh)
        if let Err(e) = self.event_bus().post(RouteAllocationEvent {
            allocated: vec![route_id.clone().into()],
            released: vec![],
        }) {
            veilid_log!(self warn "failed to post RouteAllocationEvent: {}", e);
        }

        Ok(RouteIdAndKeys {
            route_id,
            route_set_keys,
        })
    }

    /// Release an allocated route that is no longer in use
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip(self), ret, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) fn release_allocated_route(&self, id: AllocatedRouteSetId) -> bool {
        let released_id = {
            // Careful with locking order here, we need to lock the content before the cache
            let mut content = self.content.write();
            let mut cache = self.cache.write();

            let Some(rssd) = content.remove_detail(&id) else {
                return false;
            };
            veilid_log!(self debug "releasing allocated route {}: keys={}", id, rssd.get_route_set_keys());

            // Remove from hop cache
            let id_clone = id.clone();
            if !cache.remove_allocated_route(id, rssd.is_automatic()) {
                veilid_log!(self error "hop cache should have contained cache key");
            }
            id_clone
        };

        // Notify subscribers (e.g. ConnectionManager for first-hop protection refresh)
        if let Err(e) = self.event_bus().post(RouteAllocationEvent {
            allocated: vec![],
            released: vec![released_id.into()],
        }) {
            veilid_log!(self warn "failed to post RouteAllocationEvent: {}", e);
        }

        true
    }

    fn make_route_allocation_entry_filter<'t>(
        &self,
        params: &'t AllocateRouteParams,
        published_peer_info: Arc<PeerInfo>,
    ) -> RoutingTableEntryFilter<'t> {
        self.make_route_eligible_node_filter(
            &params.crypto_kinds,
            params.sequencing,
            &params.avoid_nodes,
            published_peer_info,
        )
    }

    /// Eligibility filter for any node that could serve as a route hop, or as a route
    /// test destination. Excludes our own node, our relay, denylisted countries,
    /// local-network nodes, no-route-capability nodes, no-compatible-dial-info nodes,
    /// not-yet-pinged nodes, and same-IP-block nodes. Reused by route allocation
    /// (`make_route_allocation_entry_filter`) and by route-test destination picking
    /// (`route_get_testing_destinations`).
    pub(crate) fn make_route_eligible_node_filter<'t>(
        &self,
        crypto_kinds: &'t [CryptoKind],
        sequencing: Sequencing,
        avoid_nodes: &'t [NodeId],
        published_peer_info: Arc<PeerInfo>,
    ) -> RoutingTableEntryFilter<'t> {
        // Get our relay nodes if we have them
        let own_relay_nrs: HashSet<NodeRef> = self
            .routing_table()
            .relays(RoutingDomain::PublicInternet)
            .iter()
            .map(|x| x.relay_node.unfiltered())
            .collect();

        #[cfg(feature = "geolocation")]
        let country_code_denylist = self.config().network.privacy.country_code_denylist.clone();

        #[cfg(feature = "geolocation")]
        let registry = self.registry();
        Box::new(
            move |entry: &Option<BucketEntrySnapshot>, _cur_ts: Timestamp| -> bool {
                // Exclude our own node from routes
                let Some(snap) = entry else {
                    return false;
                };

                // Defensively exclude nodes that aren't responsive
                if !snap.state.is_responsive() {
                    return false;
                }

                // Exclude our relay if we have one
                if own_relay_nrs.contains(&snap.node_ref) {
                    return false;
                }

                // Exclude nodes that don't have our requested crypto kinds
                let common_ck: Vec<CryptoKind> = snap
                    .node_ids
                    .kinds()
                    .into_iter()
                    .filter(|k| crypto_kinds.contains(k))
                    .collect();
                if common_ck.len() != crypto_kinds.len() {
                    return false;
                }

                // Exclude nodes we have specifically chosen to avoid
                if snap.node_ids.contains_any_from_iter(avoid_nodes.iter()) {
                    return false;
                }

                // Exclude nodes on our local network
                if snap
                    .routing_domain_set()
                    .contains(RoutingDomain::LocalNetwork)
                {
                    return false;
                }

                // Exclude nodes that have no publicinternet signednodeinfo
                let Some(their_pi) = snap.get_peer_info(RoutingDomain::PublicInternet) else {
                    return false;
                };
                let their_ni = their_pi.node_info();

                // Exclude nodes with no compatible dialinfo
                if !their_ni.has_sequencing_matched_dial_info(sequencing) {
                    return false;
                }

                // Exclude nodes that have don't advertise route capability
                if !their_ni.has_capability(VEILID_CAPABILITY_ROUTE) {
                    return false;
                }

                // Exclude nodes from denylisted countries
                #[cfg(feature = "geolocation")]
                if !country_code_denylist.is_empty() {
                    let geolocation_info =
                        their_ni.get_geolocation_info(RoutingDomain::PublicInternet);

                    // Since denylist is used, consider nodes with unknown countries to be automatically excluded
                    let Some(node_country_code) = geolocation_info.country_code() else {
                        veilid_log!(registry debug target:"geolocation",
                            "make_route_eligible_node_filter: skipping node {:?} from unknown country",
                            snap.best_node_id()
                        );
                        return false;
                    };
                    // The same thing applies to relays used by the node
                    // They must all be from a known country
                    let relay_country_codes: Option<Vec<CountryCode>> = geolocation_info
                        .relay_country_codes()
                        .iter()
                        .cloned()
                        .collect();
                    let Some(relay_country_codes) = relay_country_codes else {
                        veilid_log!(registry debug target:"geolocation",
                        "make_route_eligible_node_filter: skipping node {:?} using relay from unknown country",
                            snap.best_node_id()
                        );
                        return false;
                    };

                    // Ensure that node is not excluded
                    if country_code_denylist.contains(&node_country_code) {
                        veilid_log!(registry debug target:"geolocation",
                            "make_route_eligible_node_filter: skipping node {:?} from excluded country {}",
                            snap.best_node_id(),
                            node_country_code
                        );
                        return false;
                    }

                    // Ensure that node relays are not excluded
                    if let Some(cc) = relay_country_codes
                        .iter()
                        .find(|cc| country_code_denylist.contains(cc))
                    {
                        veilid_log!(registry debug target:"geolocation",
                            "make_route_eligible_node_filter: skipping node {:?} using relay from excluded country {}",
                            snap.best_node_id(),
                            cc
                        );
                        return false;
                    }
                }

                // Filter out nodes that have our same public IP address
                // Use whole ipv6 address so we don't filter out nodes in the same network
                // These will be deprioritized in the sort later, though.
                if published_peer_info
                    .node_info()
                    .is_on_same_ipblock(their_ni, 128)
                {
                    return false;
                }

                // Relay check
                for their_relay_info in their_ni.relay_info_list() {
                    // Exclude nodes whose relays we have chosen to avoid
                    if their_relay_info
                        .node_ids()
                        .contains_any_from_iter(avoid_nodes.iter())
                    {
                        return false;
                    }
                }

                true
            },
        )
    }

    fn make_route_allocation_entry_sort<'t>(
        &self,
        cache: &'t RouteSpecStoreCache,
        _cur_ts: Timestamp,
        params: &'t AllocateRouteParams,
        published_peer_info: Arc<PeerInfo>,
    ) -> RoutingTableEntrySort<'t> {
        let ip6_prefix_size = self
            .config()
            .internal()
            .network
            .max_connections_per_ip6_prefix_size as usize;

        Box::new(
            move |entry1: &Option<BucketEntrySnapshot>,
                  entry2: &Option<BucketEntrySnapshot>,
                  _cur_ts: Timestamp|
                  -> cmp::Ordering {
                // Our own node is filtered out, so it is safe to unwrap here
                let snap1 = entry1.as_ref().unwrap_or_log();
                let snap2 = entry2.as_ref().unwrap_or_log();
                let entry1_peer_info = snap1
                    .get_peer_info(RoutingDomain::PublicInternet)
                    .unwrap_or_log();
                let entry1_node_info = entry1_peer_info.node_info();
                let entry2_peer_info = snap2
                    .get_peer_info(RoutingDomain::PublicInternet)
                    .unwrap_or_log();
                let entry2_node_info = entry2_peer_info.node_info();

                // deprioritize nodes we have used already anywhere for a route of the same hop count
                let e1_used = cache.get_used_node_count(&snap1.node_ids, params.hop_count);
                let e2_used = cache.get_used_node_count(&snap2.node_ids, params.hop_count);
                let cmp_used = e1_used.cmp(&e2_used);
                if !matches!(cmp_used, cmp::Ordering::Equal) {
                    return cmp_used;
                }

                // deprioritize nodes that are on our own ipv6 network
                // this check also checks if the ipv4 address is the same but we filtered that out already
                let e1_same_ipblock = published_peer_info
                    .node_info()
                    .is_any_node_on_same_ipblock(entry1_node_info, ip6_prefix_size);
                let e2_same_ipblock = published_peer_info
                    .node_info()
                    .is_any_node_on_same_ipblock(entry2_node_info, ip6_prefix_size);
                let cmp_same_ipblock = e1_same_ipblock.cmp(&e2_same_ipblock);
                if !matches!(cmp_same_ipblock, cmp::Ordering::Equal) {
                    return cmp_same_ipblock;
                }

                // apply sequencing preference
                // ensureordered will be taken care of by filter
                // and preferunordered doesn't care
                if matches!(params.sequencing, Sequencing::PreferOrdered) {
                    let e1_can_do_ordered =
                        entry1_node_info.has_sequencing_matched_dial_info(params.sequencing);
                    let e2_can_do_ordered =
                        entry2_node_info.has_sequencing_matched_dial_info(params.sequencing);
                    // Reverse this comparison because ordered is preferable (less)
                    let cmp_seq = e2_can_do_ordered.cmp(&e1_can_do_ordered);
                    if !matches!(cmp_seq, cmp::Ordering::Equal) {
                        return cmp_seq;
                    }
                }

                // apply stability preference
                // always prioritize reliable nodes, but sort by oldest or fastest
                match params.stability {
                    Stability::LowLatency => {
                        BucketEntrySnapshot::cmp_fastest_reliable(snap1, snap2, |ls| ls.tm90)
                    }
                    Stability::Reliable => BucketEntrySnapshot::cmp_oldest_reliable(snap1, snap2),
                }
            },
        ) as RoutingTableEntrySort
    }

    fn make_route_allocation_pre_sort_filter<'t>(
        &self,
        params: &'t AllocateRouteParams,
    ) -> RoutingTableEntryPreSortFilter<'t> {
        Box::new(
            move |all_entries: &mut Vec<Option<BucketEntrySnapshot>>, _cur_ts: Timestamp| {
                // Remove the slowest (100% - ALLOCATION_HIGH_PASS_FILTER_FACTOR) of the entries from consideration
                let mut sorted_entry_indices = (0..all_entries.len()).collect::<Vec<_>>();
                sorted_entry_indices.sort_unstable_by(|i1, i2| {
                    let snap1 = all_entries[*i1].as_ref().unwrap_or_log();
                    let snap2 = all_entries[*i2].as_ref().unwrap_or_log();

                    BucketEntrySnapshot::cmp_fastest(snap1, snap2, |ls| ls.tm90)
                });

                let reduce = (sorted_entry_indices.len() as f32
                    * (1.0 - ALLOCATION_HIGH_PASS_FILTER_FACTOR))
                    as usize;
                let keep_count = (sorted_entry_indices.len() - reduce).max(params.hop_count);
                if keep_count < sorted_entry_indices.len() {
                    for i in keep_count..sorted_entry_indices.len() {
                        all_entries[sorted_entry_indices[i]] = None;
                    }

                    // Retain only non-None entries
                    // This preserves the order of the entries while removing the slow ones
                    all_entries.retain(|x| x.is_some());
                }
            },
        ) as RoutingTableEntryPreSortFilter
    }

    // Get the hop cache key for a particular route permutation
    fn route_permutation_to_hop_cache_key(
        crypto_kind: CryptoKind,
        nodes: &[BucketEntrySnapshot],
        perm: &[usize],
    ) -> RouteHopCacheKey {
        let mut node_refs = Vec::<NodeRef>::with_capacity(perm.len());
        for n in perm {
            node_refs.push(nodes[*n].node_ref.clone());
        }

        RouteHopCacheKey::from_hop_node_refs(crypto_kind, &node_refs)
    }

    fn make_route_allocation_permutation_function<'t>(
        &self,
        cache: &'t RouteSpecStoreCache,
        nodes: &'t [BucketEntrySnapshot],
        params: &'t AllocateRouteParams,
        published_peer_info: Arc<PeerInfo>,
        relay_policy: RelayPolicy,
        pass: RouteAllocationPass,
    ) -> PermFunc<'t> {
        // Get peer info for everything
        let nodes_pi: Vec<Arc<PeerInfo>> = nodes
            .iter()
            .map(|nr| {
                nr.get_peer_info(RoutingDomain::PublicInternet)
                    .unwrap_or_log()
            })
            .collect();

        let registry = self.registry();
        let routing_table = self.routing_table();

        let ip6_prefix_size = self
            .config()
            .internal()
            .network
            .max_connections_per_ip6_prefix_size as usize;

        let relay_node_filter = {
            let rdc = routing_table.get_routing_domain_controller(RoutingDomain::PublicInternet);
            let rdd = rdc
                .as_any()
                .downcast_ref::<PublicInternetRoutingDomainController>()
                .unwrap()
                .read();
            rdd.make_relay_node_filter()
        };

        let best_crypto_kind = params.crypto_kinds.first().copied().unwrap_or_log();

        Box::new(move |permutation: &[usize]| {
            let routing_table = registry.routing_table();

            // Skip already-allocated hop sets except on the last-resort pass
            if pass != RouteAllocationPass::AllowDuplicateHops {
                let cache_key =
                    Self::route_permutation_to_hop_cache_key(best_crypto_kind, nodes, permutation);
                if cache.contains_allocated_route_with_hops(&cache_key) {
                    return None;
                }
            }

            // Ensure the route doesn't contain two nodes on the same ipblock or with relays on the same ipblock
            if pass == RouteAllocationPass::UniqueIpblockDiverse {
                let mut seen_ipblocks: HashSet<IpAddr> = HashSet::new();
                for n in permutation {
                    let node = nodes.get(*n).unwrap_or_log();
                    let peer_info = node.get_peer_info(RoutingDomain::PublicInternet)?;
                    let node_info = peer_info.node_info();

                    let ipblocks = node_info.get_ipblocks(ip6_prefix_size);
                    for ipblock in ipblocks {
                        if !seen_ipblocks.insert(ipblock) {
                            return None;
                        }
                    }
                    for relay_info in node_info.relay_info_list() {
                        let ipblocks = relay_info.get_ipblocks(ip6_prefix_size);
                        for ipblock in ipblocks {
                            if !seen_ipblocks.insert(ipblock) {
                                return None;
                            }
                        }
                    }
                }
            }

            // Ensure the route doesn't contain both a node and its relay
            let mut seen_nodes: HashSet<NodeId> = HashSet::new();
            for n in permutation {
                let node = nodes.get(*n).unwrap_or_log();
                for nid in node.node_ids.iter() {
                    if !seen_nodes.insert(nid.clone()) {
                        // Already seen this node, should not be in the route twice
                        return None;
                    }
                }

                let peer_info = node.get_peer_info(RoutingDomain::PublicInternet)?;
                let node_info = peer_info.node_info();
                for rid in node_info.relay_ids() {
                    if !seen_nodes.insert(rid.clone()) {
                        // Already seen this node, should not be in the route twice
                        return None;
                    }
                }
            }

            // Ensure this route is viable by checking that each node can contact the next one
            let mut orderings = SequenceOrderingSet::all();
            if params.directions.contains(Direction::Out) {
                let mut previous_node = published_peer_info.clone();
                let mut reachable = true;
                for n in permutation {
                    let current_node = nodes_pi.get(*n).cloned().unwrap_or_log();
                    let cm = routing_table.get_best_contact_method(
                        RoutingDomain::PublicInternet,
                        ContactMethodRequest {
                            peer_a: previous_node.clone(),
                            peer_a_published: true,
                            peer_b: current_node.clone(),
                            dial_info_filter: DialInfoFilter::all(),
                            sequencing: params.sequencing,
                        },
                    );
                    if cm.is_none() {
                        reachable = false;
                        break;
                    }

                    // Check if we can do each ordering strictly
                    for ordering in orderings {
                        let cm = routing_table.get_best_contact_method(
                            RoutingDomain::PublicInternet,
                            ContactMethodRequest {
                                peer_a: previous_node.clone(),
                                peer_a_published: true,
                                peer_b: current_node.clone(),
                                dial_info_filter: DialInfoFilter::all(),
                                sequencing: ordering.strict_sequencing(),
                            },
                        );
                        if cm.is_none() {
                            orderings.remove(ordering);
                        }
                    }

                    previous_node = current_node;
                }
                if !reachable {
                    return None;
                }
            }
            if params.directions.contains(Direction::In) {
                let mut next_node = published_peer_info.clone();
                let mut reachable = true;
                for n in permutation.iter().rev() {
                    let current_node = nodes_pi.get(*n).cloned().unwrap_or_log();
                    let cm = routing_table.get_best_contact_method(
                        RoutingDomain::PublicInternet,
                        ContactMethodRequest {
                            peer_a: next_node.clone(),
                            peer_a_published: true,
                            peer_b: current_node.clone(),
                            dial_info_filter: DialInfoFilter::all(),
                            sequencing: params.sequencing,
                        },
                    );
                    if cm.is_none() {
                        reachable = false;
                        break;
                    }

                    // Check if we can do each ordering strictly
                    for ordering in orderings {
                        let cm = routing_table.get_best_contact_method(
                            RoutingDomain::PublicInternet,
                            ContactMethodRequest {
                                peer_a: next_node.clone(),
                                peer_a_published: true,
                                peer_b: current_node.clone(),
                                dial_info_filter: DialInfoFilter::all(),
                                sequencing: ordering.strict_sequencing(),
                            },
                        );
                        if cm.is_none() {
                            orderings.remove(ordering);
                        }
                    }
                    next_node = current_node;
                }
                if !reachable {
                    return None;
                }
            }

            // Apply relay-capable preference per policy. Tier 0 (AllHops) excludes
            // SignalReverse/SignalHolePunch-only first hops and biases the whole
            // route toward stable infrastructure nodes; Tier 1 (LastHopOnly)
            // preserves the existing reachability-to-destination preference.
            match relay_policy {
                RelayPolicy::AllHops => {
                    for &n in permutation {
                        let hop_node = nodes.get(n).unwrap_or_log();
                        if !relay_node_filter(hop_node) {
                            return None;
                        }
                    }
                }
                RelayPolicy::LastHopOnly => {
                    let last_hop_idx = *permutation.last().unwrap_or_log();
                    let last_hop_node = nodes.get(last_hop_idx).unwrap_or_log();
                    if !relay_node_filter(last_hop_node) {
                        return None;
                    }
                }
                RelayPolicy::None => {}
            }

            // Keep this route
            let route_nodes = permutation.to_vec();
            Some((route_nodes, orderings))
        }) as PermFunc
    }

    /// Generate AllocatedRouteSetId from typed key set of route public keys
    fn generate_allocated_route_id(
        &self,
        rssd: &RouteSetSpecDetail,
    ) -> VeilidAPIResult<AllocatedRouteSetId> {
        let route_set_keys = rssd.get_route_set_keys();
        let crypto = self.crypto();

        let pkbyteslen = route_set_keys
            .iter()
            .fold(0, |acc, x| acc + x.ref_value().len());
        let mut pkbytes = Vec::with_capacity(pkbyteslen);
        let mut best_kind: Option<CryptoKind> = None;
        for tk in route_set_keys.iter() {
            if best_kind.is_none()
                || compare_crypto_kind(&tk.kind(), best_kind.as_ref().unwrap_or_log())
                    == cmp::Ordering::Less
            {
                best_kind = Some(tk.kind());
            }
            pkbytes.extend_from_slice(tk.ref_value());
        }
        let Some(best_kind) = best_kind else {
            apibail_internal!("no compatible crypto kinds in route");
        };
        let vcrypto = crypto.get(best_kind).unwrap_or_log();

        Ok(AllocatedRouteSetId::from_route_id(RouteId::new(
            vcrypto.kind(),
            BareRouteId::new(vcrypto.generate_hash(&pkbytes).ref_value()),
        )))
    }
}
