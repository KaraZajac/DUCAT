use super::*;

/// How long to wait without receiving a ValueChanged before
/// triggering a fallback change inspection to detect missed updates.
const WATCH_FALLBACK_INSPECT_INTERVAL: TimestampDuration = TimestampDuration::new_secs(30);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::storage_manager) struct OutboundWatchState {
    /// Requested parameters
    params: OutboundWatchParameters,
    /// Nodes that have an active watch on our behalf
    nodes: Vec<PerNodeKey>,
    /// How many value change updates remain
    remaining_count: u32,
    /// The next earliest time we are willing to try to reconcile and improve the watch
    opt_next_reconcile_ts: Option<Timestamp>,
    /// The number of nodes we got at our last reconciliation
    opt_last_consensus_node_count: Option<usize>,
    /// Calculated field: minimum expiration time for all our nodes
    min_expiration: Timestamp,
    /// Calculated field: the set of value changed routes for this watch from all per node watches
    value_changed_routes: BTreeSet<PublicKey>,
    /// Timestamp when the watch was established or last renewed/reconciled.
    watch_established_ts: Timestamp,
    /// Timestamp of the last ValueChanged RPC received.
    /// Set ONLY when an actual ValueChanged arrives from a remote node.
    last_value_changed_ts: Option<Timestamp>,
    /// Timestamp of the last fallback change inspection.
    last_fallback_inspect_ts: Option<Timestamp>,
    /// Set by fallback change inspection when changes are found that should
    /// have been delivered via ValueChanged but weren't. Forces a reconcile
    /// to re-establish the watch with fresh notification routes.
    needs_forced_reconcile: bool,
    /// Last sequence number reported to the app per subkey for a transactional (non-committing) hint.
    /// Gates hints to be monotonically increasing so replays and duplicate per-node reports are dropped.
    #[serde(default)]
    last_reported_subkey_seqs: BTreeMap<ValueSubkey, ValueSeqNum>,
}

impl fmt::Display for OutboundWatchState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut value_changed_routes = self
            .value_changed_routes
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>();
        value_changed_routes.sort_unstable();

        write!(
            f,
            r#"params: {}
nodes: [{}]
remaining_count: {}
opt_next_reconcile_ts: {}
opt_consensus_node_count: {}
min_expiration: {}
value_changed_routes: [{}]"#,
            self.params,
            self.nodes
                .iter()
                .map(|x| x.node_id.to_string())
                .collect::<Vec<_>>()
                .join(","),
            self.remaining_count,
            if let Some(next_reconcile_ts) = &self.opt_next_reconcile_ts {
                next_reconcile_ts.to_string()
            } else {
                "None".to_owned()
            },
            if let Some(consensus_node_count) = &self.opt_last_consensus_node_count {
                consensus_node_count.to_string()
            } else {
                "None".to_owned()
            },
            self.min_expiration,
            value_changed_routes.join(","),
        )
    }
}

pub(in crate::storage_manager) struct OutboundWatchStateEditor<'a> {
    state: &'a mut OutboundWatchState,
}

impl OutboundWatchStateEditor<'_> {
    pub fn set_params(&mut self, params: OutboundWatchParameters) {
        self.state.params = params;
    }
    pub fn add_nodes<I: IntoIterator<Item = PerNodeKey>>(&mut self, nodes: I) {
        for node in nodes {
            if !self.state.nodes.contains(&node) {
                self.state.nodes.push(node);
            }
        }
    }
    pub fn retain_nodes<F: FnMut(&PerNodeKey) -> bool>(&mut self, f: F) {
        self.state.nodes.retain(f);
    }
    pub fn set_remaining_count(&mut self, remaining_count: u32) {
        self.state.remaining_count = remaining_count;
    }
    pub fn set_next_reconcile_ts(&mut self, next_reconcile_ts: Timestamp) {
        self.state.opt_next_reconcile_ts = Some(next_reconcile_ts);
    }
    pub fn update_last_consensus_node_count(&mut self) {
        self.state.opt_last_consensus_node_count = Some(self.state.nodes().len());
    }
    pub fn touch_value_changed(&mut self, ts: Timestamp) {
        self.state.last_value_changed_ts = Some(ts);
    }
    pub fn touch_fallback_inspect(&mut self, ts: Timestamp) {
        self.state.last_fallback_inspect_ts = Some(ts);
    }
    pub fn set_forced_reconcile(&mut self) {
        self.state.needs_forced_reconcile = true;
    }
    pub fn clear_forced_reconcile(&mut self) {
        self.state.needs_forced_reconcile = false;
    }
    pub fn set_last_reported_subkey_seq(&mut self, subkey: ValueSubkey, seq: ValueSeqNum) {
        self.state.last_reported_subkey_seqs.insert(subkey, seq);
    }
}

impl OutboundWatchState {
    pub fn new(params: OutboundWatchParameters) -> Self {
        let remaining_count = params.count;
        let min_expiration = params.expiration;

        Self {
            params,
            nodes: vec![],
            remaining_count,
            opt_next_reconcile_ts: None,
            opt_last_consensus_node_count: None,
            min_expiration,
            value_changed_routes: BTreeSet::new(),
            watch_established_ts: Timestamp::now_non_decreasing(),
            last_value_changed_ts: None,
            last_fallback_inspect_ts: None,
            needs_forced_reconcile: false,
            last_reported_subkey_seqs: BTreeMap::new(),
        }
    }

    pub fn params(&self) -> &OutboundWatchParameters {
        &self.params
    }
    pub fn nodes(&self) -> &Vec<PerNodeKey> {
        &self.nodes
    }
    pub fn remaining_count(&self) -> u32 {
        self.remaining_count
    }
    pub fn next_reconcile_ts(&self) -> Option<Timestamp> {
        self.opt_next_reconcile_ts
    }
    pub fn min_expiration(&self) -> Timestamp {
        self.min_expiration
    }
    pub fn value_changed_routes(&self) -> &BTreeSet<PublicKey> {
        &self.value_changed_routes
    }
    pub fn last_reported_subkey_seq(&self, subkey: ValueSubkey) -> Option<ValueSeqNum> {
        self.last_reported_subkey_seqs.get(&subkey).copied()
    }

    /// Get the parameters we use if we're updating this state's per node watches
    pub fn get_per_node_params(
        &self,
        desired: &OutboundWatchParameters,
    ) -> OutboundWatchParameters {
        // Change the params to update count
        if self.params() != desired {
            // If parameters are changing, just use the desired parameters
            desired.clone()
        } else {
            // If this is a renewal of the same parameters,
            // use the current remaining update count for the rpc
            let mut renew_params = desired.clone();
            renew_params.count = self.remaining_count();
            renew_params
        }
    }

    pub fn edit<R, F: FnOnce(&mut OutboundWatchStateEditor) -> R>(
        &mut self,
        per_node_state: &HashMap<PerNodeKey, OutboundWatchPerNodeState>,
        closure: F,
    ) -> R {
        let mut editor = OutboundWatchStateEditor { state: self };
        let res = closure(&mut editor);

        // Update calculated fields
        self.min_expiration = self
            .nodes
            .iter()
            .map(|x| per_node_state.get(x).unwrap_or_log().expiration)
            .reduce(|a, b| a.min(b))
            .unwrap_or(self.params.expiration);

        self.value_changed_routes = self
            .nodes
            .iter()
            .filter_map(|x| {
                per_node_state
                    .get(x)
                    .cloned()
                    .unwrap_or_log()
                    .opt_value_changed_route
            })
            .collect();

        res
    }

    pub fn watch_node_refs(
        &self,
        per_node_state: &HashMap<PerNodeKey, OutboundWatchPerNodeState>,
    ) -> Vec<NodeRef> {
        self.nodes
            .iter()
            .map(|x| {
                per_node_state
                    .get(x)
                    .unwrap_or_log()
                    .watch_node_ref
                    .clone()
                    .unwrap_or_log()
            })
            .collect()
    }

    /// Check if a fallback change inspection is needed because no
    /// ValueChanged has been received within WATCH_FALLBACK_INSPECT_INTERVAL.
    pub fn needs_fallback_inspect(&self, cur_ts: Timestamp) -> bool {
        let reference_ts = self
            .last_fallback_inspect_ts
            .map(|t| t.max(self.watch_established_ts))
            .unwrap_or(self.watch_established_ts);

        if let Some(vc_ts) = self.last_value_changed_ts {
            if vc_ts >= reference_ts {
                return false;
            }
        }

        cur_ts >= reference_ts.later(WATCH_FALLBACK_INSPECT_INTERVAL)
    }

    /// Whether a forced reconcile is needed (fallback inspection found missed changes).
    pub fn needs_forced_reconcile(&self) -> bool {
        self.needs_forced_reconcile
    }
}
