use super::*;

/// RAII handle on one specific route (by `PublicKey`) within an allocated set.
/// Holds an inner `AllocatedRouteSetRef` (set-level lock + release cascade) and
/// additionally pins the per-route lock.
#[must_use]
pub(crate) struct AllocatedRouteRef {
    set_ref: AllocatedRouteSetRef,
    route_key: PublicKey,
}

impl AllocatedRouteRef {
    pub(super) fn new(set_ref: AllocatedRouteSetRef, route_key: PublicKey) -> Self {
        set_ref.entry().lock_route(&route_key);
        Self { set_ref, route_key }
    }

    pub fn entry(&self) -> &AllocatedRouteCacheEntry {
        self.set_ref.entry()
    }

    pub fn route_set_id(&self) -> &AllocatedRouteSetId {
        self.set_ref.route_set_id()
    }

    pub fn route_key(&self) -> &PublicKey {
        &self.route_key
    }

    /// The deduplicated node ids of this route's hops (read directly from the
    /// held cache entry; no cache lookup).
    pub fn hop_node_ids(&self) -> HashSet<NodeId> {
        self.entry()
            .hop_node_refs()
            .iter()
            .map(|nr| nr.best_node_id())
            .collect()
    }

    /// Mutate this route's stats (held cache entry; no lookup).
    pub fn with_stats_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut RouteStats) -> R,
    {
        self.entry().with_stats_mut(f)
    }
}

impl Clone for AllocatedRouteRef {
    fn clone(&self) -> Self {
        Self::new(self.set_ref.clone(), self.route_key.clone())
    }
}

impl Drop for AllocatedRouteRef {
    fn drop(&mut self) {
        // Per-route unlock first; set_ref field drops after (set unlock + maybe release).
        self.set_ref.entry().unlock_route(&self.route_key, 1);
    }
}

impl fmt::Debug for AllocatedRouteRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AllocatedRouteRef")
            .field("set_id", self.set_ref.route_set_id())
            .field("route_key", &self.route_key)
            .finish()
    }
}

impl fmt::Display for AllocatedRouteRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Default: the route's own key (parseable). Alternate: set_id:route_key (human).
        if f.alternate() {
            write!(
                f,
                "{}:{}",
                f.to_string(self.set_ref.route_set_id()),
                f.to_string(&self.route_key)
            )
        } else {
            write!(f, "{}", f.to_string(&self.route_key))
        }
    }
}

/// RAII handle on one specific route (by `PublicKey`) within a remote set.
#[must_use]
pub(crate) struct RemoteRouteRef {
    set_ref: RemoteRouteSetRef,
    route_key: PublicKey,
}

impl RemoteRouteRef {
    pub(super) fn new(set_ref: RemoteRouteSetRef, route_key: PublicKey) -> Self {
        set_ref.entry().lock_route(&route_key);
        Self { set_ref, route_key }
    }

    pub fn entry(&self) -> &RemoteRouteCacheEntry {
        self.set_ref.entry()
    }

    pub fn route_key(&self) -> &PublicKey {
        &self.route_key
    }

    /// The first hop node id of this remote private route (the remaining hops
    /// are encrypted). Read directly from the held cache entry; no cache lookup.
    pub fn first_hop_node_id(&self) -> Option<NodeId> {
        let pr = self.entry().best_private_route()?;
        match &pr.hops {
            PrivateRouteHops::FirstHop(rh) => rh.node.node_id(),
            _ => None,
        }
    }

    /// Mutate this route's stats (held cache entry; no lookup).
    pub fn with_stats_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut RouteStats) -> R,
    {
        self.entry().with_stats_mut(f)
    }

    /// Mark this remote private route as having seen our currently published
    /// node info, so we can optimize sending it. Errors if our peer info isn't
    /// published yet. PRIVACY: we never accept node-info timestamps from remote
    /// private routes — only stamp our own published timestamp here.
    pub fn mark_seen_our_node_info(&self) -> VeilidAPIResult<()> {
        let Some(our_node_info_ts) = self
            .set_ref
            .routing_table()
            .get_published_peer_info(RoutingDomain::PublicInternet)
            .map(|pi| pi.node_info().timestamp())
        else {
            apibail_internal!("peer info is not yet published");
        };
        self.entry()
            .set_last_seen_our_node_info_ts(our_node_info_ts);
        Ok(())
    }
}

impl Clone for RemoteRouteRef {
    fn clone(&self) -> Self {
        Self::new(self.set_ref.clone(), self.route_key.clone())
    }
}

impl Drop for RemoteRouteRef {
    fn drop(&mut self) {
        // Per-route unlock first; set_ref field drops after (set unlock).
        self.set_ref.entry().unlock_route(&self.route_key, 1);
    }
}

impl fmt::Debug for RemoteRouteRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteRouteRef")
            .field("set_id", self.set_ref.route_set_id())
            .field("route_key", &self.route_key)
            .finish()
    }
}

impl fmt::Display for RemoteRouteRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Default: the route's own key (parseable). Alternate: set_id:route_key (human).
        if f.alternate() {
            write!(
                f,
                "{}:{}",
                f.to_string(self.set_ref.route_set_id()),
                f.to_string(&self.route_key)
            )
        } else {
            write!(f, "{}", f.to_string(&self.route_key))
        }
    }
}

impl RouteSpecStore {
    /// Acquire an `AllocatedRouteRef` pinning the allocated route with this key.
    pub fn lock_allocated_route_by_key(&self, route_key: &PublicKey) -> Option<AllocatedRouteRef> {
        let set_ref = self.lock_allocated_route_set_by_key(route_key)?;
        Some(AllocatedRouteRef::new(set_ref, route_key.clone()))
    }

    /// Acquire a `RemoteRouteRef` pinning the remote route with this key.
    pub fn lock_remote_route_by_key(&self, route_key: &PublicKey) -> Option<RemoteRouteRef> {
        let set_ref = self.lock_remote_route_set_by_key(route_key)?;
        Some(RemoteRouteRef::new(set_ref, route_key.clone()))
    }
}
