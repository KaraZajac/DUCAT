use super::*;

#[derive(Clone, Debug)]
pub struct NodeTransactionParams {
    /// Crypto kind of the record key
    pub kind: CryptoKind,
    /// Server-side transaction id
    pub xid: u64,
    /// Ref to the node running this transaction
    pub node_ref: NodeRef,
    /// When the server says this node transaction is dead
    pub expiration: Timestamp,
    /// RTT measured during Begin, used to seed the per-node RTT estimate
    pub opt_initial_rtt: Option<TimestampDuration>,
    /// Per-subkey seqs this node reported in its Begin answer
    pub begin_seqs: Vec<ValueSeqNum>,
}

/// Transaction per node
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct NodeTransaction {
    /// Registry for logging
    #[serde(skip)]
    opt_registry: Option<VeilidComponentRegistry>,
    /// The server-side unique key for this node transaction
    rnxid: RemoteNodeTransactionId,
    /// Ref to keep node in routing table
    #[serde(skip)]
    node_ref: Option<NodeRef>,
    /// When the server says this node transaction is dead
    /// None means the server has already ended the transaction
    opt_expiration: Option<Timestamp>,
    /// The timestamp of when the node transaction was created
    created_ts: Timestamp,
    /// The timestamp of the last stage transition
    stage_ts: Timestamp,
    /// The operational stage of this node transaction
    stage: OutboundTransactionStage,
    /// The values of remote subkeys seen by this node transaction (newer subkeys returned, and gets)
    current_values: BTreeMap<ValueSubkey, Option<Arc<SignedValueData>>>,
    /// The values of remote subkeys changed by this node transaction (sets)
    updated_values: BTreeMap<ValueSubkey, Arc<SignedValueData>>,
    /// Most recent RTT measured for this node, refreshed on each successful RPC
    #[serde(default)]
    opt_last_rtt: Option<TimestampDuration>,
    /// Per-subkey seqs this node reported in its Begin answer
    #[serde(default)]
    begin_seqs: Vec<ValueSeqNum>,
}

impl VeilidComponentRegistryAccessor for NodeTransaction {
    fn registry(&self) -> VeilidComponentRegistry {
        self.opt_registry.clone().unwrap_or_log()
    }
}

impl NodeTransaction {
    pub(super) fn new(
        registry: VeilidComponentRegistry,
        rnxid: RemoteNodeTransactionId,
        node_ref: NodeRef,
        expiration: Timestamp,
        opt_initial_rtt: Option<TimestampDuration>,
        begin_seqs: Vec<ValueSeqNum>,
    ) -> Self {
        let cur_ts = Timestamp::now();
        Self {
            opt_registry: Some(registry),
            created_ts: cur_ts,
            stage_ts: cur_ts,
            stage: OutboundTransactionStage::Begin,
            rnxid,
            node_ref: Some(node_ref),
            opt_expiration: Some(expiration),
            current_values: Default::default(),
            updated_values: Default::default(),
            opt_last_rtt: opt_initial_rtt,
            begin_seqs,
        }
    }

    /// The seq this node reported for a subkey in its Begin answer
    pub fn begin_seq(&self, subkey: ValueSubkey) -> ValueSeqNum {
        self.begin_seqs
            .get(subkey as usize)
            .copied()
            .unwrap_or_default()
    }

    pub(super) fn prepare(&mut self, routing_table: &RoutingTable, cur_ts: Timestamp) -> bool {
        if let Some(expiration) = self.opt_expiration {
            if expiration < cur_ts {
                return false;
            }
        } else {
            return false;
        }
        let Some(node_ref) = routing_table.lookup_node_id(self.node_id()).ok().flatten() else {
            return false;
        };
        self.opt_registry = Some(routing_table.registry());
        self.node_ref = Some(node_ref);
        true
    }

    #[expect(dead_code)]
    pub fn created_ts(&self) -> Timestamp {
        self.created_ts
    }

    pub fn opt_expiration(&self) -> Option<Timestamp> {
        self.opt_expiration
    }

    pub fn stage_ts(&self) -> Timestamp {
        self.stage_ts
    }

    pub fn stage(&self) -> OutboundTransactionStage {
        self.stage
    }

    pub(super) fn set_stage(
        &mut self,
        stage: OutboundTransactionStage,
        opt_expiration: Option<Timestamp>,
    ) {
        self.stage_ts = Timestamp::now();
        self.stage = stage;
        self.opt_expiration = opt_expiration;
    }

    pub fn node_ref(&self) -> NodeRef {
        // Safe as long as prepare has been called
        self.node_ref.clone().unwrap_or_log()
    }

    pub fn rnxid(&self) -> &RemoteNodeTransactionId {
        &self.rnxid
    }

    pub fn node_id(&self) -> NodeId {
        self.rnxid.node_id()
    }

    pub(super) fn update_expiration(&mut self, opt_expiration: Option<Timestamp>) {
        self.opt_expiration = opt_expiration;
    }

    pub(super) fn record_current_subkey_value(
        &mut self,
        subkey: ValueSubkey,
        value: Option<Arc<SignedValueData>>,
    ) {
        // Keep a record of both existing and missing subkeys
        self.current_values.insert(subkey, value);
    }

    pub(super) fn record_updated_subkey_value(
        &mut self,
        subkey: ValueSubkey,
        value: Option<Arc<SignedValueData>>,
    ) {
        // Keep only changed subkeys
        if let Some(v) = value {
            self.updated_values.insert(subkey, v);
        } else {
            self.updated_values.remove(&subkey);
        }
    }

    /// Record the most recent RTT measured for this node after a successful RPC.
    pub(super) fn record_rtt(&mut self, rtt: TimestampDuration) {
        self.opt_last_rtt = Some(rtt);
    }

    pub fn commit_will_change_remote(&self) -> bool {
        for (subkey, updated_value) in self
            .updated_values
            .iter()
            .map(|(sk, svd)| (sk, svd.clone()))
        {
            let updated_seq = updated_value.value_data().seq();

            let opt_current_value = self.current_values.get(subkey).cloned().unwrap_or_default();
            let opt_current_seq = opt_current_value
                .map(|x| x.value_data().seq())
                .unwrap_or_default();

            if updated_seq > opt_current_seq {
                return true;
            }
        }
        #[cfg(feature = "verbose-tracing")]
        {
            let subkey_summary = self
                .updated_values
                .iter()
                .map(|(subkey, updated_value)| {
                    let updated_seq = updated_value.value_data().seq();
                    let opt_current_seq = self
                        .current_values
                        .get(subkey)
                        .and_then(|v| v.as_ref().map(|x| x.value_data().seq()));
                    format!(
                        "#{}:updated={},current={}",
                        subkey,
                        updated_seq,
                        opt_current_seq
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "None".to_string())
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            veilid_log!(self debug target: "dht", "commit_will_change_remote: node={} → false subkeys=[{}]",
                self.rnxid, subkey_summary);
        }
        false
    }

    #[expect(dead_code)]
    pub fn get_inconsistent_subkeys(
        &self,
        desired_subkeys: &BTreeMap<ValueSubkey, Arc<SignedValueData>>,
    ) -> BTreeSet<ValueSubkey> {
        let mut out = BTreeSet::new();

        for (subkey, desired_value) in desired_subkeys.iter().map(|(sk, svd)| (sk, svd.clone())) {
            let opt_current_value = self.current_values.get(subkey).cloned().flatten();
            let opt_updated_value = self.updated_values.get(subkey).cloned();

            let desired_value_data = desired_value.value_data();
            let opt_current_value_data = opt_current_value.as_ref().map(|x| x.value_data());
            let opt_updated_value_data = opt_updated_value.as_ref().map(|x| x.value_data());

            // If we tried to set a value and it succeeded, then:
            // 1. If the sequence number online was older or missing, then it would have returned
            //    None, which would result in the updated value being the desired value
            // 2. If the sequence number online was newer or the same, then it would have returned
            //    a current value different from the desired value with equal or newer sequence number, and cleared the updated value
            // If we tried to set a value and it did not succeed, then the negation of the above tests would be true
            let mut consistent = false;
            if opt_updated_value_data == Some(desired_value_data) {
                // Case 1 above, we set a newer value than what was online, so updated == desired
                consistent = true;
            } else if let Some(current_value_data) = opt_current_value_data {
                if current_value_data != desired_value_data
                    && current_value_data.seq() >= desired_value_data.seq()
                {
                    // Case 2 above, the value online is not the desired value and its sequence number is greater or equal to the desired value
                    consistent = true;
                }
            }

            // If any subkey is not consistent, include in the inconsistent set
            if !consistent {
                out.insert(*subkey);
            }
        }

        out
    }
}

impl fmt::Display for NodeTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: @{} stage: {:?}@{} exp: {} rtt: {}",
            self.rnxid,
            self.created_ts,
            self.stage,
            self.stage_ts,
            f.to_string_opt(self.opt_expiration),
            f.to_string_opt(self.opt_last_rtt),
        )
    }
}
