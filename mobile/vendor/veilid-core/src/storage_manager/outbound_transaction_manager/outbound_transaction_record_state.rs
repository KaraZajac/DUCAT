use super::*;

/// Parameters for creating an OutboundTransactionRecord
#[derive(Clone, Debug, Serialize, Deserialize, GetSize)]
pub(in crate::storage_manager) struct OutboundTransactionRecordParams {
    /// The record key being transacted over
    pub record_key: RecordKey,
    /// The signer key being used to authenticate the transaction
    pub signing_keypair: KeyPair,
    /// Consensus count required for this record to transact (set_value_count)
    pub required_strict_consensus_count: usize,
    /// Minimum read consensus count for this record (get_value_count)
    pub required_get_consensus_count: usize,
    /// Safety selection to use for this record
    pub safety_selection: SafetySelection,
    /// Local snapshot of the record
    pub local_snapshot: Arc<RecordSnapshot>,
}

/// Stage consensus for record state across all node transactions
#[derive(Clone, Debug)]
pub(in crate::storage_manager) struct OutboundTransactionRecordStageConsensus {
    /// The best consensus stage we could come up with for this record
    pub stage: OutboundTransactionStage,
    /// The list of node transactions that should be rolled back at this point
    pub node_transactions_to_rollback: LocalNodeTransactionIdSet,
    /// The list of node transactions that should be dropped at this point
    pub node_transactions_to_drop: LocalNodeTransactionIdSet,
}

/// Which node transaction ids at what stage
type StageConsensusMap = HashMap<OutboundTransactionStage, BTreeSet<LocalNodeTransactionId>>;

/// Filter for get_transact_command_nodes
type GetTransactCommandNodesFilter<'a> = Box<dyn Fn(&'a NodeTransaction) -> bool + 'a>;

/// State per record
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::storage_manager) struct OutboundTransactionRecordState {
    /// Registry for logging
    #[serde(skip)]
    opt_registry: Option<VeilidComponentRegistry>,
    /// Parameters for this record state
    record_params: OutboundTransactionRecordParams,
    /// Transactions per node by locally allocated node transaction id
    node_transactions_by_id: BTreeMap<LocalNodeTransactionId, NodeTransaction>,
    /// Snapshot of maximum sequence numbers per subkey on the network at the time of transaction begin
    begin_network_seqs: Vec<ValueSeqNum>,
    /// The timestamp of when the transaction record was created
    created_ts: Timestamp,
    /// Descriptor for the record. Record may not exist locally until after the transaction, so this descriptor may have come from the network.
    descriptor: Option<Arc<SignedValueDescriptor>>,
    /// Schema for the record
    schema: Option<DHTSchema>,
    /// The last desired value for subkeys we have tried to set
    desired_subkeys: BTreeMap<ValueSubkey, Arc<SignedValueData>>,
    /// Consensus result of remote snapshot subkeys (newer subkeys returned, and gets)
    current_consensus: OutboundTransactionConsensus,
    /// Consensus result of remote subkey state upon transaction commit (sets)
    updated_consensus: OutboundTransactionConsensus,
}

impl fmt::Display for OutboundTransactionRecordState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} created@{} signer={} safety_selection={:?}\nnode_transactions:\n{}\nbegin_network_seqs:{}\nlocal_seqs:{}\n{}{}{}",
            self.record_params.record_key,
            f.to_string(self.created_ts),
            self.record_params.signing_keypair.key(),
            self.record_params.safety_selection,
            self.node_transactions_by_id
                .iter()
                .map(|(k, v)| format!("  {}: {}", f.to_string(k), f.to_string(v)))
                .collect::<Vec<_>>()
                .join("\n"),
            self.begin_network_seqs.to_table_string(),
            self.record_params.local_snapshot.seqs().to_table_string(),
            if !self.desired_subkeys.is_empty() {
                let desired_subkeys = self
                    .desired_subkeys
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "#{}={}",
                            k,
                            v.value_data().seq()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!("desired_subkeys: {}\n", desired_subkeys)
            } else {
                "".to_string()
            },
            if !self.current_consensus.is_empty() {
                format!("current_subkey_states: {}\n", &self.current_consensus)
            } else {
                "".to_string()
            },
            if !self.updated_consensus.is_empty() {
                format!("updated_subkey_states: {}\n", &self.updated_consensus)
            } else {
                "".to_string()
            }
        )
    }
}

impl VeilidComponentRegistryAccessor for OutboundTransactionRecordState {
    fn registry(&self) -> VeilidComponentRegistry {
        self.opt_registry.clone().unwrap_or_log()
    }
}

impl OutboundTransactionRecordState {
    pub(super) fn new(
        registry: VeilidComponentRegistry,
        record_params: OutboundTransactionRecordParams,
    ) -> Self {
        Self {
            opt_registry: Some(registry),
            record_params,
            node_transactions_by_id: BTreeMap::new(),
            begin_network_seqs: vec![],
            created_ts: Timestamp::now(),
            descriptor: None,
            schema: None,
            desired_subkeys: Default::default(),
            current_consensus: OutboundTransactionConsensus::new(),
            updated_consensus: OutboundTransactionConsensus::new(),
        }
    }

    /// Calculate the consensus of the node transactions to determine what this record's effective stage is
    /// and actions to perform to reconcile the transaction for this record
    pub fn stage_consensus(&self) -> Option<OutboundTransactionRecordStageConsensus> {
        // If we have no node transactions, this is at an Init stage
        if self.node_transactions_by_id.is_empty() {
            return None;
        }

        // Count up what stages we are at with each node transaction
        let stage_consensus_map = self.get_stage_consensus_map();

        // Find a singular consensus
        let stage = {
            let mut opt_best_stage = None;
            for (st, stn) in stage_consensus_map.iter().map(|(st, stn)| (*st, stn)) {
                if stn.len() >= self.record_params.required_strict_consensus_count {
                    if opt_best_stage.is_none() {
                        opt_best_stage = Some(st);
                    } else {
                        // Multiple stages met strict consensus (should not happen) — log and mark Failed.
                        #[cfg(feature = "verbose-tracing")]
                        veilid_log!(self debug target: "dht", "stage_consensus: ambiguous — multiple stages met threshold {} for record {}: {:?}",
                            self.record_params.required_strict_consensus_count, self.record_params.record_key.opaque(), stage_consensus_map);
                        opt_best_stage = Some(OutboundTransactionStage::Failed);
                        break;
                    }
                }
            }
            // If no stage has met the strict consensus, this is also failed
            #[cfg(feature = "verbose-tracing")]
            if opt_best_stage.is_none() {
                veilid_log!(self debug target: "dht", "stage_consensus: no stage reached required threshold={} for record {}: stage_map={:?}",
                    self.record_params.required_strict_consensus_count, self.record_params.record_key.opaque(), stage_consensus_map);
            }
            opt_best_stage.unwrap_or(OutboundTransactionStage::Failed)
        };

        // If the consensus stage has some requred state at this point, validate it
        let stage = match stage {
            OutboundTransactionStage::Failed
            | OutboundTransactionStage::Rollback
            | OutboundTransactionStage::Commit => {
                // Nothing to validate here, stage is the same
                stage
            }
            OutboundTransactionStage::Begin | OutboundTransactionStage::End => {
                if self.descriptor.is_none() {
                    // Descriptor was never found, stage is Failed
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(self debug target: "dht", "stage_consensus: descriptor missing for record {} at stage {:?} → Failed",
                        self.record_params.record_key.opaque(), stage);
                    OutboundTransactionStage::Failed
                } else {
                    // We have what we need, stage is the same
                    stage
                }
            }
        };

        // Now that we know what our stage stage is, determine what should to be done
        // to move on to the next operation cleanly
        let stage_consensus = match stage {
            OutboundTransactionStage::Failed
            | OutboundTransactionStage::Rollback
            | OutboundTransactionStage::Commit => {
                // Failed stage means we can only Rollback node transactions so we get a Rollback consensus
                // Rollback stage means we have a consensus at Rollback but there may be other nodes that should get rolled back
                // Commit stage means we have a consensus at Commit but there may be other nodes that should get rolled back
                // At these stages there is no point to dropping node transactions because they will -all- get dropped at termination

                let node_transactions_to_rollback =
                    Self::get_all_rollbacks_internal(&stage_consensus_map);
                OutboundTransactionRecordStageConsensus {
                    stage,
                    node_transactions_to_rollback,
                    node_transactions_to_drop: Default::default(),
                }
            }
            OutboundTransactionStage::Begin => {
                // Begin stage means we have a consensus at Begin, but there may be other nodes that should get rolled back and dropped, or just dropped

                // Find all rollback-capable (not finished) node transaction ids and already-rolled-back ids so we can drop them
                let mut force_fail = false;
                let mut node_transactions_to_rollback = LocalNodeTransactionIdSet::new();
                let mut node_transactions_to_drop = LocalNodeTransactionIdSet::new();
                for (st, stn) in stage_consensus_map.iter() {
                    match st {
                        OutboundTransactionStage::Failed => {
                            // Roll back and drop any failed nodes
                            node_transactions_to_rollback.extend(stn);
                            node_transactions_to_drop.extend(stn);
                        }
                        OutboundTransactionStage::Rollback => {
                            // If nodes are already rolled back then just drop them
                            node_transactions_to_drop.extend(stn);
                        }
                        OutboundTransactionStage::Begin => {
                            // Keep the consensus nodes
                        }
                        OutboundTransactionStage::End | OutboundTransactionStage::Commit => {
                            // If some nodes ended or committed but we are still somehow at a consensus of Begin,
                            // roll back everything and move to a failed state
                            force_fail = true;
                        }
                    }
                }

                if force_fail {
                    // Don't bother dropping any nodes, only roll back everything
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(self debug target: "dht", "stage_consensus: force_fail at Begin for record {} — End/Commit nodes present, map={:?}",
                        self.record_params.record_key.opaque(), stage_consensus_map);
                    let node_transactions_to_rollback =
                        Self::get_all_rollbacks_internal(&stage_consensus_map);
                    OutboundTransactionRecordStageConsensus {
                        stage: OutboundTransactionStage::Failed,
                        node_transactions_to_rollback,
                        node_transactions_to_drop: Default::default(),
                    }
                } else {
                    // Return the stage consensus
                    OutboundTransactionRecordStageConsensus {
                        stage,
                        node_transactions_to_rollback,
                        node_transactions_to_drop,
                    }
                }
            }
            OutboundTransactionStage::End => {
                // End stage means we have a consensus at End, but there may be other nodes that should get rolled back and dropped, or just dropped

                // Find all rollback-capable (not finished) node transaction ids and already-rolled-back ids so we can drop them
                let mut force_fail = false;
                let mut node_transactions_to_rollback = LocalNodeTransactionIdSet::new();
                let mut node_transactions_to_drop = LocalNodeTransactionIdSet::new();
                for (st, stn) in stage_consensus_map.iter() {
                    match st {
                        OutboundTransactionStage::Failed | OutboundTransactionStage::Begin => {
                            // Roll back and drop any failed nodes or nodes still at the begin stage
                            node_transactions_to_rollback.extend(stn);
                            node_transactions_to_drop.extend(stn);
                        }
                        OutboundTransactionStage::Rollback => {
                            // If nodes are already rolled back then just drop them
                            node_transactions_to_drop.extend(stn);
                        }
                        OutboundTransactionStage::End => {
                            // Keep the consensus nodes
                        }
                        OutboundTransactionStage::Commit => {
                            // If some nodes committed but we are still somehow at a consensus of End,
                            // roll back everything and move to a failed state
                            force_fail = true;
                        }
                    }
                }

                if force_fail {
                    // Don't bother dropping any nodes, only roll back everything
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(self debug target: "dht", "stage_consensus: force_fail at End for record {} — Commit nodes present while End consensus, map={:?}",
                        self.record_params.record_key.opaque(), stage_consensus_map);
                    let node_transactions_to_rollback =
                        Self::get_all_rollbacks_internal(&stage_consensus_map);
                    OutboundTransactionRecordStageConsensus {
                        stage: OutboundTransactionStage::Failed,
                        node_transactions_to_rollback,
                        node_transactions_to_drop: Default::default(),
                    }
                } else {
                    // Return the stage consensus
                    OutboundTransactionRecordStageConsensus {
                        stage,
                        node_transactions_to_rollback,
                        node_transactions_to_drop,
                    }
                }
            }
        };

        #[cfg(feature = "verbose-tracing")]
        veilid_log!(self debug target: "dht", "stage_consensus: record={} result=stage:{:?} rollback_count={} drop_count={}",
            self.record_params.record_key.opaque(), stage_consensus.stage,
            stage_consensus.node_transactions_to_rollback.len(),
            stage_consensus.node_transactions_to_drop.len());

        Some(stage_consensus)
    }

    /// Force-rollback everything that isn't done and return a stage consensus describing the actions
    pub(super) fn get_all_rollbacks(&self) -> LocalNodeTransactionIdSet {
        let stage_consensus_map = self.get_stage_consensus_map();
        Self::get_all_rollbacks_internal(&stage_consensus_map)
    }

    pub(super) fn get_all_rollbacks_internal(
        stage_consensus_map: &StageConsensusMap,
    ) -> LocalNodeTransactionIdSet {
        // Don't bother dropping any nodes, only roll back everything that can be rolled back
        let mut node_transactions_to_rollback = LocalNodeTransactionIdSet::new();
        for (st, stn) in stage_consensus_map.iter() {
            if !matches!(
                st,
                OutboundTransactionStage::Rollback | OutboundTransactionStage::Commit
            ) {
                node_transactions_to_rollback.extend(stn);
            }
        }

        // Return the stage consensus
        node_transactions_to_rollback
    }

    /// Count up what stages we are at with each node transaction
    pub(super) fn get_stage_consensus_map(&self) -> StageConsensusMap {
        let mut stage_consensus_map = StageConsensusMap::new();
        for (lnxid, nt) in self.node_transactions_by_id.iter() {
            let node_transaction_stage = nt.stage();
            stage_consensus_map
                .entry(node_transaction_stage)
                .or_default()
                .insert(*lnxid);
        }

        stage_consensus_map
    }

    #[expect(dead_code)]
    pub fn created_ts(&self) -> Timestamp {
        self.created_ts
    }

    pub fn durability_expiration(&self) -> DurabilityExpiration {
        // Only Begin/End-stage nodes still rely on keepalives.
        let mut active: Vec<Timestamp> = self
            .node_transactions_by_id
            .values()
            .filter(|nt| {
                matches!(
                    nt.stage(),
                    OutboundTransactionStage::Begin | OutboundTransactionStage::End
                )
            })
            .filter_map(|nt| nt.opt_expiration())
            .collect();

        if active.is_empty() {
            return DurabilityExpiration::NotApplicable;
        }

        let k = self.record_params.required_strict_consensus_count;
        if active.len() < k {
            return DurabilityExpiration::Lost;
        }

        // Sort ascending; cutoff at index (m - k). At any T < cutoff, k nodes
        // have expiration > T (alive). At T >= cutoff, fewer than k remain.
        active.sort_unstable();
        DurabilityExpiration::AliveUntil(active[active.len() - k])
    }

    pub fn stage_ts(&self) -> Timestamp {
        self.node_transactions_by_id
            .values()
            .map(|x| x.stage_ts())
            .reduce(|a, b| a.max(b))
            .unwrap_or(self.created_ts)
    }

    pub fn record_params(&self) -> &OutboundTransactionRecordParams {
        &self.record_params
    }

    pub fn record_key(&self) -> &RecordKey {
        &self.record_params.record_key
    }

    pub fn operation_signer(&self) -> &KeyPair {
        &self.record_params.signing_keypair
    }

    pub fn safety_selection(&self) -> &SafetySelection {
        &self.record_params.safety_selection
    }

    pub fn required_strict_consensus_count(&self) -> usize {
        self.record_params.required_strict_consensus_count
    }

    pub(super) fn prepare(&mut self, routing_table: &RoutingTable, cur_ts: Timestamp) {
        self.opt_registry = Some(routing_table.registry());
        self.node_transactions_by_id
            .retain(|_, v| v.prepare(routing_table, cur_ts));
    }

    pub(super) fn update_descriptor(
        &mut self,
        descriptor: Arc<SignedValueDescriptor>,
    ) -> VeilidAPIResult<()> {
        let schema = descriptor.schema()?;
        if let Some(prev_descriptor) = self.descriptor.clone() {
            if prev_descriptor != descriptor {
                apibail_internal!(
                    "mismatched descriptor {:?} != {:?}",
                    prev_descriptor,
                    descriptor
                );
            }
        }
        self.descriptor = Some(descriptor);
        self.schema = Some(schema);
        Ok(())
    }

    pub fn descriptor(&self) -> Option<Arc<SignedValueDescriptor>> {
        self.descriptor.clone()
    }
    pub fn schema(&self) -> Option<&DHTSchema> {
        self.schema.as_ref()
    }

    pub(super) fn update_begin_network_seqs(
        &mut self,
        seqs: Vec<ValueSeqNum>,
    ) -> VeilidAPIResult<()> {
        let Some(schema) = &self.schema else {
            apibail_internal!("should have schema before seqs");
        };

        if seqs.len() != schema.subkey_count() {
            apibail_internal!(
                "mismatched subkey count {} != {}",
                seqs.len(),
                schema.subkey_count()
            );
        }

        if self.begin_network_seqs.is_empty() {
            self.begin_network_seqs = seqs;
        } else {
            if seqs.len() != self.begin_network_seqs.len() {
                apibail_internal!(
                    "mismatched subkey count that should have been verified already {} != {}",
                    seqs.len(),
                    schema.subkey_count()
                );
            }
            for (ri_seq, seq) in self.begin_network_seqs.iter_mut().zip(seqs) {
                ri_seq.max_assign(seq)
            }
        }

        Ok(())
    }

    pub fn begin_network_seq(&self, subkey: ValueSubkey) -> VeilidAPIResult<ValueSeqNum> {
        self.begin_network_seqs
            .get(usize::try_from(subkey).map_err(VeilidAPIError::internal)?)
            .copied()
            .ok_or_else(|| VeilidAPIError::internal("subkey out of range"))
    }

    /// Count begin-set nodes whose Begin answer reported this subkey at or above min_seq
    pub fn count_begin_holders(&self, subkey: ValueSubkey, min_seq: ValueSeqNum) -> usize {
        self.node_transactions_by_id
            .values()
            .filter(|nt| nt.begin_seq(subkey) >= min_seq)
            .count()
    }

    pub fn local_snapshot(&self) -> Arc<RecordSnapshot> {
        self.record_params.local_snapshot.clone()
    }

    pub(super) fn new_node_transaction(
        &mut self,
        lnxid: LocalNodeTransactionId,
        params: NodeTransactionParams,
    ) -> VeilidAPIResult<&mut NodeTransaction> {
        // Make remote node transaction id from params
        let rnxid = RemoteNodeTransactionId::new(
            params.node_ref.node_ids().get(params.kind).unwrap_or_log(),
            params.xid,
        );

        // Verify unique lnxid and rnxid
        if let Some((k, v)) = self
            .node_transactions_by_id
            .iter()
            .find(|(_, v)| v.rnxid() == &rnxid)
        {
            apibail_internal!(
                "node transaction already exists: lnxid={}, rnxid={}",
                k,
                v.rnxid()
            );
        }
        let registry = self.registry();
        match self.node_transactions_by_id.entry(lnxid) {
            std::collections::btree_map::Entry::Vacant(v) => Ok(v.insert(NodeTransaction::new(
                registry,
                rnxid.clone(),
                params.node_ref,
                params.expiration,
                params.opt_initial_rtt,
                params.begin_seqs,
            ))),
            std::collections::btree_map::Entry::Occupied(_) => {
                Err(VeilidAPIError::internal("node transaction already exists"))
            }
        }
    }

    // pub fn get_node_transaction<'a>(
    //     &'a self,
    //     lnxid: LocalNodeTransactionId,
    // ) -> Option<&'a NodeTransaction> {
    //     self.node_transactions_by_id.get(&lnxid)
    // }

    pub(super) fn get_node_transaction_mut(
        &mut self,
        lnxid: LocalNodeTransactionId,
    ) -> Option<&mut NodeTransaction> {
        self.node_transactions_by_id.get_mut(&lnxid)
    }

    pub fn get_node_transactions_count(&self) -> usize {
        self.node_transactions_by_id.len()
    }

    pub fn get_node_transaction_ids(&self) -> LocalNodeTransactionIdSet {
        self.node_transactions_by_id.keys().copied().collect()
    }

    pub(super) fn get_node_transactions(
        &self,
    ) -> impl Iterator<Item = (&LocalNodeTransactionId, &NodeTransaction)> + '_ {
        self.node_transactions_by_id.iter()
    }

    pub(super) fn get_node_transactions_mut(
        &mut self,
    ) -> impl Iterator<Item = (&LocalNodeTransactionId, &mut NodeTransaction)> + '_ {
        self.node_transactions_by_id.iter_mut()
    }

    pub(super) fn get_transact_command_nodes<'a>(
        &'a self,
        opt_lnxids: Option<LocalNodeTransactionIdSet>,
        opt_filter: Option<GetTransactCommandNodesFilter<'a>>,
    ) -> VeilidAPIResult<OutboundTransactCommandNodes> {
        // Resolve all node transaction ids
        let lnxids = match opt_lnxids {
            None => self.node_transactions_by_id.keys().copied().collect(),
            Some(lnxids) => {
                for lnxid in &lnxids {
                    if !self.node_transactions_by_id.contains_key(lnxid) {
                        apibail_internal!(
                            "tried to get command node for transaction id {} not in record {}",
                            lnxid,
                            self.record_key().opaque()
                        );
                    }
                }
                lnxids
            }
        };

        // Construct the command node list to send the transaction commands to
        let out = self
            .node_transactions_by_id
            .iter()
            .filter(|(lnxid, nt)| {
                if !lnxids.contains(lnxid) {
                    return false;
                }

                if let Some(filter) = opt_filter.as_ref() {
                    if !filter(nt) {
                        return false;
                    }
                }

                true
            })
            .map(|(lnxid, nt)| OutboundTransactCommandNode {
                lnxid: *lnxid,
                rnxid: nt.rnxid().clone(),
                node_ref: nt.node_ref(),
            })
            .collect::<Vec<_>>();

        Ok(out)
    }

    pub(super) fn set_desired_subkey(&mut self, subkey: ValueSubkey, value: Arc<SignedValueData>) {
        self.desired_subkeys.insert(subkey, value);
    }

    pub fn current_consensus(&self) -> &OutboundTransactionConsensus {
        &self.current_consensus
    }

    pub(super) fn current_consensus_mut(&mut self) -> &mut OutboundTransactionConsensus {
        &mut self.current_consensus
    }

    pub fn updated_consensus(&self) -> &OutboundTransactionConsensus {
        &self.updated_consensus
    }

    pub(super) fn updated_consensus_mut(&mut self) -> &mut OutboundTransactionConsensus {
        &mut self.updated_consensus
    }

    pub fn current_subkey_get_result(&self, subkey: ValueSubkey) -> VeilidAPIResult<GetResult> {
        let opt_descriptor = self.descriptor();
        let opt_snapshot_value = self
            .record_params
            .local_snapshot
            .subkey_value_data(subkey)?;

        let opt_state_value = self
            .current_consensus
            .get(subkey)
            .and_then(|ss| ss.opt_value.clone());

        let opt_value = match (opt_snapshot_value, opt_state_value) {
            (None, None) => None,
            (None, Some(b)) => Some(b),
            (Some(a), None) => Some(a),
            (Some(a), Some(b)) => {
                let a_seq = a.value_data().seq();
                let b_seq = b.value_data().seq();

                if a_seq > b_seq {
                    Some(a)
                } else if a_seq < b_seq {
                    Some(b)
                } else {
                    // Always defer to the network copy if conflicting or equal
                    Some(b)
                }
            }
        };

        Ok(GetResult {
            opt_value,
            opt_descriptor,
        })
    }

    pub fn local_commit_results(
        &self,
    ) -> VeilidAPIResult<Vec<(ValueSubkey, Arc<SignedValueData>)>> {
        let Some(max_subkey) = self.schema().map(|s| s.max_subkey()) else {
            return Ok(vec![]);
        };

        let mut out = vec![];
        for subkey in 0..=max_subkey {
            let opt_current_value = self
                .current_consensus
                .get(subkey)
                .and_then(|sc| sc.opt_value.clone());
            let opt_updated_value = self
                .updated_consensus
                .get(subkey)
                .and_then(|sc| sc.opt_value.clone());

            if let Some(updated_value) = opt_updated_value {
                out.push((subkey, updated_value));
            } else if let Some(current_value) = opt_current_value {
                let opt_snapshot_value = self
                    .record_params
                    .local_snapshot
                    .subkey_value_data(subkey)?;
                if let Some(snapshot_value) = opt_snapshot_value {
                    if current_value.value_data().seq() > snapshot_value.value_data().seq() {
                        out.push((subkey, current_value));
                    }
                } else {
                    out.push((subkey, current_value));
                }
            }
        }
        Ok(out)
    }

    pub(super) fn remove_node_transaction(
        &mut self,
        lnxid: LocalNodeTransactionId,
    ) -> Option<NodeTransaction> {
        self.node_transactions_by_id.remove(&lnxid)
    }
}
