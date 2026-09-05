use super::*;

/// Ephemeral data for allocated routes
#[derive(Debug)]
pub struct AllocatedRouteCacheEntry {
    /// Keys for this route's crypto kinds
    route_set_keys: PublicKeyGroup,

    /// Secrets for this route's crypto kinds
    route_set_secrets: SecretKeyGroup,

    /// Route noderefs
    hop_node_refs: Vec<NodeRef>,

    /// Hop cache key representing this route's node permutation order
    hop_cache_key: RouteHopCacheKey,

    /// Automatically allocated route vs manually allocated route
    automatic: bool,

    /// Directions this route is guaranteed to work in
    directions: DirectionSet,

    /// Stability preference (prefer reliable nodes over faster)
    stability: Stability,

    /// Sequencing capability (connection oriented protocols vs datagram)
    orderings: SequenceOrderingSet,

    /// If the route is published yet or not (upon node restart, this is reset to false to force republication)
    published: AtomicBool,

    /// In-use reference count for the whole set (AllocatedRouteSetRef)
    lock_count: AtomicUsize,

    /// Per-route in-use reference counts (AllocatedRouteRef), keyed by the
    /// route's PublicKey. Slots initialized for every key in `route_set_keys`.
    per_route_lock_counts: BTreeMap<PublicKey, AtomicUsize>,

    /// Sticky terminal flag: route is dead, release it once its refcount hits zero
    marked_for_release: AtomicBool,

    /// Stats
    stats: RwLock<RouteStats>,
}

impl fmt::Display for AllocatedRouteCacheEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (latency_stats, transfer_stats) = self.with_stats(|s| {
            (
                s.latency_stats().to_string(),
                s.transfer_stats().to_string(),
            )
        });
        write!(
            f,
            "count={} keys={} hops=[{}] auto={:?} dirs={:?} stability={:?} orderings={:?} published={:?} latency={} transfer={}",
            self.hop_node_refs.len(),
            self.route_set_keys,
            self.hop_node_refs.iter().map(|h| h.to_string()).collect::<Vec<_>>().join(","),
            self.automatic,
            self.directions,
            self.stability,
            self.orderings,
            self.published.load(Ordering::Relaxed),
            f.to_string(&latency_stats),
            f.to_string(&transfer_stats)
        )
    }
}
impl AllocatedRouteCacheEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        route_set_keys: PublicKeyGroup,
        route_set_secrets: SecretKeyGroup,
        hop_node_refs: Vec<NodeRef>,
        hop_cache_key: RouteHopCacheKey,
        automatic: bool,
        directions: DirectionSet,
        stability: Stability,
        orderings: SequenceOrderingSet,
        stats: RouteSetSpecDetailStats,
    ) -> Self {
        let per_route_lock_counts = route_set_keys
            .iter()
            .map(|k| (k.clone(), AtomicUsize::new(0)))
            .collect();
        AllocatedRouteCacheEntry {
            route_set_keys,
            route_set_secrets,
            hop_node_refs,
            hop_cache_key,
            automatic,
            directions,
            stability,
            orderings,
            published: AtomicBool::new(false),
            lock_count: AtomicUsize::new(0),
            per_route_lock_counts,
            marked_for_release: AtomicBool::new(false),
            stats: RwLock::new(RouteStats::new_from_spec_detail_stats(stats)),
        }
    }

    pub fn route_set_keys(&self) -> &PublicKeyGroup {
        &self.route_set_keys
    }

    pub fn route_set_secret_for_key(&self, key: &PublicKey) -> VeilidAPIResult<SecretKey> {
        if !self.route_set_keys.contains(key) {
            apibail_internal!("route set key not found: {}", key);
        }
        let Some(secret_key) = self.route_set_secrets.get(key.kind()) else {
            apibail_internal!("route set secret not found for key: {}", key);
        };
        Ok(secret_key)
    }

    pub fn best_route_set_key(&self) -> Option<PublicKey> {
        self.route_set_keys().first().cloned()
    }

    #[must_use]
    pub fn is_published(&self) -> bool {
        self.published.load(Ordering::Relaxed)
    }
    pub fn set_published(&self, published: bool) {
        self.published.store(published, Ordering::Relaxed);
    }

    /// Add one in-flight use of the whole set (held by AllocatedRouteSetRef)
    pub(super) fn lock(&self) {
        self.lock_count.fetch_add(1, Ordering::AcqRel);
    }
    /// Release `count` in-flight set-level uses, saturating at zero
    pub(super) fn unlock(&self, count: usize) {
        // fetch_update deprecated in newer std; move to try_update when MSRV >= 1.96
        #[allow(deprecated)]
        let _ = self
            .lock_count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |c| {
                Some(c.saturating_sub(count))
            });
    }

    /// Add one in-flight use of a specific route within this set
    /// (held by AllocatedRouteRef). No-op if `key` is not in the set.
    pub(super) fn lock_route(&self, key: &PublicKey) {
        if let Some(c) = self.per_route_lock_counts.get(key) {
            c.fetch_add(1, Ordering::AcqRel);
        }
    }
    /// Release `count` in-flight uses of a specific route, saturating at zero.
    /// No-op if `key` is not in the set.
    pub(super) fn unlock_route(&self, key: &PublicKey, count: usize) {
        if let Some(c) = self.per_route_lock_counts.get(key) {
            // fetch_update deprecated in newer std; move to try_update when MSRV >= 1.96
            #[allow(deprecated)]
            let _ = c.fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                Some(v.saturating_sub(count))
            });
        }
    }

    /// Whether any in-flight operation currently holds this set or any of its routes
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.lock_count.load(Ordering::Acquire) > 0
            || self
                .per_route_lock_counts
                .values()
                .any(|c| c.load(Ordering::Acquire) > 0)
    }

    /// Mark this route dead; it will be released once its refcount hits zero
    pub fn mark_for_release(&self) {
        self.marked_for_release.store(true, Ordering::Release);
    }
    #[must_use]
    pub fn is_marked_for_release(&self) -> bool {
        self.marked_for_release.load(Ordering::Acquire)
    }
    #[must_use]
    pub fn hop_count(&self) -> usize {
        self.hop_node_refs.len()
    }
    pub fn hop_node_refs(&self) -> Vec<NodeRef> {
        self.hop_node_refs.clone()
    }
    pub fn hop_node_ref(&self, idx: usize) -> Option<NodeRef> {
        self.hop_node_refs.get(idx).cloned()
    }
    pub fn hop_cache_key(&self) -> RouteHopCacheKey {
        self.hop_cache_key.clone()
    }

    pub fn contains_nodes(&self, nodes: &[NodeId]) -> bool {
        for node in nodes {
            for hop_node_ref in self.hop_node_refs.iter() {
                if hop_node_ref.node_ids().contains(node) {
                    return true;
                }
            }
        }
        false
    }

    pub fn is_automatic(&self) -> bool {
        self.automatic
    }

    pub fn directions(&self) -> DirectionSet {
        self.directions
    }

    pub fn stability(&self) -> Stability {
        self.stability
    }

    pub fn orderings(&self) -> SequenceOrderingSet {
        self.orderings
    }

    /// Whether the route is a usable match for a sequencing, excluding routes whose matching
    /// ordering(s) are all dead. Death-based: the route is a live match if at least one ordering
    /// it provides matches the sequencing and is not dead, so brand-new/untested routes stay
    /// selectable.
    pub fn is_live_sequencing_match(&self, sequencing: Sequencing) -> bool {
        if self.is_marked_for_release() {
            return false;
        }
        self.with_stats(|stats| {
            self.orderings.iter().any(|ordering| {
                sequencing.matches_ordering(ordering) && !stats.is_dead_for_ordering(ordering)
            })
        })
    }

    pub fn is_route_optimizable(&self) -> bool {
        self.with_stats(|stats| stats.last_known_valid_ts().is_some())
    }

    pub fn with_stats<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&RouteStats) -> R,
    {
        let stats = self.stats.read();
        f(&stats)
    }

    pub(super) fn with_stats_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut RouteStats) -> R,
    {
        let mut stats = self.stats.write();
        f(&mut stats)
    }
}
