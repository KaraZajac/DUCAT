use super::*;

/// RAII handle on an allocated route set. While any `AllocatedRouteSetRef`
/// (or any `AllocatedRouteRef` to a route within the set) is held, the cache
/// entry will not be released; on the last drop, if the set was marked dead
/// during use, the route is released immediately.
///
/// Mirrors the NodeRef pattern used for BucketEntry holds.
///
/// Drop takes the route spec store's cache WRITE lock if a release is needed.
/// Do not drop a `*SetRef` while holding any cache lock yourself.
#[must_use]
pub(crate) struct AllocatedRouteSetRef {
    registry: VeilidComponentRegistry,
    entry: Arc<AllocatedRouteCacheEntry>,
    set_id: AllocatedRouteSetId,
}

impl_veilid_component_accessors!(AllocatedRouteSetRef);

impl AllocatedRouteSetRef {
    pub(super) fn new(
        registry: VeilidComponentRegistry,
        entry: Arc<AllocatedRouteCacheEntry>,
        set_id: AllocatedRouteSetId,
    ) -> Self {
        entry.lock();
        Self {
            registry,
            entry,
            set_id,
        }
    }

    pub fn entry(&self) -> &AllocatedRouteCacheEntry {
        &self.entry
    }

    pub fn route_set_id(&self) -> &AllocatedRouteSetId {
        &self.set_id
    }
}

impl Clone for AllocatedRouteSetRef {
    fn clone(&self) -> Self {
        self.entry.lock();
        Self {
            registry: self.registry.clone(),
            entry: self.entry.clone(),
            set_id: self.set_id.clone(),
        }
    }
}

impl Drop for AllocatedRouteSetRef {
    fn drop(&mut self) {
        self.entry.unlock(1);
        if self.entry.is_marked_for_release() && !self.entry.is_locked() {
            self.routing_table()
                .route_spec_store()
                .release_allocated_route(self.set_id.clone());
        }
    }
}

impl fmt::Debug for AllocatedRouteSetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AllocatedRouteSetRef")
            .field("set_id", &self.set_id)
            .finish()
    }
}

impl fmt::Display for AllocatedRouteSetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", f.to_string(&self.set_id))
    }
}

/// RAII handle on a remote (imported) route set. See `AllocatedRouteSetRef`
/// for the lifecycle contract; the remote variant is identical except that
/// release flips through `release_remote_route_id` instead.
#[must_use]
pub(crate) struct RemoteRouteSetRef {
    registry: VeilidComponentRegistry,
    entry: Arc<RemoteRouteCacheEntry>,
    set_id: RemoteRouteSetId,
}

impl_veilid_component_accessors!(RemoteRouteSetRef);

impl RemoteRouteSetRef {
    pub(super) fn new(
        registry: VeilidComponentRegistry,
        entry: Arc<RemoteRouteCacheEntry>,
        set_id: RemoteRouteSetId,
    ) -> Self {
        entry.lock();
        Self {
            registry,
            entry,
            set_id,
        }
    }

    pub fn entry(&self) -> &RemoteRouteCacheEntry {
        &self.entry
    }

    pub fn route_set_id(&self) -> &RemoteRouteSetId {
        &self.set_id
    }
}

impl Clone for RemoteRouteSetRef {
    fn clone(&self) -> Self {
        self.entry.lock();
        Self {
            registry: self.registry.clone(),
            entry: self.entry.clone(),
            set_id: self.set_id.clone(),
        }
    }
}

impl Drop for RemoteRouteSetRef {
    fn drop(&mut self) {
        self.entry.unlock(1);
        // Remote routes don't have a marked-for-release flag today; remote
        // releases are driven by LRU eviction and explicit removals, not by
        // ref-count transitions. We just decrement here.
    }
}

impl fmt::Debug for RemoteRouteSetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteRouteSetRef")
            .field("set_id", &self.set_id)
            .finish()
    }
}

impl fmt::Display for RemoteRouteSetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", f.to_string(&self.set_id))
    }
}

impl RouteSpecStore {
    /// Acquire an `AllocatedRouteSetRef` by route set id. Returns None if
    /// the set is not in the cache.
    #[expect(dead_code)]
    pub(super) fn lock_allocated_route_set_by_id(
        &self,
        set_id: &AllocatedRouteSetId,
    ) -> Option<AllocatedRouteSetRef> {
        let entry = self.cache.read().get_allocated_route_by_id(set_id)?;
        Some(AllocatedRouteSetRef::new(
            self.registry(),
            entry,
            set_id.clone(),
        ))
    }

    /// Acquire an `AllocatedRouteSetRef` by one of the set's route public keys.
    /// Returns None if no allocated set contains that key.
    pub(super) fn lock_allocated_route_set_by_key(
        &self,
        key: &PublicKey,
    ) -> Option<AllocatedRouteSetRef> {
        let cache = self.cache.read();
        let set_id = cache.get_allocated_route_id_by_key(key)?;
        let entry = cache.get_allocated_route_by_id(&set_id)?;
        Some(AllocatedRouteSetRef::new(self.registry(), entry, set_id))
    }

    /// Acquire a `RemoteRouteSetRef` by route set id. Returns None if the
    /// set is not in the cache (or has expired).
    #[expect(dead_code)]
    pub(super) fn lock_remote_route_set_by_id(
        &self,
        set_id: &RemoteRouteSetId,
    ) -> Option<RemoteRouteSetRef> {
        let cur_ts = Timestamp::now();
        let entry = self.cache.read().peek_remote_route(cur_ts, set_id)?;
        Some(RemoteRouteSetRef::new(
            self.registry(),
            entry,
            set_id.clone(),
        ))
    }

    /// Acquire a `RemoteRouteSetRef` by one of the set's route public keys.
    /// Returns None if no remote set contains that key (or it has expired).
    pub(super) fn lock_remote_route_set_by_key(
        &self,
        key: &PublicKey,
    ) -> Option<RemoteRouteSetRef> {
        let cur_ts = Timestamp::now();
        let cache = self.cache.read();
        let set_id = cache.get_remote_route_id_by_key(key)?;
        let entry = cache.peek_remote_route(cur_ts, &set_id)?;
        Some(RemoteRouteSetRef::new(self.registry(), entry, set_id))
    }
}
