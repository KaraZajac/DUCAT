use super::*;
use core::marker::PhantomData;

/// Inner data for EntrySnapshot
#[derive(Clone)]
struct EntrySnapshotInner {
    registry: VeilidComponentRegistry,
    cur_ts: Timestamp,
    entries: Vec<BucketEntrySnapshot>,
}

impl fmt::Debug for EntrySnapshotInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntrySnapshotInner")
            .field("cur_ts", &self.cur_ts)
            .field("entries_len", &self.entries.len())
            .finish()
    }
}

/// A point-in-time snapshot of all routing table entries.
/// Intentionally !Send and !Sync to prevent holding across await points,
/// since it contains NodeRefs that hold refcounts on BucketEntry.
#[derive(Debug)]
pub(crate) struct RoutingTableSnapshot {
    inner: Arc<EntrySnapshotInner>,
    _not_send: PhantomData<*const ()>,
}

impl Clone for RoutingTableSnapshot {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _not_send: PhantomData,
        }
    }
}

impl VeilidComponentRegistryAccessor for RoutingTableSnapshot {
    fn registry(&self) -> VeilidComponentRegistry {
        self.inner.registry.clone()
    }
}

impl RoutingTableSnapshot {
    pub fn new(
        registry: VeilidComponentRegistry,
        cur_ts: Timestamp,
        entries: Vec<BucketEntrySnapshot>,
    ) -> Self {
        Self {
            inner: Arc::new(EntrySnapshotInner {
                registry,
                cur_ts,
                entries,
            }),
            _not_send: PhantomData,
        }
    }

    #[expect(dead_code)]
    pub fn cur_ts(&self) -> Timestamp {
        self.inner.cur_ts
    }

    pub fn entries(&self) -> &[BucketEntrySnapshot] {
        &self.inner.entries
    }

    /// Build a set of all node ids for filtering against known nodes. With `min_state`, includes
    /// only entries at or above that state; with `None`, includes every captured entry.
    pub fn existing_node_ids(&self, min_state: Option<BucketEntryState>) -> HashSet<NodeId> {
        self.inner
            .entries
            .iter()
            .filter(|snap| min_state.is_none_or(|ms| snap.state >= ms))
            .flat_map(|snap| snap.node_ids.iter().cloned())
            .collect()
    }

    /// Consume and return the entries Vec.
    /// If there are other clones, this will clone the entries.
    #[expect(dead_code)]
    pub fn into_entries(self) -> Vec<BucketEntrySnapshot> {
        Arc::unwrap_or_clone(self.inner).entries
    }

    /// Get a list of nodes from the snapshot, sorted and filtered by the given criteria
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn get_peers_with_sort_and_filter<T, O>(
        &self,
        node_count: usize,
        cur_ts: Timestamp,
        mut filters: VecDeque<RoutingTableEntryFilter>,
        mut pre_sort_filter: RoutingTableEntryPreSortFilter,
        mut compare: RoutingTableEntrySort,
        transform: T,
    ) -> Vec<O>
    where
        T: FnMut(Option<BucketEntrySnapshot>) -> O,
    {
        // Collect all the nodes for sorting, with snapshots
        let self_entry: Option<BucketEntrySnapshot> = None;
        let include_self = filters.iter_mut().all(|filter| filter(&self_entry, cur_ts));

        let mut nodes: Vec<Option<BucketEntrySnapshot>> =
            Vec::with_capacity(self.inner.entries.len() + if include_self { 1 } else { 0 });

        // Include only nodes that match the filters
        for entry in self.inner.entries.iter().cloned().map(Some) {
            if filters.iter_mut().all(|filter| filter(&entry, cur_ts)) {
                nodes.push(entry);
            }
        }
        if include_self {
            nodes.push(None);
        }

        // Apply pre-sort filter
        pre_sort_filter(&mut nodes, cur_ts);

        // Sort by preference for returning nodes
        // Snapshots are immutable — no total-order violation possible
        nodes.sort_unstable_by(|a, b| compare(a, b, cur_ts));

        // Return transformed vector for filtered+sorted nodes
        nodes.into_iter().take(node_count).map(transform).collect()
    }

    /// Count entries that match some criteria
    #[expect(dead_code)]
    pub fn get_entry_count(
        &self,
        routing_domain_set: RoutingDomainSet,
        min_state: BucketEntryState,
        crypto_kinds: &[CryptoKind],
    ) -> usize {
        let mut count = 0usize;
        for entry in &self.inner.entries {
            if entry.state >= min_state
                && !entry.routing_domain_set().is_disjoint(routing_domain_set)
                && !common_crypto_kinds(&entry.crypto_kinds(), crypto_kinds).is_empty()
            {
                count += 1;
            }
        }
        count
    }
}
