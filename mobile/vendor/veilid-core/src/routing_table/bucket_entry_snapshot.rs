use super::*;

#[derive(Debug)]
pub(crate) struct PerRoutingDomainSnapshot {
    pub peer_info: Arc<PeerInfo>,
    pub node_status: Option<NodeStatus>,
    pub last_seen_our_node_info_ts: Timestamp,
}

#[derive(Debug)]
pub(crate) struct BucketEntrySnapshotInner {
    pub cur_ts: Timestamp,
    pub node_ref: NodeRef,
    pub time_added: Timestamp,
    pub peer_stats: PeerStats,
    pub rpc_stats: RPCStats,
    pub connection_stats: ConnectionStats,
    pub state: BucketEntryState,
    pub node_ids: NodeIdGroup,
    pub per_routing_domain: BTreeMap<RoutingDomain, PerRoutingDomainSnapshot>,
    pub per_sequence_ordering: BTreeMap<SequenceOrdering, RPCStats>,
    pub per_transport: BTreeMap<TransportType, RPCStats>,
}

/// A point-in-time snapshot of mutable BucketEntry fields used for sorting and filtering.
/// Created once before sorting to avoid total-order violations from concurrent
/// updates between comparisons (Rust 1.81+ driftsort validates total ordering).
///
/// Contains a `NodeRef` for creating `FilteredNodeRef` in transforms, and frozen
/// copies of all mutable fields needed by sort/filter closures. `Option<BucketEntrySnapshot>`
/// where `None` represents the self node.
#[derive(Clone, Debug)]
pub(crate) struct BucketEntrySnapshot {
    inner: Arc<BucketEntrySnapshotInner>,
}

impl core::ops::Deref for BucketEntrySnapshot {
    type Target = BucketEntrySnapshotInner;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl BucketEntrySnapshot {
    pub(super) fn new(inner: BucketEntrySnapshotInner) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }

    pub fn crypto_kinds(&self) -> Vec<CryptoKind> {
        self.node_ids.iter().map(|x| x.kind()).collect()
    }

    pub fn routing_domain_set(&self) -> RoutingDomainSet {
        self.per_routing_domain.keys().cloned().collect()
    }

    pub fn is_reliable(&self) -> bool {
        self.state == BucketEntryState::Reliable
    }

    // pub fn is_reliable_for(&self, transport: LowLevelTransportType) -> bool {
    //     self.per_transport
    //         .get(&transport)
    //         .map(|t| t.state == LowLevelState::Reliable)
    //         .unwrap_or(false)
    // }

    /// Set of sequence orderings the node supports, filtered by our outbound dial-info filter.
    pub fn supported_sequence_orderings(
        &self,
        routing_domain: RoutingDomain,
        outbound_dif: &DialInfoFilter,
    ) -> SequenceOrderingSet {
        let Some(pi) = self.get_peer_info(routing_domain) else {
            return SequenceOrderingSet::new();
        };
        pi.node_info().supported_sequence_orderings(outbound_dif)
    }

    // /// Set of low-level transports the node can be reached through (directly or
    // /// via any of its relays), filtered by our outbound dial-info filter.
    // pub fn supported_low_level_transports(
    //     &self,
    //     routing_domain: RoutingDomain,
    //     outbound_dif: &DialInfoFilter,
    // ) -> BTreeSet<LowLevelTransportType> {
    //     let Some(pi) = self.get_peer_info(routing_domain) else {
    //         return BTreeSet::new();
    //     };
    //     pi.node_info().low_level_transport_set(outbound_dif)
    // }

    // /// Best high-level TransportType for a given low-level transport on this node,
    // /// considering own and relay dial infos. None if outbound_dif filters everything.
    // pub fn preferred_transport_for(
    //     &self,
    //     routing_domain: RoutingDomain,
    //     low_level: LowLevelTransportType,
    //     outbound_dif: &DialInfoFilter,
    // ) -> Option<TransportType> {
    //     let pi = self.get_peer_info(routing_domain)?;
    //     pi.node_info()
    //         .preferred_transport_for(low_level, outbound_dif)
    // }

    pub fn has_node_info(&self, routing_domain_set: RoutingDomainSet) -> bool {
        routing_domain_set
            .iter()
            .any(|routing_domain| self.per_routing_domain.contains_key(&routing_domain))
    }

    pub fn best_node_id(&self) -> Option<NodeId> {
        self.node_ids.first().cloned()
    }

    pub fn get_peer_info(&self, routing_domain: RoutingDomain) -> Option<Arc<PeerInfo>> {
        self.per_routing_domain
            .get(&routing_domain)
            .map(|x| x.peer_info.clone())
    }

    pub fn node_status(&self, routing_domain: RoutingDomain) -> Option<NodeStatus> {
        self.per_routing_domain
            .get(&routing_domain)
            .and_then(|x| x.node_status.clone())
    }

    pub fn has_all_capabilities(
        &self,
        routing_domain: RoutingDomain,
        capabilities: &[VeilidCapability],
    ) -> bool {
        let Some(pi) = self.get_peer_info(routing_domain) else {
            return false;
        };
        pi.node_info().has_all_capabilities(capabilities)
    }

    pub fn cmp_fastest(
        a: &Self,
        b: &Self,
        metric: impl Fn(&LatencyStats) -> TimestampDuration,
    ) -> std::cmp::Ordering {
        // Lower latency to the front
        if let Some(a_latency) = &a.peer_stats.latency {
            if let Some(b_latency) = &b.peer_stats.latency {
                metric(a_latency).cmp(&metric(b_latency))
            } else {
                std::cmp::Ordering::Less
            }
        } else if b.peer_stats.latency.is_some() {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    }

    // Less is more reliable then faster
    pub fn cmp_fastest_reliable(
        a: &Self,
        b: &Self,
        metric: impl Fn(&LatencyStats) -> TimestampDuration,
    ) -> std::cmp::Ordering {
        // Reverse compare so most reliable is at front
        let ret = b.state.cmp(&a.state);
        if ret != std::cmp::Ordering::Equal {
            return ret;
        }

        // Lower latency to the front
        Self::cmp_fastest(a, b, metric)
    }

    // Less is more reliable then older
    pub fn cmp_oldest_reliable(a: &Self, b: &Self) -> std::cmp::Ordering {
        // Reverse compare so most reliable is at front
        let ret = b.state.cmp(&a.state);
        if ret != std::cmp::Ordering::Equal {
            return ret;
        }

        // Lower timestamp to the front, recent or no timestamp is at the end
        // First check steady-ping reliability timestamp
        if let Some(a_ts) = &a.rpc_stats.first_steady_answer_ts {
            if let Some(b_ts) = &b.rpc_stats.first_steady_answer_ts {
                a_ts.cmp(b_ts)
            } else {
                std::cmp::Ordering::Less
            }
        } else if b.rpc_stats.first_steady_answer_ts.is_some() {
            std::cmp::Ordering::Greater
        } else {
            // Then check 'since added to routing table' timestamp
            a.time_added.cmp(&b.time_added)
        }
    }

    pub fn has_seen_our_node_info_ts(
        &self,
        routing_domain: RoutingDomain,
        our_node_info_ts: Timestamp,
    ) -> bool {
        let Some(rds) = self.per_routing_domain.get(&routing_domain) else {
            return false;
        };
        our_node_info_ts == rds.last_seen_our_node_info_ts
    }

    /// Per-sequence-ordering ping decision. Returns false if the sequence ordering has no stats yet
    /// (no DialInfo for it, or never used) - callers should check `supported_sequence_orderings`
    /// against the peer info for cold-start pings.
    ///
    /// The cadence is driven by the node's overall (min-across-transports) state, not this
    /// transport's own state: when the node is unreliable, every transport is pinged at the fast
    /// unreliable interval so a stale 'reliable' transport gets re-probed and can degrade,
    /// instead of riding the slow reliable backoff.
    pub fn needs_proof_of_life_ping(
        &self,
        routing_domain: RoutingDomain,
        outbound_dif: &DialInfoFilter,
    ) -> SequenceOrderingSet {
        let mut out = SequenceOrderingSet::empty();
        for so in self.supported_sequence_orderings(routing_domain, outbound_dif) {
            let Some(stats) = self.per_sequence_ordering.get(&so) else {
                // Supported sequence ordering but no stats yet, cold start ping required
                out |= so;
                continue;
            };

            let needs_ping = match self.state {
                BucketEntryState::Punished | BucketEntryState::Dead => false,
                BucketEntryState::Missing | BucketEntryState::Initial => true,
                BucketEntryState::Unreliable => match stats.last_question_ts {
                    None => true,
                    Some(last_question_ts) => {
                        self.cur_ts.duration_since(last_question_ts) >= UNRELIABLE_PING_INTERVAL
                    }
                },
                BucketEntryState::Reliable => {
                    match (stats.last_question_ts, stats.first_steady_answer_ts) {
                        // Never asked: ping it.
                        (None, _) => true,
                        // Asked but not yet established with this sequence ordering: probe at the unreliable interval.
                        (Some(last_question_ts), None) => {
                            self.cur_ts.duration_since(last_question_ts) >= UNRELIABLE_PING_INTERVAL
                        }
                        // Established reliable with this sequence ordering: use the reliable backoff.
                        (Some(last_question_ts), Some(first_steady_answer_ts)) => {
                            let start_of_reliable_time = first_steady_answer_ts.later(
                                UNRELIABLE_ANSWER_SPAN.saturating_sub(UNRELIABLE_PING_INTERVAL),
                            );
                            let reliable_cur = self.cur_ts.duration_since(start_of_reliable_time);
                            let reliable_last =
                                last_question_ts.duration_since(start_of_reliable_time);
                            retry_falloff_log(
                                reliable_last.as_u64(),
                                reliable_cur.as_u64(),
                                RELIABLE_PING_INTERVAL_START.as_u64(),
                                RELIABLE_PING_INTERVAL_MAX.as_u64(),
                                RELIABLE_PING_INTERVAL_MULTIPLIER,
                            )
                        }
                    }
                }
            };
            if needs_ping {
                out |= so;
            }
        }

        out
    }
}
