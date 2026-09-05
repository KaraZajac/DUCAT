use super::*;

/// What remote private routes have seen
#[derive(Debug, Default)]
pub struct RemoteRouteCacheEntry {
    /// The private routes themselves
    private_routes: Vec<Arc<PrivateRoute>>,
    /// Did this remote private route see our node info due to no safety route in use
    last_seen_our_node_info_ts: AtomicOptionTimestamp,
    /// Last time this remote private route was requested for any reason (cache expiration)
    last_touched_ts: AtomicTimestamp,
    /// In-use reference count for the whole set (RemoteRouteSetRef)
    lock_count: AtomicUsize,
    /// Per-route in-use reference counts (RemoteRouteRef), keyed by the private
    /// route's PublicKey. Slots initialized for every key in `private_routes`.
    per_route_lock_counts: BTreeMap<PublicKey, AtomicUsize>,
    /// Stats
    stats: RwLock<RouteStats>,
}

impl fmt::Display for RemoteRouteCacheEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (latency_stats, transfer_stats) = self.with_stats(|s| {
            (
                s.latency_stats().to_string(),
                s.transfer_stats().to_string(),
            )
        });
        write!(
            f,
            "last_seen_our_ni={} last_touched={} latency={} transfer={}",
            f.to_string(&self.last_seen_our_node_info_ts),
            f.to_string(&self.last_touched_ts),
            f.to_string(&latency_stats),
            f.to_string(&transfer_stats)
        )
    }
}
impl RemoteRouteCacheEntry {
    pub fn new(private_routes: Vec<Arc<PrivateRoute>>, cur_ts: Timestamp) -> Self {
        let per_route_lock_counts = private_routes
            .iter()
            .map(|pr| (pr.public_key.clone(), AtomicUsize::new(0)))
            .collect();
        RemoteRouteCacheEntry {
            private_routes,
            last_seen_our_node_info_ts: AtomicOptionTimestamp::none(),
            last_touched_ts: AtomicTimestamp::new(cur_ts),
            lock_count: AtomicUsize::new(0),
            per_route_lock_counts,
            stats: RwLock::new(RouteStats::new(cur_ts)),
        }
    }

    /// Add one in-flight use of the whole set (held by RemoteRouteSetRef)
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
    /// (held by RemoteRouteRef). No-op if `key` is not in the set.
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
    pub fn get_private_routes(&self) -> &[Arc<PrivateRoute>] {
        &self.private_routes
    }
    pub fn best_private_route(&self) -> Option<Arc<PrivateRoute>> {
        self.private_routes
            .iter()
            .reduce(|acc, x| {
                if x.public_key < acc.public_key {
                    x
                } else {
                    acc
                }
            })
            .filter(|x| VALID_CRYPTO_KINDS.contains(&x.public_key.kind()))
            .cloned()
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

    pub fn has_seen_our_node_info_ts(&self, our_node_info_ts: Timestamp) -> bool {
        self.last_seen_our_node_info_ts.get() == Some(our_node_info_ts)
    }
    pub fn set_last_seen_our_node_info_ts(&self, last_seen_our_node_info_ts: Timestamp) {
        self.last_seen_our_node_info_ts
            .set(Some(last_seen_our_node_info_ts));
    }

    // Check to see if this remote private route has expired
    pub fn did_expire(&self, cur_ts: Timestamp) -> bool {
        self.last_touched_ts
            .expiration_state(cur_ts, REMOTE_PRIVATE_ROUTE_CACHE_EXPIRY)
            == ExpirationState::Dead
    }

    /// Start fresh if this had expired. Stats reset; the in-use refcount is
    /// separate and untouched, so in-flight route refs still unlock cleanly.
    pub fn unexpire(&self, cur_ts: Timestamp) {
        self.last_seen_our_node_info_ts.set(None);
        self.last_touched_ts.set(cur_ts);
        *self.stats.write() = RouteStats::new(cur_ts);
    }

    /// Note when this was last used
    pub fn touch(&self, cur_ts: Timestamp) {
        self.last_touched_ts.set(cur_ts)
    }
}
