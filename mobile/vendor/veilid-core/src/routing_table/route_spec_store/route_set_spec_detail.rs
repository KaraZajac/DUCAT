use super::*;

/// The specification of a single route in a route set, per crypto kind
#[derive(Clone, Debug, Serialize, Deserialize)]
#[must_use]
pub struct RouteSpecDetail {
    /// Secret key
    pub secret_key: SecretKey,
    /// Route hop node ids
    pub hops: Vec<NodeId>,
}

/// Statistics summaries (rolled stats) that we want to serialize
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[must_use]
pub struct RouteSetSpecDetailStats {
    /// Timestamp of when the route set was created
    pub created_ts: Timestamp,
    /// Transfers up and down
    pub transfer: TransferStatsDownUp,
    /// Latency stats
    pub latency: LatencyStats,
    /// Answer stats per SequenceOrdering (serde-default: old aggregate field is dropped)
    #[serde(default)]
    pub answer_by_ordering: Vec<(SequenceOrdering, AnswerStats)>,
}

#[derive(Debug, Serialize, Deserialize)]
#[must_use]
pub struct RouteSetSpecDetail {
    /// Routes in the set per crypto kind
    route_set: BTreeMap<PublicKey, RouteSpecDetail>,
    /// Directions this route is guaranteed to work in
    directions: DirectionSet,
    /// Stability preference (prefer reliable nodes over faster)
    stability: Stability,
    /// Sequencing capability (connection oriented protocols vs datagram)
    orderings: SequenceOrderingSet,
    /// Automatically allocated route vs manually allocated route
    automatic: bool,
    /// Stats
    stats: RwLock<RouteSetSpecDetailStats>,
}

impl Clone for RouteSetSpecDetail {
    fn clone(&self) -> Self {
        Self {
            route_set: self.route_set.clone(),
            directions: self.directions,
            stability: self.stability,
            orderings: self.orderings,
            automatic: self.automatic,
            stats: RwLock::new(self.stats.read().clone()),
        }
    }
}

impl fmt::Display for RouteSetSpecDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (latency_stats, transfer_stats) = {
            let stats = self.stats.read();
            (stats.latency.to_string(), stats.transfer.to_string())
        };
        write!(
            f,
            "count={}, stability={:?} dirs={:?} auto={:?} latency={} transfer={} kinds=[{}] hops=[{}]",
            self.get_hop_count(),
            self.get_stability(),
            self.get_directions(),
            self.is_automatic(),
            f.to_string(&latency_stats),
            f.to_string(&transfer_stats),
            self.route_set.keys().map(|x| x.to_string()).collect::<Vec<_>>().join(","),
            self.route_set.first_key_value()
                .unwrap_or_log()
                .1
                .hops
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(","),
        )
    }
}

impl RouteSetSpecDetail {
    pub fn new(
        route_set: BTreeMap<PublicKey, RouteSpecDetail>,
        directions: DirectionSet,
        stability: Stability,
        orderings: SequenceOrderingSet,
        automatic: bool,
    ) -> VeilidAPIResult<Self> {
        if route_set.is_empty() {
            apibail_missing_argument!("route set is empty", "route_set");
        }
        if directions.is_empty() {
            apibail_missing_argument!("directions is empty", "directions");
        }
        if orderings.is_empty() {
            apibail_missing_argument!("orderings is empty", "orderings");
        }
        Ok(Self {
            route_set,
            directions,
            stability,
            orderings,
            stats: RwLock::new(Default::default()),
            automatic,
        })
    }

    pub fn lookup_node_refs(&self, routing_table: &RoutingTable) -> Option<Vec<NodeRef>> {
        'outer: for rsd in self.route_set.values() {
            let mut hop_node_refs = Vec::with_capacity(rsd.hops.len());
            for h in &rsd.hops {
                let Ok(Some(nr)) = routing_table.lookup_node_id(h.clone()) else {
                    continue 'outer;
                };
                hop_node_refs.push(nr);
            }
            return Some(hop_node_refs);
        }
        None
    }

    pub fn get_route_set_keys(&self) -> PublicKeyGroup {
        let mut tks = PublicKeyGroup::new();
        for k in self.route_set.keys() {
            tks.add(k.clone());
        }
        tks
    }
    pub fn get_route_set_secrets(&self) -> SecretKeyGroup {
        let mut tks = SecretKeyGroup::new();
        for v in self.route_set.values() {
            tks.add(v.secret_key.clone());
        }
        tks
    }

    pub fn iter_route_set(
        &self,
    ) -> alloc::collections::btree_map::Iter<'_, PublicKey, RouteSpecDetail> {
        self.route_set.iter()
    }
    pub fn get_hop_count(&self) -> usize {
        self.route_set
            .first_key_value()
            .unwrap_or_log()
            .1
            .hops
            .len()
    }

    pub fn get_stability(&self) -> Stability {
        self.stability
    }
    pub fn get_directions(&self) -> DirectionSet {
        self.directions
    }
    pub fn get_orderings(&self) -> SequenceOrderingSet {
        self.orderings
    }

    #[must_use]
    pub fn is_automatic(&self) -> bool {
        self.automatic
    }

    pub fn get_stats(&self) -> RouteSetSpecDetailStats {
        self.stats.read().clone()
    }

    pub fn update_latency(&self, route_stats: &RouteStats) {
        let mut stats = self.stats.write();
        stats.latency = route_stats.latency.clone();
    }

    pub fn update_transfers(&self, route_stats: &RouteStats) {
        let mut stats = self.stats.write();
        stats.transfer = route_stats.transfer.clone();
    }

    pub fn update_answers(&self, route_stats: &RouteStats) {
        let mut stats = self.stats.write();
        stats.answer_by_ordering = route_stats.answer_by_ordering();
    }
}
