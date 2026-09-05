use super::*;

use weak_table::PtrWeakHashSet;

impl_veilid_log_facility!("rtab");

pub const RECENT_PEERS_TABLE_SIZE: usize = 64;

#[derive(Debug)]
pub struct NodeRelativePerformance {
    pub percentile: f32,
    pub node_index: usize,
    pub node_count: usize,
}

//////////////////////////////////////////////////////////////////////////

/// RoutingTable rwlock-internal data
#[must_use]
pub struct RoutingTableInner {
    /// Convenience accessor for the global component registry
    pub(super) registry: VeilidComponentRegistry,
    /// Routing table buckets that hold references to entries, per crypto kind
    pub(super) buckets: BTreeMap<CryptoKind, Vec<Bucket>>,
    /// A weak set of all the entries we have in the buckets for faster iteration
    pub(super) all_entries: PtrWeakHashSet<Weak<BucketEntry>>,
}

impl_veilid_component_accessors!(RoutingTableInner);

impl RoutingTableInner {
    pub(super) fn new(registry: VeilidComponentRegistry) -> RoutingTableInner {
        RoutingTableInner {
            registry: registry.clone(),
            buckets: BTreeMap::new(),
            all_entries: PtrWeakHashSet::new(),
        }
    }

    pub fn bucket_entry_count(&self) -> usize {
        self.all_entries.len()
    }

    pub fn reset_all_updated_since_last_network_change(&mut self) {
        let cur_ts = Timestamp::now();
        self.with_entries(cur_ts, BucketEntryState::Dead, |v| {
            v.with_mut(|e| {
                e.reset_updated_since_last_network_change();
            });
            Option::<()>::None
        });
    }

    pub(in crate::routing_table) fn bucket_depth(pos: usize) -> usize {
        match pos {
            0 => 256,
            1 => 128,
            2 => 64,
            3 => 32,
            4 => 16,
            5 => 8,
            6 => 4,
            7 => 2,
            _ => 1,
        }
    }

    /// Per-bucket entry counts for a given crypto kind, indexed by bucket position.
    pub(in crate::routing_table) fn bucket_counts(&self, ck: CryptoKind) -> Vec<usize> {
        let Some(buckets) = self.buckets.get(&ck) else {
            return Vec::new();
        };
        buckets.iter().map(|b| b.len()).collect()
    }

    /// Total count of entries across all buckets (any crypto kind) that exceed
    /// their bucket's max depth and haven't been kicked yet. This is "over-attached"
    /// in the bucket-fill sense — capacity is overrun, lazy kicking will trim later.
    pub(in crate::routing_table) fn excess_kickable_count(&self) -> usize {
        let mut total = 0usize;
        for buckets in self.buckets.values() {
            for (pos, bucket) in buckets.iter().enumerate() {
                let depth = Self::bucket_depth(pos);
                if bucket.len() > depth {
                    total += bucket.len() - depth;
                }
            }
        }
        total
    }

    pub fn init_buckets(&mut self) {
        // Size the buckets (one per bit), one bucket set per crypto kind
        self.buckets.clear();
        for ck in VALID_CRYPTO_KINDS {
            let mut ckbuckets = Vec::with_capacity(BUCKET_COUNT);
            for _ in 0..BUCKET_COUNT {
                let bucket = Bucket::new(self.registry(), ck);
                ckbuckets.push(bucket);
            }
            self.buckets.insert(ck, ckbuckets);
        }
    }

    /// Attempt to empty the routing table
    /// should only be performed when there are no node_refs (detached)
    pub fn purge_buckets(&mut self) {
        veilid_log!(self trace
            "Starting routing table buckets purge. Table currently has {} nodes",
            self.bucket_entry_count()
        );
        let closest_nodes = BTreeSet::new();
        for ck in VALID_CRYPTO_KINDS {
            for bucket in self.buckets.get_mut(&ck).unwrap_or_log().iter_mut() {
                bucket.kick(0, &closest_nodes);
            }
        }
        self.all_entries.remove_expired();

        veilid_log!(self debug
            "Routing table buckets purge complete. Routing table now has {} nodes",
            self.bucket_entry_count()
        );
    }

    /// Attempt to remove last_connections from entries
    pub fn purge_last_connections(&self) {
        veilid_log!(self trace "Starting routing table last_connections purge.");
        for ck in VALID_CRYPTO_KINDS {
            for bucket in &self.buckets[&ck] {
                for entry in bucket.entries() {
                    entry.1.with_mut(|e| {
                        e.clear_last_flows(DialInfoFilter::all());
                    });
                }
            }
        }
        veilid_log!(self debug "Routing table last_connections purge complete.");
    }

    /// Attempt to settle buckets and remove entries down to the desired number
    /// which may not be possible due extant NodeRefs
    pub fn kick_bucket(&mut self, bucket_index: BucketIndex, exempt_peers: &BTreeSet<BareNodeId>) {
        let bucket = self.get_bucket_mut(bucket_index);
        let bucket_depth = Self::bucket_depth(bucket_index.1);

        if let Some(dead_node_ids) = bucket.kick(bucket_depth, exempt_peers) {
            // Remove expired entries
            self.all_entries.remove_expired();

            veilid_log!(self debug "Bucket {}:{} kicked Routing table now has {} nodes\nKicked nodes:{:#?}", bucket_index.0, bucket_index.1, self.bucket_entry_count(), dead_node_ids);
        }
    }

    /// Iterate entries with a filter
    pub fn with_entries<T, F: FnMut(Arc<BucketEntry>) -> Option<T>>(
        &self,
        cur_ts: Timestamp,
        min_state: BucketEntryState,
        mut f: F,
    ) -> Option<T> {
        for entry in &self.all_entries {
            if entry.with(|e| e.state(cur_ts) >= min_state) {
                entry.ref_count.fetch_add(1u32, Ordering::AcqRel);
                if let Some(out) = f(entry.clone()) {
                    entry.ref_count.fetch_sub(1u32, Ordering::AcqRel);
                    return Some(out);
                }
                entry.ref_count.fetch_sub(1u32, Ordering::AcqRel);
            }
        }
        None
    }

    pub fn get_bucket_mut(&mut self, bucket_index: BucketIndex) -> &mut Bucket {
        self.buckets
            .get_mut(&bucket_index.0)
            .unwrap_or_log()
            .get_mut(bucket_index.1)
            .unwrap_or_log()
    }

    pub fn get_bucket(&self, bucket_index: BucketIndex) -> &Bucket {
        self.buckets
            .get(&bucket_index.0)
            .unwrap_or_log()
            .get(bucket_index.1)
            .unwrap_or_log()
    }

    pub fn snapshot(
        &self,
        registry: VeilidComponentRegistry,
        cur_ts: Timestamp,
        min_state: BucketEntryState,
    ) -> RoutingTableSnapshot {
        // #[cfg(feature = "verbose-tracing")]
        // let start_ts = Timestamp::now();

        let mut entries = Vec::with_capacity(self.bucket_entry_count());
        self.with_entries(cur_ts, min_state, |v| {
            entries.push(v.snapshot(registry.clone(), cur_ts));
            Option::<()>::None
        });

        // #[cfg(feature = "verbose-tracing")]
        // {
        //     let elapsed = Timestamp::now().duration_since(start_ts);
        //     veilid_log!(self debug "snapshot_entries: {} entries in {:#}", entries.len(), elapsed);
        // }

        RoutingTableSnapshot::new(registry, cur_ts, entries)
    }
}
