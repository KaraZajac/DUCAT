use super::*;

impl_veilid_log_facility!("rtab");

/// How many 'reliable' nodes closest to our own node id to keep
const KEEP_N_CLOSEST_RELIABLE_PEERS_COUNT: usize = 16;

/// How many 'unreliable' nodes closest to our own node id to keep
const KEEP_N_CLOSEST_UNRELIABLE_PEERS_COUNT: usize = 8;

impl RoutingTable {
    fn make_closest_bare_node_id_sort(
        bare_hash_coordinate: BareHashCoordinate,
    ) -> impl Fn(&BareNodeId, &BareNodeId) -> core::cmp::Ordering {
        move |a: &BareNodeId, b: &BareNodeId| -> core::cmp::Ordering {
            let da = a.to_bare_hash_coordinate().distance(&bare_hash_coordinate);
            let db = b.to_bare_hash_coordinate().distance(&bare_hash_coordinate);
            da.cmp(&db)
        }
    }

    // Kick the queued buckets in the routing table to free dead nodes if necessary
    // Attempts to keep the size of the routing table down to the bucket depth
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn kick_buckets_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        cur_ts: Timestamp,
    ) -> EyreResult<()> {
        let kick_queue: Vec<BucketIndex> = core::mem::take(&mut *self.kick_queue.lock())
            .into_iter()
            .collect();

        let mut exempt_peers_by_kind = BTreeMap::<CryptoKind, BTreeSet<BareNodeId>>::new();

        {
            let inner = self.inner.read();
            for kind in VALID_CRYPTO_KINDS {
                let our_node_id = self.node_id(kind);
                let Some(buckets) = inner.buckets.get(&kind) else {
                    continue;
                };
                let sort = Self::make_closest_bare_node_id_sort(
                    our_node_id.ref_value().to_bare_hash_coordinate(),
                );

                let mut closest_peers = BTreeSet::<BareNodeId>::new();
                let mut closest_unreliable_count = 0usize;
                let mut closest_reliable_count = 0usize;

                // Iterate buckets backward, sort entries by closest distance first
                'outer: for bucket in buckets.iter().rev() {
                    let mut entries = bucket.entries().collect::<Vec<_>>();
                    entries.sort_unstable_by(|a, b| sort(a.0, b.0));
                    for (key, entry) in entries {
                        // See if this entry is a distance-metric capability node
                        // If not, disqualify it from this closest_nodes list
                        if !entry.with(|e| {
                            e.has_any_capabilities(
                                RoutingDomain::PublicInternet,
                                DISTANCE_METRIC_CAPABILITIES,
                            )
                        }) {
                            continue;
                        }

                        let state = entry.with(|e| e.state(cur_ts));
                        match state {
                            BucketEntryState::Dead
                            | BucketEntryState::Punished
                            | BucketEntryState::Missing
                            | BucketEntryState::Initial => {
                                // Do nothing for unresponsive entries
                            }
                            BucketEntryState::Unreliable => {
                                // Add to closest unreliable nodes list
                                if closest_unreliable_count < KEEP_N_CLOSEST_UNRELIABLE_PEERS_COUNT
                                {
                                    closest_peers.insert(key.clone());
                                    closest_unreliable_count += 1;
                                }
                            }
                            BucketEntryState::Reliable => {
                                // Add to closest reliable nodes list
                                if closest_reliable_count < KEEP_N_CLOSEST_RELIABLE_PEERS_COUNT {
                                    closest_peers.insert(key.clone());
                                    closest_reliable_count += 1;
                                }
                            }
                        }
                        if closest_unreliable_count == KEEP_N_CLOSEST_UNRELIABLE_PEERS_COUNT
                            && closest_reliable_count == KEEP_N_CLOSEST_RELIABLE_PEERS_COUNT
                        {
                            break 'outer;
                        }
                    }
                }

                exempt_peers_by_kind.insert(kind, closest_peers);
            }
        }

        // Now that we have a kick queue and exempt peers, we can kick the buckets
        {
            let mut inner = self.inner.write();
            for bucket_index in kick_queue {
                inner.kick_bucket(bucket_index, &exempt_peers_by_kind[&bucket_index.0]);
            }
        }
        Ok(())
    }
}
