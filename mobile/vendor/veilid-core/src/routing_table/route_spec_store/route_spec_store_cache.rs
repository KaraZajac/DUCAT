use super::*;
impl_veilid_log_facility!("rtab::route");

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[must_use]
struct UsedNodesKey {
    node_id: NodeId,
    hop_count: usize,
}

/// A key for the cache that can be used to uniquely identify this route's contents
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[must_use]
pub struct RouteHopCacheKey {
    data: Bytes,
}

impl RouteHopCacheKey {
    pub fn from_hop_node_refs(crypto_kind: CryptoKind, hop_node_refs: &[NodeRef]) -> Self {
        let mut data: BytesMut =
            BytesMut::with_capacity(hop_node_refs.len() * HASH_COORDINATE_LENGTH);
        for hop_node_ref in hop_node_refs {
            let best_hop_node_hc = hop_node_ref
                .node_ids()
                .get(crypto_kind)
                .unwrap_or_log()
                .to_hash_coordinate();
            data.extend_from_slice(best_hop_node_hc.ref_value());
        }

        Self {
            data: data.freeze(),
        }
    }
}

/// Ephemeral data used to help the RouteSpecStore operate efficiently
#[derive(Debug)]
pub(super) struct RouteSpecStoreCache {
    /// Registry accessor
    registry: VeilidComponentRegistry,
    /// How many times nodes have been used
    used_nodes: HashMap<UsedNodesKey, usize>,
    /// How many times nodes have been used at the terminal point of a route
    used_end_nodes: HashMap<UsedNodesKey, usize>,
    /// How many allocated routes use each hop set
    hop_cache: HashMap<RouteHopCacheKey, usize>,

    /// Allocated route info by route id
    allocated_route_set_cache: HashMap<AllocatedRouteSetId, Arc<AllocatedRouteCacheEntry>>,
    /// Allocated route ids indexed by route's public key
    allocated_routes_by_key: HashMap<PublicKey, AllocatedRouteSetId>,

    /// Remote private routes we've imported and statistics
    remote_route_set_cache: LruCache<RemoteRouteSetId, Arc<RemoteRouteCacheEntry>>,
    /// Remote private route ids indexed by route's public key
    remote_routes_by_key: HashMap<PublicKey, RemoteRouteSetId>,

    /// List of dead allocated routes
    dead_allocated_routes: Vec<AllocatedRouteSetId>,
    /// List of dead remote routes
    dead_remote_routes: Vec<RemoteRouteSetId>,
}

impl_veilid_component_accessors!(RouteSpecStoreCache);

impl RouteSpecStoreCache {
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            registry,
            used_nodes: Default::default(),
            used_end_nodes: Default::default(),
            hop_cache: Default::default(),
            allocated_route_set_cache: Default::default(),
            allocated_routes_by_key: Default::default(),
            remote_route_set_cache: LruCache::new(REMOTE_ROUTE_CACHE_SIZE),
            remote_routes_by_key: HashMap::new(),
            dead_allocated_routes: Default::default(),
            dead_remote_routes: Default::default(),
        }
    }

    /// Add an allocated route set to our cache via its cache key
    pub fn add_allocated_route(
        &mut self,
        id: AllocatedRouteSetId,
        rssd: &RouteSetSpecDetail,
        hop_node_refs: Vec<NodeRef>,
    ) -> VeilidAPIResult<()> {
        let route_set_keys = rssd.get_route_set_keys();
        let route_set_secrets = rssd.get_route_set_secrets();

        if route_set_keys.is_empty() {
            apibail_internal!("route set keys should not be empty: id={}", id);
        }

        if route_set_secrets.kinds() != route_set_keys.kinds() {
            apibail_internal!(
                "route set secrets should have the same kinds as route set keys: id={}",
                id
            );
        }

        if hop_node_refs.is_empty() {
            apibail_internal!("hop node refs should not be empty: id={}", id);
        }

        // Same public key is never permitted; duplicate hop sets are
        for key in route_set_keys.iter() {
            if self.allocated_routes_by_key.contains_key(key) {
                apibail_internal!("route with duplicate public key: key={}, id={}", key, id);
            }
        }

        let hop_count = hop_node_refs.len();

        let best_crypto_kind = route_set_keys.first().unwrap_or_log().kind();
        let hop_cache_key = RouteHopCacheKey::from_hop_node_refs(best_crypto_kind, &hop_node_refs);
        self.hop_cache
            .entry(hop_cache_key.clone())
            .and_modify(|e| *e += 1)
            .or_insert(1);

        let arce = AllocatedRouteCacheEntry::new(
            route_set_keys,
            route_set_secrets,
            hop_node_refs.clone(),
            hop_cache_key,
            rssd.is_automatic(),
            rssd.get_directions(),
            rssd.get_stability(),
            rssd.get_orderings(),
            rssd.get_stats(),
        );

        // store in id by key table
        for key in arce.route_set_keys().iter() {
            self.allocated_routes_by_key.insert(key.clone(), id.clone());
        }

        // store entry by id table
        self.allocated_route_set_cache.insert(id, Arc::new(arce));

        // store used nodes caches
        for (idx, hop_node_ref) in hop_node_refs.iter().enumerate() {
            for node_id in hop_node_ref.node_ids().iter() {
                let key = UsedNodesKey {
                    node_id: node_id.clone(),
                    hop_count: hop_node_refs.len(),
                };
                if idx == hop_count - 1 {
                    self.used_end_nodes
                        .entry(key.clone())
                        .and_modify(|e| *e += 1)
                        .or_insert(1);
                }
                self.used_nodes
                    .entry(key)
                    .and_modify(|e| *e += 1)
                    .or_insert(1);
            }
        }

        Ok(())
    }

    /// Checks if an allocated route is in our cache
    pub fn contains_allocated_route_with_hops(&self, cache_key: &RouteHopCacheKey) -> bool {
        self.hop_cache.contains_key(cache_key)
    }

    /// Removes an allocated route set from our cache
    pub fn remove_allocated_route(&mut self, id: AllocatedRouteSetId, is_automatic: bool) -> bool {
        let Some(arce) = self.allocated_route_set_cache.remove(&id) else {
            return false;
        };

        // remove from id by key table
        for key in arce.route_set_keys().iter() {
            if self.allocated_routes_by_key.remove(key).is_none() {
                veilid_log!(self error "allocated_routes_by_key should have contained key: key={}, id={}", key, id);
            }
        }

        // Remove from hop cache table
        let cache_key = arce.hop_cache_key();
        match self.hop_cache.entry(cache_key) {
            std::collections::hash_map::Entry::Occupied(mut o) => {
                *o.get_mut() -= 1;
                if *o.get() == 0 {
                    o.remove();
                }
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                veilid_log!(self error "hop cache should have contained cache key: id={}", id);
            }
        }

        // Remove from used nodes caches
        let hop_node_refs = arce.hop_node_refs();
        let hop_count = hop_node_refs.len();
        for (idx, hop_node_ref) in hop_node_refs.iter().enumerate() {
            for node_id in hop_node_ref.node_ids().iter() {
                let key = UsedNodesKey {
                    node_id: node_id.clone(),
                    hop_count: hop_node_refs.len(),
                };
                if idx == hop_count - 1 {
                    match self.used_end_nodes.entry(key.clone()) {
                        std::collections::hash_map::Entry::Occupied(mut o) => {
                            *o.get_mut() -= 1;
                            if *o.get() == 0 {
                                o.remove();
                            }
                        }
                        std::collections::hash_map::Entry::Vacant(_) => {
                            veilid_log!(self error "used_end_nodes cache should have contained hop");
                        }
                    }
                }
                match self.used_nodes.entry(key) {
                    std::collections::hash_map::Entry::Occupied(mut o) => {
                        *o.get_mut() -= 1;
                        if *o.get() == 0 {
                            o.remove();
                        }
                    }
                    std::collections::hash_map::Entry::Vacant(_) => {
                        veilid_log!(self error "used_nodes cache should have contained hop");
                    }
                }
            }
        }

        // Mark it as dead for the update if it wasn't automatically created
        if !is_automatic {
            self.dead_allocated_routes.push(id);
        }

        true
    }

    /// Get an allocated route by id
    pub fn get_allocated_route_by_id(
        &self,
        id: &AllocatedRouteSetId,
    ) -> Option<Arc<AllocatedRouteCacheEntry>> {
        self.allocated_route_set_cache.get(id).cloned()
    }

    /// Look up an allocated route set id by one of the route public keys
    pub fn get_allocated_route_id_by_key(&self, key: &PublicKey) -> Option<AllocatedRouteSetId> {
        self.allocated_routes_by_key.get(key).cloned()
    }

    /// Get the number of allocated routes in the cache
    pub fn get_allocated_route_count(&self) -> usize {
        self.allocated_route_set_cache.len()
    }

    /// Iterate all of the allocated routes we have in the cache
    pub fn iter_allocated_routes(
        &self,
    ) -> impl Iterator<Item = (&AllocatedRouteSetId, &Arc<AllocatedRouteCacheEntry>)> {
        self.allocated_route_set_cache.iter()
    }

    /// Calculate how many times a node with a particular node id set has been used anywhere in the path of our allocated routes
    pub fn get_used_node_count(&self, node_ids: &NodeIdGroup, hop_count: usize) -> usize {
        node_ids.iter().fold(0usize, |acc, k| {
            let key = UsedNodesKey {
                node_id: k.clone(),
                hop_count,
            };
            acc + self.used_nodes.get(&key).cloned().unwrap_or_default()
        })
    }

    /// Add remote private route to caches
    fn insert_remote_route_cache_entry(
        &mut self,
        id: RemoteRouteSetId,
        rrce: Arc<RemoteRouteCacheEntry>,
    ) {
        veilid_log!(self debug "Adding remote route to cache: {}, keys=[{}]", id, rrce.get_private_routes().iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","));

        // also store in id by key table
        for private_route in rrce.get_private_routes() {
            self.remote_routes_by_key
                .insert(private_route.public_key.clone(), id.clone());
        }

        let mut dead = None;
        self.remote_route_set_cache
            .insert_with_callback(id, rrce, |dead_id, dead_rrce| {
                dead = Some((dead_id, dead_rrce));
            });

        if let Some((dead_id, dead_rrce)) = dead {
            // If anything LRUs out, remove from the by-key table
            // Follow the same logic as 'remove_remote_private_route' here
            let mut dead_keys = Vec::new();
            for dead_private_route in dead_rrce.get_private_routes() {
                let _ = self
                    .remote_routes_by_key
                    .remove(&dead_private_route.public_key)
                    .unwrap_or_log();
                dead_keys.push(&dead_private_route.public_key);
            }
            self.dead_remote_routes.push(dead_id);
        }
    }

    /// Iterate all of the remote private routes we have in the cache
    pub fn get_remote_route_ids(&self, cur_ts: Timestamp) -> Vec<RemoteRouteSetId> {
        self.remote_route_set_cache
            .iter()
            .filter_map(|(id, rrce)| {
                if !rrce.did_expire(cur_ts) {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Remote private route cache accessor
    ///
    /// Will LRU entries and may expire entries and not return them if they are stale
    pub fn get_remote_route(
        &mut self,
        cur_ts: Timestamp,
        id: &RemoteRouteSetId,
    ) -> Option<Arc<RemoteRouteCacheEntry>> {
        if let Some(rrce) = self.remote_route_set_cache.get(id) {
            if !rrce.did_expire(cur_ts) {
                rrce.touch(cur_ts);
                return Some(rrce.clone());
            }
        }
        None
    }

    /// Remote private route cache accessor without LRU action
    ///
    /// Will not LRU entries but may expire entries and not return them if they are stale
    pub fn peek_remote_route(
        &self,
        cur_ts: Timestamp,
        id: &RemoteRouteSetId,
    ) -> Option<Arc<RemoteRouteCacheEntry>> {
        if let Some(rrce) = self.remote_route_set_cache.peek(id) {
            if !rrce.did_expire(cur_ts) {
                rrce.touch(cur_ts);
                return Some(rrce.clone());
            }
        }
        None
    }

    /// Look up a remote private route id by one of the route public keys
    pub fn get_remote_route_id_by_key(&self, key: &PublicKey) -> Option<RemoteRouteSetId> {
        self.remote_routes_by_key.get(key).cloned()
    }

    /// Get or create a remote private route cache entry
    /// may LRU and/or expire other cache entries to make room for the new one
    /// or update an existing entry with the same private route set
    /// returns the route set id
    pub fn add_remote_route(
        &mut self,
        cur_ts: Timestamp,
        id: RemoteRouteSetId,
        private_routes: Vec<Arc<PrivateRoute>>,
    ) {
        // get id for this route set
        if let Some(rrce) = self.get_remote_route(cur_ts, &id) {
            if rrce.did_expire(cur_ts) {
                // Start fresh if this had expired
                rrce.unexpire(cur_ts);
            } else {
                // If not expired, just mark as being used
                rrce.touch(cur_ts);
            }
        } else {
            // New remote private route cache entry
            let rrce = Arc::new(RemoteRouteCacheEntry::new(private_routes, cur_ts));

            self.insert_remote_route_cache_entry(id.clone(), rrce);
            if self.peek_remote_route(cur_ts, &id).is_none() {
                veilid_log!(self error "remote private route should exist");
            };
        };
    }

    /// Remove a remote private route from the cache
    pub fn remove_remote_route(&mut self, id: RemoteRouteSetId) -> bool {
        let Some(rrce) = self.remote_route_set_cache.remove(&id) else {
            return false;
        };
        veilid_log!(self debug "removing remote route from cache {}: keys=[{}]", id, rrce.get_private_routes().iter().map(|x| x.public_key.to_string()).collect::<Vec<_>>().join(","));

        let mut dead_keys = Vec::new();
        for private_route in rrce.get_private_routes() {
            let _ = self
                .remote_routes_by_key
                .remove(&private_route.public_key)
                .unwrap_or_log();

            dead_keys.push(&private_route.public_key);
        }
        self.dead_remote_routes.push(id);
        true
    }

    /// Take the dead local and remote routes so we can update clients
    pub fn take_dead_routes(
        &mut self,
    ) -> Option<(Vec<AllocatedRouteSetId>, Vec<RemoteRouteSetId>)> {
        if self.dead_allocated_routes.is_empty() && self.dead_remote_routes.is_empty() {
            // Nothing to do
            return None;
        }
        let dead_allocated_routes = core::mem::take(&mut self.dead_allocated_routes);
        let dead_remote_routes = core::mem::take(&mut self.dead_remote_routes);
        Some((dead_allocated_routes, dead_remote_routes))
    }

    /// Clean up allocated routes and imported remote routes when our peer info changes
    /// Resets statistics so we can test the routes again. Clears publication status
    /// so we can republish the routes.
    pub fn report_peer_info_changed(&self) {
        // Restart start for allocated routes so we test the route again
        for arce in self.allocated_route_set_cache.values() {
            // If the route is published it will need to be republished if our peer info changes
            if arce.is_published() {
                // Must republish route now
                arce.set_published(false);

                arce.with_stats_mut(|s| {
                    s.reset();
                });
            }
        }
        // Restart stats for routes so we test the route again
        for (_, rrce) in self.remote_route_set_cache.iter() {
            // Restart stats for routes so we test the route again
            rrce.with_stats_mut(|s| {
                s.reset();
            });
        }
    }

    /// Roll transfer statistics
    pub fn roll_transfers(&self, last_ts: Timestamp, cur_ts: Timestamp) {
        for arce in self.allocated_route_set_cache.values() {
            arce.with_stats_mut(|s| {
                s.roll_transfers(last_ts, cur_ts);
            });
        }

        for (_, rrce) in self.remote_route_set_cache.iter() {
            rrce.with_stats_mut(|s| {
                s.roll_transfers(last_ts, cur_ts);
            });
        }
    }

    /// Roll answer statistics
    pub fn roll_answers(&self, cur_ts: Timestamp) {
        for arce in self.allocated_route_set_cache.values() {
            arce.with_stats_mut(|s| {
                s.roll_answers(cur_ts);
            });
        }
        for (_, rrce) in self.remote_route_set_cache.iter() {
            rrce.with_stats_mut(|s| {
                s.roll_answers(cur_ts);
            });
        }
    }

    pub fn update_allocated_route_stats<F>(
        &self,
        _cur_ts: Timestamp,
        key: &PublicKey,
        f: F,
    ) -> VeilidAPIResult<()>
    where
        F: FnOnce(&mut RouteStats) -> VeilidAPIResult<()>,
    {
        if let Some(rid) = self.get_allocated_route_id_by_key(key) {
            if let Some(arce) = self.get_allocated_route_by_id(&rid) {
                arce.with_stats_mut(f)?;
            }
        }

        Ok(())
    }

    pub fn update_remote_route_stats<F>(
        &self,
        cur_ts: Timestamp,
        key: &PublicKey,
        f: F,
    ) -> VeilidAPIResult<()>
    where
        F: FnOnce(&mut RouteStats) -> VeilidAPIResult<()>,
    {
        if let Some(rrid) = self.get_remote_route_id_by_key(key) {
            if let Some(rrce) = self.peek_remote_route(cur_ts, &rrid) {
                rrce.with_stats_mut(f)?;
            }
        }

        Ok(())
    }
}
