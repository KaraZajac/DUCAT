mod local_node_transaction_id;
mod node_transaction;
mod outbound_transaction_handle;
mod outbound_transaction_keepalive_deadlines;
mod outbound_transaction_keepalive_processor;
mod outbound_transaction_record_state;
mod outbound_transaction_stage;
mod outbound_transaction_state;
mod remote_node_transaction_id;
mod subkey_consensus;
mod transaction_cleanup;

use super::*;

use transaction_begin::{OutboundTransactBeginParams, OutboundTransactBeginResult};
use transaction_command::{
    OutboundTransactCommandNode, OutboundTransactCommandNodes, OutboundTransactCommandParams,
    OutboundTransactCommandPerNodeResult, OutboundTransactCommandResult,
    TransactCommandDisposition,
};

pub(in crate::storage_manager) use local_node_transaction_id::*;
pub(in crate::storage_manager) use node_transaction::*;
pub(in crate::storage_manager) use outbound_transaction_keepalive_processor::*;
pub(in crate::storage_manager) use outbound_transaction_record_state::*;
pub(in crate::storage_manager) use outbound_transaction_stage::*;
pub(in crate::storage_manager) use outbound_transaction_state::*;
pub(in crate::storage_manager) use remote_node_transaction_id::*;
pub(in crate::storage_manager) use subkey_consensus::*;
pub(in crate::storage_manager) use transaction_cleanup::*;

pub use outbound_transaction_handle::*;

impl_veilid_log_facility!("stor");

/// Parameters for adding a node transaction to an existing transaction
pub(in crate::storage_manager) struct AddNodeTransactionParams {
    /// Handle of the transaction this node belongs to
    pub transaction_handle: OutboundTransactionHandle,
    /// Record key this node transaction is for
    pub opaque_record_key: OpaqueRecordKey,
    /// Server-side transaction id
    pub xid: u64,
    /// Ref to the node running this transaction
    pub node_ref: NodeRef,
    /// When the server says this node transaction is dead
    pub expiration: Timestamp,
    /// Destination used to reach this node
    pub dest: Destination,
    /// Record descriptor
    pub descriptor: Arc<SignedValueDescriptor>,
    /// RTT measured during Begin, used to seed the per-node RTT estimate
    pub opt_begin_rtt: Option<TimestampDuration>,
    /// Per-subkey seqs this node reported in its Begin answer
    pub begin_seqs: Vec<ValueSeqNum>,
}

#[derive(Debug, Clone)]
struct LocalNodeTransactionMapping {
    transaction_handle: OutboundTransactionHandle,
    opaque_record_key: OpaqueRecordKey,
}

#[derive(Debug)]
pub(in crate::storage_manager) struct OutboundTransactionManager {
    /// Registry used for logging
    registry: VeilidComponentRegistry,
    /// Record key to handle map
    handles_by_key: HashMap<OpaqueRecordKey, OutboundTransactionHandle>,
    /// Each transaction per record key
    transactions: HashMap<OutboundTransactionHandle, OutboundTransactionState>,
    /// Next transaction id to assign
    next_txid: u64,
    /// Local node transaction ids to transaction mapping.
    /// Invariant: an entry exists iff the corresponding node_transaction exists.
    local_node_transaction_mappings: HashMap<LocalNodeTransactionId, LocalNodeTransactionMapping>,
    /// Next local node transaction id to assign
    next_lnxid: u64,

    /// Keepalive processor
    opt_keepalive_processor: Option<Arc<OutboundTransactionKeepaliveProcessor>>,
}

impl_veilid_component_accessors!(OutboundTransactionManager);

impl fmt::Display for OutboundTransactionManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = format!("transactions({}): [\n", self.transactions.len());
        {
            let mut keys = self.transactions.keys().cloned().collect::<Vec<_>>();
            keys.sort_unstable();

            for k in keys {
                let v = self.transactions.get(&k).unwrap_or_log();
                out += &format!("  {}:\n{}\n", k, indent_all_by(4, f.to_string(v)));
            }
        }
        out += "]\n";

        write!(f, "{}", out)
    }
}

type OutboundTransactionPerNodeResultHandler<'a> = Box<
    dyn FnMut(&mut NodeTransaction, OutboundTransactCommandPerNodeResult) -> VeilidAPIResult<()>
        + 'a,
>;

pub type OutboundTransactionManagerTerminate = PinBoxFutureStatic<()>;

impl OutboundTransactionManager {
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            registry,
            handles_by_key: HashMap::new(),
            transactions: HashMap::new(),
            local_node_transaction_mappings: HashMap::new(),
            next_txid: 0u64,
            next_lnxid: 1_000_000_000u64,
            opt_keepalive_processor: None,
        }
    }

    pub fn init(&mut self) {
        let cur_ts = Timestamp::now();
        let routing_table = self.routing_table();
        for transaction in self.transactions.values_mut() {
            transaction.prepare(&routing_table, cur_ts);
        }

        let keepalive_processor = Arc::new(OutboundTransactionKeepaliveProcessor::new(
            routing_table.registry(),
        ));

        keepalive_processor.init();

        self.opt_keepalive_processor = Some(keepalive_processor);
    }

    pub fn terminate(&mut self) -> OutboundTransactionManagerTerminate {
        let opt_keepalive_processor = self.opt_keepalive_processor.take();
        Box::pin(async move {
            if let Some(keepalive_processor) = opt_keepalive_processor {
                keepalive_processor.terminate().await;
            }
        })
    }

    fn allocate_transaction_handle(&mut self) -> VeilidAPIResult<OutboundTransactionHandle> {
        let start_txid = self.next_txid;
        loop {
            // Get txid and increment id allocator
            let handle = OutboundTransactionHandle::new(self.next_txid);
            self.next_txid = self.next_txid.wrapping_add(1);

            if self.transactions.contains_key(&handle) {
                if self.next_txid == start_txid {
                    // This should basically never happen, but logic is here for completeness
                    veilid_log!(self debugwarn "no free transaction handles available, wrapped around to start_txid={}", start_txid);
                    apibail_try_again!("no free transaction handles available");
                }
                // Iterate and try again with the next txid
            } else {
                // Got a unique transaction handle, return it
                break Ok(handle);
            }
        }
    }

    fn allocate_local_node_transaction_id(&mut self) -> VeilidAPIResult<LocalNodeTransactionId> {
        let start_lnxid = self.next_lnxid;
        loop {
            // Get lnxid and increment id allocator
            let lnxid = LocalNodeTransactionId::new(self.next_lnxid);
            self.next_lnxid = self.next_lnxid.wrapping_add(1);

            if self.local_node_transaction_mappings.contains_key(&lnxid) {
                if self.next_lnxid == start_lnxid {
                    // This should basically never happen, but logic is here for completeness
                    veilid_log!(self debugwarn "no free local node transaction ids available, wrapped around to start_lnxid={}", start_lnxid);
                    apibail_try_again!("no free local node transaction ids available");
                }
                // Iterate and try again with the next lnxid
            } else {
                // Got a unique local node transaction id, return it
                break Ok(lnxid);
            }
        }
    }

    /// Create a new transaction over a set of records
    pub fn new_transaction(
        &mut self,
        record_params: Vec<OutboundTransactionRecordParams>,
        kind: OutboundTransactionKind,
    ) -> VeilidAPIResult<OutboundTransactionHandle> {
        // Ensure no other transactions are using any of these record keys and make handle
        let mut opaque_record_keys = vec![];
        for rp in &record_params {
            let opaque_record_key = rp.record_key.opaque();
            if self.handles_by_key.contains_key(&opaque_record_key) {
                apibail_generic!(
                    "Record {} already has a a transaction open",
                    opaque_record_key
                );
            }
            opaque_record_keys.push(opaque_record_key);
        }
        let transaction_handle = self.allocate_transaction_handle()?;

        // Create a new outbound transaction state
        let mut outbound_transaction_state = OutboundTransactionState::new(self.registry(), kind);

        // Add all records
        for rp in record_params {
            outbound_transaction_state.new_record_state(rp)?;
        }

        // Add to transaction list
        for opaque_record_key in opaque_record_keys {
            self.handles_by_key
                .insert(opaque_record_key, transaction_handle);
        }
        self.transactions
            .insert(transaction_handle, outbound_transaction_state);

        // Success, return the transaction handle
        Ok(transaction_handle)
    }

    /// Add a new node transaction to an existing transaction
    pub fn add_node_transaction(
        &mut self,
        params: AddNodeTransactionParams,
    ) -> VeilidAPIResult<()> {
        let lnxid = self.allocate_local_node_transaction_id()?;

        let outbound_transaction_state =
            self.get_transaction_state_mut(params.transaction_handle)?;
        let record_state = outbound_transaction_state
            .get_record_state_mut(&params.opaque_record_key)
            .ok_or_else(|| VeilidAPIError::internal("missing record state"))?;
        let nt = record_state.new_node_transaction(
            lnxid,
            NodeTransactionParams {
                kind: params.opaque_record_key.kind(),
                xid: params.xid,
                node_ref: params.node_ref,
                expiration: params.expiration,
                opt_initial_rtt: params.opt_begin_rtt,
                begin_seqs: params.begin_seqs,
            },
        )?;
        let rnxid = nt.rnxid().clone();

        // Add local node transaction mapping
        self.local_node_transaction_mappings.insert(
            lnxid,
            LocalNodeTransactionMapping {
                transaction_handle: params.transaction_handle,
                opaque_record_key: params.opaque_record_key.clone(),
            },
        );

        // Register with the keepalive processor for this node transaction
        // Done here so that it happens in the OTM inside the StorageManager inner lock,
        // along with the rest of the node transaction addition, to reduce the risk of race conditions with the keepalive processor.
        if let Some(keepalive_processor) = &self.opt_keepalive_processor {
            keepalive_processor.register(
                OutboundTransactionKeepaliveParams {
                    lnxid,
                    opaque_record_key: params.opaque_record_key,
                    rnxid,
                    dest: params.dest,
                    descriptor: params.descriptor,
                },
                params.expiration,
            );
        }

        Ok(())
    }

    fn get_transaction_record_state_mut_by_lnxid(
        &mut self,
        transaction_handle: OutboundTransactionHandle,
        lnxid: LocalNodeTransactionId,
    ) -> Option<&mut OutboundTransactionRecordState> {
        let registry = self.registry();

        let Some(mapping) = self.local_node_transaction_mappings.get(&lnxid) else {
            veilid_log!(registry debug "Missing local node transaction mapping {} for transaction {} in background drop", lnxid, transaction_handle);
            return None;
        };

        let Some(state) = self.transactions.get_mut(&mapping.transaction_handle) else {
            veilid_log!(registry debug "Missing transaction state for node transaction {} for transaction {} in background drop", lnxid, transaction_handle);
            return None;
        };

        if mapping.transaction_handle != transaction_handle {
            veilid_log!(registry debug "Mismatched transaction handle {} for node transaction {} for transaction {} in background drop", mapping.transaction_handle, lnxid, transaction_handle);
            return None;
        }

        let Some(record_state) = state.get_record_state_mut(&mapping.opaque_record_key) else {
            veilid_log!(registry debug "Missing record state for node transaction {} for transaction {} in background drop", lnxid, transaction_handle);
            return None;
        };

        Some(record_state)
    }

    fn get_transaction_record_state_by_lnxid(
        &self,
        transaction_handle: OutboundTransactionHandle,
        lnxid: LocalNodeTransactionId,
    ) -> Option<&OutboundTransactionRecordState> {
        let registry = self.registry();

        let Some(mapping) = self.local_node_transaction_mappings.get(&lnxid) else {
            veilid_log!(registry debug "Missing local node transaction mapping {} for transaction {} in background drop", lnxid, transaction_handle);
            return None;
        };

        let Some(state) = self.transactions.get(&mapping.transaction_handle) else {
            veilid_log!(registry debug "Missing transaction state for node transaction {} for transaction {} in background drop", lnxid, transaction_handle);
            return None;
        };

        if mapping.transaction_handle != transaction_handle {
            veilid_log!(registry debug "Mismatched transaction handle {} for node transaction {} for transaction {} in background drop", mapping.transaction_handle, lnxid, transaction_handle);
            return None;
        }

        let Some(record_state) = state.get_record_state(&mapping.opaque_record_key) else {
            veilid_log!(registry debug "Missing record state for node transaction {} for transaction {} in background drop", lnxid, transaction_handle);
            return None;
        };

        Some(record_state)
    }

    /// Remove node transactions from a transaction
    /// Adds a stop token to record the background rollback task upon transaction cleanup.
    pub fn drop_node_transactions(
        &mut self,
        transaction_handle: OutboundTransactionHandle,
        node_transactions_to_drop: LocalNodeTransactionIdSet,
    ) -> VeilidAPIResult<()> {
        let registry = self.registry();

        for lnxid in node_transactions_to_drop {
            let Some(record_state) =
                self.get_transaction_record_state_mut_by_lnxid(transaction_handle, lnxid)
            else {
                // Missing record state, skip, already logged
                continue;
            };

            // Drop node transaction
            if record_state.remove_node_transaction(lnxid).is_none() {
                veilid_log!(registry debug "Missing node transaction {} for transaction {} in background drop", lnxid, transaction_handle);
            }

            // Drop keepalive
            if let Some(keepalive_processor) = &self.opt_keepalive_processor {
                keepalive_processor.unregister(lnxid);
            }

            // Drop mapping
            if self
                .local_node_transaction_mappings
                .remove(&lnxid)
                .is_none()
            {
                veilid_log!(registry debug "Missing mapping for node transaction {} for transaction {} in background drop", lnxid, transaction_handle);
            }
        }

        Ok(())
    }

    /// Add background task to a transaction
    /// Registers the background task with a cleanup token that can be waited on to ensure the task has completed.
    pub fn add_transaction_background_task(
        &mut self,
        transaction_handle: OutboundTransactionHandle,
        task: impl Future<Output = ()> + Send + 'static,
    ) -> VeilidAPIResult<()> {
        let stop_source = StopSource::new();
        let stop_token = stop_source.token();

        let state = self.get_transaction_state_mut(transaction_handle)?;
        state.add_background_token(stop_token);

        let registry = self.registry();
        let fut = async move {
            task.await;

            let storage_manager = registry.storage_manager();

            // If the transaction still exists, remove the completed background tokens
            // It may not exist other errors happened after the the partial_drop_and_background_rollback_locked
            {
                let mut inner = storage_manager.inner.lock();
                if let Ok(transaction_state) = inner
                    .outbound_transaction_manager
                    .get_transaction_state_mut(transaction_handle)
                {
                    transaction_state.remove_completed_background_tokens();
                }
            }

            // Move the stop source in here and drop it when we're done
            drop(stop_source);
        };

        let storage_manager = self.storage_manager();
        storage_manager
            .background_operation_processor
            .add_future(fut);

        Ok(())
    }

    /// Merge two transactions into a single transaction
    /// Additional transaction is merged into the existing transaction and the additional transaction handle is released
    pub fn merge_transactions(
        &mut self,
        existing_transaction_handle: OutboundTransactionHandle,
        additional_transaction_handle: OutboundTransactionHandle,
    ) -> VeilidAPIResult<()> {
        // Existing transaction must exist and be in Begin stage.
        let Some(existing_state) = self.transactions.get(&existing_transaction_handle) else {
            apibail_transaction_not_found!(
                "existing transaction does not exist: {}",
                existing_transaction_handle
            );
        };
        match existing_state.stage_consensus().map(|c| c.stage) {
            Some(OutboundTransactionStage::Begin) => {}
            Some(other) => {
                apibail_transaction_not_found!(
                    "existing transaction stage is {:?}, must be Begin to extend: {}",
                    other,
                    existing_transaction_handle
                );
            }
            None => {
                apibail_transaction_not_found!(
                    "existing transaction not yet started: {}",
                    existing_transaction_handle
                );
            }
        }
        if !self
            .transactions
            .contains_key(&additional_transaction_handle)
        {
            veilid_log!(self debugwarn "Dropping non-existent merge transaction: {:?}", additional_transaction_handle);
            apibail_invalid_argument!(
                "additional transaction does not exist",
                "additional_transaction_handle",
                additional_transaction_handle.to_string()
            );
        }

        veilid_log!(self debug target: "network_result", "Merging transaction {} -> {}", additional_transaction_handle, existing_transaction_handle);

        // Get and remove additional transaction state
        // Unwrap is safe because we just checked contains_key
        let additional_outbound_transaction_state = self
            .transactions
            .remove(&additional_transaction_handle)
            .unwrap_or_log();

        // Move handle record key mappings from additional transaction to existing transaction
        for additional_record_state in additional_outbound_transaction_state.get_record_states() {
            let additional_opaque_record_key = additional_record_state.record_key().opaque();
            if let Some(old) = self.handles_by_key.insert(
                additional_opaque_record_key.clone(),
                existing_transaction_handle,
            ) {
                if old != additional_transaction_handle {
                    veilid_log!(self error "Incorrect transaction handle mapping for record {}: {} != {}", additional_opaque_record_key, old, additional_transaction_handle);
                }
            } else {
                veilid_log!(self error "Missing prior transaction handle mapping for record {}: {}", additional_opaque_record_key, additional_transaction_handle);
            }
        }

        // Get existing transaction state
        // Unwrap is safe because we just checked contains_key
        let existing_outbound_transaction_state = self
            .get_transaction_state_mut(existing_transaction_handle)
            .unwrap_or_log();

        // Merge additional transaction state into existing transaction state
        existing_outbound_transaction_state.merge(additional_outbound_transaction_state);

        // Move local node transaction mappings
        for mapping in self.local_node_transaction_mappings.values_mut() {
            if mapping.transaction_handle == additional_transaction_handle {
                mapping.transaction_handle = existing_transaction_handle;
            }
        }

        Ok(())
    }

    /// Drop a transaction completely. Does not error.
    /// If the transaction does not exist, this does nothing and returns None.
    /// If the transaction does exist, it is returned as Some(transaction) after being removed.
    #[must_use]
    pub fn drop_transaction(
        &mut self,
        transaction_handle: OutboundTransactionHandle,
    ) -> Option<OutboundTransactionState> {
        let outbound_transaction_state = match self.transactions.remove(&transaction_handle) {
            Some(x) => x,
            None => {
                veilid_log!(self debugwarn "Dropping non-existent transaction: {:?}", transaction_handle);
                return None;
            }
        };

        veilid_log!(self debug target: "network_result", "Dropping transaction: {:?}", transaction_handle);

        let mut unregister_keepalive_lnxids = LocalNodeTransactionIdSet::new();
        for record_state in outbound_transaction_state.get_record_states() {
            let lnxids = record_state.get_node_transaction_ids();

            // Drop mappings
            for lnxid in &lnxids {
                if self.local_node_transaction_mappings.remove(lnxid).is_none() {
                    veilid_log!(self debug "Missing mapping for node transaction {} for transaction {} in drop", lnxid, transaction_handle);
                }
            }

            unregister_keepalive_lnxids.extend(lnxids);

            self.handles_by_key
                .remove(&record_state.record_key().opaque());
        }

        // Cancel and clean up keepalives for dropped transaction
        // if the keepalive processor is still active. Done in the OTM inside the StorageManager inner lock,
        // along with the rest of the transaction cleanup, to reduce the risk of race conditions with the keepalive processor.
        if let Some(keepalive_processor) = &self.opt_keepalive_processor {
            for unregister_keepalive_lnxid in unregister_keepalive_lnxids {
                keepalive_processor.unregister(unregister_keepalive_lnxid);
            }
        }
        Some(outbound_transaction_state)
    }

    /// Get transaction handle for record
    pub fn get_transaction_by_record(
        &self,
        opaque_record_key: &OpaqueRecordKey,
    ) -> Option<OutboundTransactionHandle> {
        self.handles_by_key.get(opaque_record_key).cloned()
    }

    /// Get a transaction's foreground/background kind
    pub fn get_transaction_kind(
        &self,
        transaction_handle: OutboundTransactionHandle,
    ) -> Option<OutboundTransactionKind> {
        self.transactions
            .get(&transaction_handle)
            .map(|tx| tx.kind())
    }

    /// Check if any foreground transaction is active
    pub fn has_foreground_transaction(&self) -> bool {
        self.transactions
            .values()
            .any(|tx| tx.kind() == OutboundTransactionKind::Foreground)
    }

    /// Check if a transaction exists and return the opaque record keys associated with it if it does
    pub fn get_transaction_keys(
        &self,
        transaction_handle: OutboundTransactionHandle,
    ) -> Option<Vec<OpaqueRecordKey>> {
        self.transactions
            .get(&transaction_handle)
            .map(|tx| tx.keys())
    }

    /// Get a transaction state
    pub fn get_transaction_state(
        &self,
        transaction_handle: OutboundTransactionHandle,
    ) -> VeilidAPIResult<&OutboundTransactionState> {
        self.transactions
            .get(&transaction_handle)
            .ok_or_else(|| VeilidAPIError::transaction_not_found("missing transaction"))
    }

    /// Modify a transaction state
    fn get_transaction_state_mut(
        &mut self,
        transaction_handle: OutboundTransactionHandle,
    ) -> VeilidAPIResult<&mut OutboundTransactionState> {
        self.transactions
            .get_mut(&transaction_handle)
            .ok_or_else(|| VeilidAPIError::transaction_not_found("missing transaction"))
    }

    /// Iterate transaction handles and states
    pub fn transactions(
        &self,
    ) -> impl Iterator<Item = (&OutboundTransactionHandle, &OutboundTransactionState)> {
        self.transactions.iter()
    }

    /// Prepare to begin a transaction
    pub fn prepare_transact_begin_params(
        &self,
        transaction_handle: OutboundTransactionHandle,
    ) -> VeilidAPIResult<Vec<Arc<OutboundTransactBeginParams>>> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state(transaction_handle)?;

        // Assert stage
        if let Some(stage_consensus) = outbound_transaction_state.stage_consensus() {
            apibail_transaction_not_found!("stage was {:?}, wanted Init", stage_consensus.stage,);
        }

        let mut out = vec![];
        for record_state in outbound_transaction_state.get_record_states() {
            out.push(Arc::new(OutboundTransactBeginParams {
                transaction_handle,
                record_params: record_state.record_params().clone(),
            }));
        }

        Ok(out)
    }

    /// Record begin transaction
    pub fn record_transact_begin_results(
        &mut self,
        results: Vec<OutboundTransactBeginResult>,
    ) -> VeilidAPIResult<()> {
        #[cfg(feature = "verbose-tracing")]
        let registry = self.registry();

        // Get the required strict consensus count
        let required_strict_consensus_count =
            self.config().internal().network.dht.set_value_count as usize;

        // Add all node transaction ids
        // Each record must still be in Init state (no node_transactions yet) when being recorded
        for result in results {
            let opaque_record_key = result.params.record_params.record_key.opaque();
            let transaction_handle = result.params.transaction_handle;

            // Get transaction
            let outbound_transaction_state = self.get_transaction_state_mut(transaction_handle)?;

            // Hold the route refs used by this transaction
            outbound_transaction_state.add_route_refs(result.route_refs);

            // Get record state
            let Some(record_state) =
                outbound_transaction_state.get_record_state_mut(&opaque_record_key)
            else {
                apibail_internal!(
                    "missing record during begin results recording: {}",
                    opaque_record_key
                );
            };

            // Ensure results came in with enough consensus
            let node_transaction_count = record_state.get_node_transactions_count();
            if node_transaction_count < required_strict_consensus_count {
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(registry debug target: "dht", "Did not get a consensus of transaction ids for begin for handle={} record={}: merged_seqs={} with state: {}",
                    transaction_handle,
                    opaque_record_key,
                    result.seqs.to_table_string(),
                    record_state
                );

                apibail_try_again!("did not get consensus of transaction ids in begin (rec={}, count={}, required_consensus={})",
                    opaque_record_key,
                    node_transaction_count,
                    required_strict_consensus_count
                );
            }

            // Update record state with results
            record_state.update_descriptor(result.descriptor)?;
            record_state.update_begin_network_seqs(result.seqs)?;

            // Assert this record is now in the Begin state
            let Some(stage_consensus) = record_state.stage_consensus() else {
                apibail_generic!(
                    "record {} has no stage consensus during begin results recording",
                    opaque_record_key
                );
            };

            if stage_consensus.stage != OutboundTransactionStage::Begin {
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(registry debug target: "dht", "record {} did not get Begin consensus with state: {}", opaque_record_key, record_state);

                apibail_generic!(
                    "record {} has stage consensus {:?} during begin results recording, wanted Begin",
                    opaque_record_key,
                    stage_consensus.stage
                );
            }

            #[cfg(feature = "verbose-tracing")]
            veilid_log!(registry debug target: "dht", "Begin results for record {}: {}", opaque_record_key, record_state);
        }

        Ok(())
    }

    /// Generic transact command result recording boilerplate common to all results
    fn record_transact_command_results(
        outbound_transaction_state: &mut OutboundTransactionState,
        results: Vec<OutboundTransactCommandResult>,
        mut callback: OutboundTransactionPerNodeResultHandler<'_>,
    ) -> VeilidAPIResult<()> {
        // Record results
        for result in results {
            let opaque_record_key = &result.params.opaque_record_key;

            let Some(record_state) =
                outbound_transaction_state.get_record_state_mut(opaque_record_key)
            else {
                apibail_internal!("missing record: {}", opaque_record_key);
            };

            callback =
                Self::record_transact_command_per_record_results(record_state, result, callback)?
        }
        Ok(())
    }

    /// Generic transact command per-record result recording boilerplate common to all record results
    fn record_transact_command_per_record_results<'a>(
        record_state: &mut OutboundTransactionRecordState,
        result: OutboundTransactCommandResult,
        mut callback: OutboundTransactionPerNodeResultHandler<'a>,
    ) -> VeilidAPIResult<OutboundTransactionPerNodeResultHandler<'a>> {
        let mut command_lnxids = result.get_command_lnxids();
        for pnr in result.per_node_results {
            if !command_lnxids.remove(&pnr.lnxid) {
                apibail_internal!(
                    "node transaction has multiple results: {} pnr={:?}",
                    result.params.opaque_record_key,
                    pnr
                );
            }

            let node_transaction = record_state
                .get_node_transaction_mut(pnr.lnxid)
                .ok_or_else(|| VeilidAPIError::internal("missing node transaction"))?;

            match pnr.disposition {
                TransactCommandDisposition::Invalid => {
                    // If not valid, the server already rolled it back
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(node_transaction debug target: "dht", "transact_command_result: lnxid={} rnxid={} INVALID (server rolled back) record={}",
                        pnr.lnxid, pnr.rnxid, result.params.opaque_record_key);
                    node_transaction.set_stage(OutboundTransactionStage::Rollback, None);
                    continue;
                }
                TransactCommandDisposition::Skipped => {
                    // Node was not waited for due to early consensus exit, leave its stage unchanged
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(node_transaction debug target: "dht", "transact_command_result: lnxid={} rndid={} SKIPPED (no response/early exit) record={} stage={:?}",
                        pnr.lnxid, pnr.rnxid, result.params.opaque_record_key, node_transaction.stage());
                    continue;
                }
                TransactCommandDisposition::Valid => {
                    // Refresh the per-node RTT from this successful response
                    if let Some(rtt) = pnr.opt_rtt {
                        node_transaction.record_rtt(rtt);
                    }
                    // If transaction is still valid, then call the processing callback
                    callback(node_transaction, pnr)?;
                }
            }
        }

        // Any commands that did not return a result have their node transactions marked as failed
        for missing_lnxid in command_lnxids {
            let Some(node_transaction) = record_state.get_node_transaction_mut(missing_lnxid)
            else {
                apibail_internal!(
                    "missing node transaction in record state: {}",
                    missing_lnxid,
                );
            };
            #[cfg(feature = "verbose-tracing")]
            veilid_log!(node_transaction debug target: "dht", "transact_command_result: node={} MISSING result → Failed",
                missing_lnxid);
            node_transaction.set_stage(OutboundTransactionStage::Failed, None);
        }
        Ok(callback)
    }

    /// Prepare to rollback a transaction.
    ///
    /// Best-effort: if Begin never reached consensus (or got zero accepts), this returns
    /// rollback params only for nodes that actually did accept Begin. If no nodes ever
    /// accepted, returns an empty list (nothing to roll back). The caller still gets the
    /// original begin error (TryAgain / etc.) propagated through rollback_guard_locked.
    pub fn prepare_rollback_transact_value_params(
        &self,
        transaction_handle: OutboundTransactionHandle,
        opt_rollback_ids: Option<LocalNodeTransactionIdSet>,
    ) -> VeilidAPIResult<Vec<Arc<OutboundTransactCommandParams>>> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state(transaction_handle)?;

        // If rollback ids are specified, just go with it
        // Otherwise get the full set of fail rollback ids (empty if no nodes accepted)
        let rollback_ids = match opt_rollback_ids {
            Some(rbids) => rbids,
            None => outbound_transaction_state.get_all_rollbacks(),
        };

        let mut out = vec![];

        // Split up rollback ids by record key
        let mut rollback_ids_by_record_key: BTreeMap<OpaqueRecordKey, LocalNodeTransactionIdSet> =
            BTreeMap::new();
        for rollback_id in rollback_ids {
            let Some(record_state) =
                self.get_transaction_record_state_by_lnxid(transaction_handle, rollback_id)
            else {
                // Missing record state, skip, already logged
                continue;
            };
            rollback_ids_by_record_key
                .entry(record_state.record_key().opaque())
                .or_default()
                .insert(rollback_id);
        }

        let outbound_transaction_state = self.get_transaction_state(transaction_handle)?;
        for (opaque_record_key, record_rollback_ids) in rollback_ids_by_record_key {
            let Some(record_state) =
                outbound_transaction_state.get_record_state(&opaque_record_key)
            else {
                apibail_internal!(
                    "Missing record state for {} in rollback for transaction {}: ",
                    opaque_record_key,
                    transaction_handle
                );
            };

            let safety_selection = record_state.safety_selection().clone();
            let nodes = record_state.get_transact_command_nodes(Some(record_rollback_ids), None)?;

            out.push(Arc::new(OutboundTransactCommandParams {
                opaque_record_key,
                safety_selection,
                nodes,
                command: TransactCommand::Rollback,
                opt_seqs: None,
                opt_subkey: None,
                opt_value: None,
                required_strict_consensus_count: 0,
                pre_authorized_valid_count: 0,
            }));
        }

        Ok(out)
    }

    /// Record rollback transaction
    pub fn record_transact_rollback_results(
        &mut self,
        transaction_handle: OutboundTransactionHandle,
        results: Vec<OutboundTransactCommandResult>,
    ) -> VeilidAPIResult<()> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state_mut(transaction_handle)?;

        // If there are no results, nothing to record. This happens when a rollback was
        // issued for a transaction that never reached Begin consensus (zero nodes accepted).
        if results.is_empty() {
            return Ok(());
        }

        // Record results
        Self::record_transact_command_results(
            outbound_transaction_state,
            results,
            Box::new(
                |node_transaction: &mut NodeTransaction,
                 _: OutboundTransactCommandPerNodeResult| {
                    // Transition to rollback stage
                    node_transaction.set_stage(OutboundTransactionStage::Rollback, None);
                    Ok(())
                },
            ) as OutboundTransactionPerNodeResultHandler,
        )?;

        Ok(())
    }

    /// Prepare to end a transaction
    pub fn prepare_transact_end_params(
        &self,
        transaction_handle: OutboundTransactionHandle,
    ) -> VeilidAPIResult<Vec<Arc<OutboundTransactCommandParams>>> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state(transaction_handle)?;

        // Assert stage
        let stage = outbound_transaction_state
            .stage_consensus()
            .ok_or_else(|| VeilidAPIError::generic("transaction not started"))?
            .stage;
        match stage {
            OutboundTransactionStage::Begin => {}
            OutboundTransactionStage::End
            | OutboundTransactionStage::Failed
            | OutboundTransactionStage::Rollback
            | OutboundTransactionStage::Commit => {
                apibail_transaction_not_found!("stage was {:?}, wanted Begin", stage);
            }
        }

        let mut out = vec![];

        let mut unregister_keepalive_lnxids = LocalNodeTransactionIdSet::new();

        for record_state in outbound_transaction_state.get_record_states() {
            let opaque_record_key = record_state.record_key().opaque();
            let safety_selection = record_state.safety_selection().clone();
            let nodes = record_state.get_transact_command_nodes(None, None)?;
            let required_strict_consensus_count = record_state.required_strict_consensus_count();

            for node in &nodes {
                unregister_keepalive_lnxids.insert(node.lnxid);
            }

            out.push(Arc::new(OutboundTransactCommandParams {
                opaque_record_key,
                safety_selection,
                nodes,
                command: TransactCommand::End,
                opt_seqs: None,
                opt_subkey: None,
                opt_value: None,
                required_strict_consensus_count,
                pre_authorized_valid_count: 0,
            }));
        }

        // Cancel all scheduled keepalives
        if let Some(keepalive_processor) = &self.opt_keepalive_processor {
            for lnxid in unregister_keepalive_lnxids {
                keepalive_processor.unregister(lnxid);
            }
        }

        Ok(out)
    }

    /// Record end transaction
    pub fn record_transact_end_results(
        &mut self,
        transaction_handle: OutboundTransactionHandle,
        results: Vec<OutboundTransactCommandResult>,
    ) -> VeilidAPIResult<()> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state_mut(transaction_handle)?;

        // Assert stage
        let stage = outbound_transaction_state
            .stage_consensus()
            .ok_or_else(|| VeilidAPIError::generic("transaction not started"))?
            .stage;
        match stage {
            OutboundTransactionStage::Begin => {}
            OutboundTransactionStage::End
            | OutboundTransactionStage::Failed
            | OutboundTransactionStage::Rollback
            | OutboundTransactionStage::Commit => {
                apibail_transaction_not_found!("stage was {:?}, wanted Begin", stage);
            }
        }

        // Record results
        Self::record_transact_command_results(
            outbound_transaction_state,
            results,
            Box::new(
                |node_transaction: &mut NodeTransaction,
                 pnr: OutboundTransactCommandPerNodeResult| {
                    // Transition to end stage
                    node_transaction.set_stage(OutboundTransactionStage::End, pnr.opt_expiration);
                    Ok(())
                },
            ) as OutboundTransactionPerNodeResultHandler,
        )?;

        Ok(())
    }

    /// Prepare to commit a transaction
    pub fn prepare_transact_commit_params(
        &self,
        transaction_handle: OutboundTransactionHandle,
    ) -> VeilidAPIResult<Vec<Arc<OutboundTransactCommandParams>>> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state(transaction_handle)?;

        // Assert stage
        let stage = outbound_transaction_state
            .stage_consensus()
            .ok_or_else(|| VeilidAPIError::generic("transaction not started"))?
            .stage;
        match stage {
            OutboundTransactionStage::End => {}
            OutboundTransactionStage::Begin
            | OutboundTransactionStage::Failed
            | OutboundTransactionStage::Rollback
            | OutboundTransactionStage::Commit => {
                apibail_transaction_not_found!("stage was {:?}, wanted End", stage);
            }
        }

        let mut out = vec![];

        for record_state in outbound_transaction_state.get_record_states() {
            let opaque_record_key = record_state.record_key().opaque();
            let safety_selection = record_state.safety_selection().clone();
            // commit_will_change_remote() logs internally when returning false
            let nodes = record_state.get_transact_command_nodes(
                None,
                Some(Box::new(|nt: &NodeTransaction| {
                    nt.commit_will_change_remote()
                })),
            )?;
            // Nodes pre-promoted to Commit stage (no-op commits) count toward
            // consensus without an RPC, so seed the fanout's valid count.
            let pre_authorized_valid_count = record_state
                .get_node_transactions()
                .filter(|(_, nt)| !nt.commit_will_change_remote())
                .count();
            #[cfg(feature = "verbose-tracing")]
            veilid_log!(self debug target: "dht", "prepare_commit: record={} nodes_requiring_commit_rpc={} pre_authorized={} total_nodes={}",
                opaque_record_key,
                nodes.len(),
                pre_authorized_valid_count,
                record_state.get_node_transactions_count());
            let required_strict_consensus_count = record_state.required_strict_consensus_count();

            out.push(Arc::new(OutboundTransactCommandParams {
                opaque_record_key,
                safety_selection,
                nodes,
                command: TransactCommand::Commit,
                opt_seqs: None,
                opt_subkey: None,
                opt_value: None,
                required_strict_consensus_count,
                pre_authorized_valid_count,
            }));
        }

        Ok(out)
    }

    /// Record commit transaction
    pub fn record_transact_commit_results(
        &mut self,
        transaction_handle: OutboundTransactionHandle,
        results: Vec<OutboundTransactCommandResult>,
    ) -> VeilidAPIResult<()> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state_mut(transaction_handle)?;

        // Assert stage
        let stage = outbound_transaction_state
            .stage_consensus()
            .ok_or_else(|| VeilidAPIError::generic("transaction not started"))?
            .stage;
        match stage {
            OutboundTransactionStage::End => {}
            OutboundTransactionStage::Begin
            | OutboundTransactionStage::Failed
            | OutboundTransactionStage::Rollback
            | OutboundTransactionStage::Commit => {
                apibail_transaction_not_found!("stage was {:?}, wanted End", stage);
            }
        }

        // For all node transactions where commit commands were not required,
        // transition them directly to the commit state.
        for record_state in outbound_transaction_state.get_record_states_mut() {
            for (_, node_transaction) in record_state.get_node_transactions_mut() {
                // commit_will_change_remote() logs internally when returning false
                if !node_transaction.commit_will_change_remote() {
                    node_transaction.set_stage(OutboundTransactionStage::Commit, None);
                }
            }
        }

        // Record results
        Self::record_transact_command_results(
            outbound_transaction_state,
            results,
            Box::new(
                |node_transaction: &mut NodeTransaction,
                 _: OutboundTransactCommandPerNodeResult| {
                    // Transition to end stage
                    node_transaction.set_stage(OutboundTransactionStage::Commit, None);
                    Ok(())
                },
            ) as OutboundTransactionPerNodeResultHandler,
        )?;

        Ok(())
    }

    /// Prepare to set a value in a transaction
    pub fn prepare_transact_set_params(
        &self,
        transaction_handle: OutboundTransactionHandle,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
        signed_value_data: Arc<SignedValueData>,
    ) -> VeilidAPIResult<Arc<OutboundTransactCommandParams>> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state(transaction_handle)?;

        // Assert stage
        let stage = outbound_transaction_state
            .stage_consensus()
            .ok_or_else(|| VeilidAPIError::generic("transaction not started"))?
            .stage;
        match stage {
            OutboundTransactionStage::Begin => {}
            OutboundTransactionStage::End
            | OutboundTransactionStage::Failed
            | OutboundTransactionStage::Rollback
            | OutboundTransactionStage::Commit => {
                apibail_transaction_not_found!("stage was {:?}, wanted Begin", stage);
            }
        }

        let Some(record_state) = outbound_transaction_state.get_record_state(opaque_record_key)
        else {
            apibail_invalid_argument!(
                "record not in transaction",
                "opaque_record_key",
                opaque_record_key
            );
        };

        // Check if the subkey is in range
        if subkey
            > record_state
                .schema()
                .ok_or_else(|| VeilidAPIError::internal("missing descriptor"))?
                .max_subkey()
        {
            apibail_invalid_argument!("subkey out of range", "subkey", subkey);
        }

        let safety_selection = record_state.safety_selection().clone();
        // Only send Set to Begin-stage nodes; Rollback nodes reject unconditionally.
        let nodes = record_state.get_transact_command_nodes(
            None,
            Some(Box::new(|nt: &NodeTransaction| {
                nt.stage() == OutboundTransactionStage::Begin
            })),
        )?;
        let required_strict_consensus_count = record_state.required_strict_consensus_count();

        Ok(Arc::new(OutboundTransactCommandParams {
            opaque_record_key: opaque_record_key.clone(),
            safety_selection,
            nodes,
            command: TransactCommand::Set,
            opt_seqs: None,
            opt_subkey: Some(subkey),
            opt_value: Some(signed_value_data),
            required_strict_consensus_count,
            pre_authorized_valid_count: 0,
        }))
    }

    /// Record set value in transaction
    pub fn record_transact_set_result(
        &mut self,
        transaction_handle: OutboundTransactionHandle,
        result: OutboundTransactCommandResult,
    ) -> VeilidAPIResult<()> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state_mut(transaction_handle)?;

        // Assert stage
        let stage = outbound_transaction_state
            .stage_consensus()
            .ok_or_else(|| VeilidAPIError::generic("transaction not started"))?
            .stage;
        match stage {
            OutboundTransactionStage::Begin => {}
            OutboundTransactionStage::End
            | OutboundTransactionStage::Failed
            | OutboundTransactionStage::Rollback
            | OutboundTransactionStage::Commit => {
                apibail_transaction_not_found!("stage was {:?}, wanted Begin", stage);
            }
        }

        // Get the record state we're working on
        let Some(record_state) =
            outbound_transaction_state.get_record_state_mut(&result.params.opaque_record_key)
        else {
            apibail_internal!("missing record in set: {}", result.params.opaque_record_key);
        };

        // Set desired subkey to track goal state
        let subkey = result.params.opt_subkey.unwrap_or_log();
        let value = result.params.opt_value.clone().unwrap_or_log();
        record_state.set_desired_subkey(subkey, value.clone());

        // Record set results and calculate the result state consensus
        let mut opt_set_subkey_consensus: Option<SubkeyConsensus> = None;
        let mut found_newer = false;
        let required_strict_consensus_count = record_state.required_strict_consensus_count();

        let set_result_handler = Box::new(
            |node_transaction: &mut NodeTransaction, pnr: OutboundTransactCommandPerNodeResult| {
                // Check if node id transactions reached consensus

                node_transaction.update_expiration(pnr.opt_expiration);

                // Record subkey write
                let opt_value = if let Some(newer_value) = pnr.opt_value {
                    // Something newer was found

                    // (Asserted in decode/validate) Subkey should be present if value is
                    let Some(newer_value_subkey) = pnr.opt_subkey else {
                        apibail_internal!("missing subkey for value");
                    };
                    // (Asserted in decode/validate) Ensure newer subkey matches params
                    if subkey != newer_value_subkey {
                        apibail_internal!("returned subkey does not match parameter");
                    }
                    // (Asserted in decode/validate) Ensure newer value was actually newer or equal
                    if newer_value.value_data().seq() < value.value_data().seq() {
                        apibail_internal!("returned newer value is older than current value");
                    }

                    // Newer value found online
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(node_transaction debug target: "dht", "transact_set: node={} subkey={} NEWER value found online seq={} (our seq={}), write cancelled",
                        node_transaction.rnxid(), subkey,
                        newer_value.value_data().seq(), value.value_data().seq());
                    node_transaction.record_current_subkey_value(subkey, Some(newer_value.clone()));
                    node_transaction.record_updated_subkey_value(subkey, None);

                    let opt_value = Some(newer_value);
                    found_newer = true;
                    opt_value
                } else {
                    // Successful write
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(node_transaction debug target: "dht", "transact_set: node={} subkey={} write accepted seq={}",
                        node_transaction.rnxid(), subkey, value.value_data().seq());
                    node_transaction.record_updated_subkey_value(subkey, Some(value.clone()));

                    Some(value.clone())
                };

                if let Some(set_subkey_state) = &mut opt_set_subkey_consensus {
                    set_subkey_state.add_value(opt_value, required_strict_consensus_count);
                } else {
                    opt_set_subkey_consensus = Some(SubkeyConsensus::new(opt_value));
                }

                Ok(())
            },
        ) as OutboundTransactionPerNodeResultHandler;

        let _ = Self::record_transact_command_per_record_results(
            record_state,
            result,
            set_result_handler,
        )?;

        // Record the subkey consensus results
        if let Some(set_subkey_consensus) = opt_set_subkey_consensus {
            if found_newer {
                // Add found newer value to current subkey consensus
                record_state
                    .current_consensus_mut()
                    .record(subkey, Some(set_subkey_consensus));
                // Remove updated subkey consensus
                record_state.updated_consensus_mut().record(subkey, None);
            } else {
                // Add set value to updated subkey consensus
                record_state
                    .updated_consensus_mut()
                    .record(subkey, Some(set_subkey_consensus));
            }
        } else {
            // If no consensus was reached, we eliminate the consensus records for this subkey
            // and return a try again error
            #[cfg(feature = "verbose-tracing")]
            veilid_log!(record_state debug target: "dht", "transact_set: subkey={}:{} NO CONSENSUS reached (found_newer={}) → TryAgain",
                record_state.record_key().opaque(), subkey, found_newer);
            record_state.updated_consensus_mut().record(subkey, None);
            record_state.current_consensus_mut().record(subkey, None);
            apibail_try_again!(
                "set did not reach consensus for subkey: {}:{}",
                record_state.record_key().opaque(),
                subkey
            );
        }

        Ok(())
    }

    /// Prepare to get a value in a transaction
    pub fn prepare_transact_get_params(
        &self,
        transaction_handle: OutboundTransactionHandle,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
    ) -> VeilidAPIResult<Arc<OutboundTransactCommandParams>> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state(transaction_handle)?;

        // Assert stage
        let stage = outbound_transaction_state
            .stage_consensus()
            .ok_or_else(|| VeilidAPIError::generic("transaction not started"))?
            .stage;
        match stage {
            OutboundTransactionStage::Begin => {}
            OutboundTransactionStage::End
            | OutboundTransactionStage::Failed
            | OutboundTransactionStage::Rollback
            | OutboundTransactionStage::Commit => {
                apibail_transaction_not_found!("stage was {:?}, wanted Begin", stage);
            }
        }

        let Some(record_state) = outbound_transaction_state.get_record_state(opaque_record_key)
        else {
            apibail_invalid_argument!(
                "record not in transaction",
                "opaque_record_key",
                opaque_record_key
            );
        };

        // Check if the subkey is in range
        if subkey
            > record_state
                .schema()
                .ok_or_else(|| VeilidAPIError::internal("missing descriptor"))?
                .max_subkey()
        {
            apibail_invalid_argument!("subkey out of range", "subkey", subkey);
        }

        let safety_selection = record_state.safety_selection().clone();
        let nodes = record_state.get_transact_command_nodes(None, None)?;
        // Use get_value_count as GET consensus threshold so a single non-responding node
        // won't force a costly retry that exceeds the outer timeout. (get_value_count <= set_value_count by config.)
        let required_strict_consensus_count =
            self.config().internal().network.dht.get_value_count as usize;

        Ok(Arc::new(OutboundTransactCommandParams {
            opaque_record_key: opaque_record_key.clone(),
            safety_selection,
            nodes,
            command: TransactCommand::Get,
            opt_seqs: None,
            opt_subkey: Some(subkey),
            opt_value: None,
            required_strict_consensus_count,
            pre_authorized_valid_count: 0,
        }))
    }

    /// Record get value in transaction
    pub fn record_transact_get_result(
        &mut self,
        transaction_handle: OutboundTransactionHandle,
        result: OutboundTransactCommandResult,
    ) -> VeilidAPIResult<()> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state_mut(transaction_handle)?;

        // Assert stage
        let stage = outbound_transaction_state
            .stage_consensus()
            .ok_or_else(|| VeilidAPIError::generic("transaction not started"))?
            .stage;
        match stage {
            OutboundTransactionStage::Begin => {}
            OutboundTransactionStage::End
            | OutboundTransactionStage::Failed
            | OutboundTransactionStage::Rollback
            | OutboundTransactionStage::Commit => {
                apibail_transaction_not_found!("stage was {:?}, wanted Begin", stage);
            }
        }

        // Check if node id transactions reached consensus
        let Some(record_state) =
            outbound_transaction_state.get_record_state_mut(&result.params.opaque_record_key)
        else {
            apibail_internal!("missing record in get: {}", result.params.opaque_record_key);
        };

        // Record get results and calculate the result state consensus
        let subkey = result.params.opt_subkey.unwrap_or_log();

        // Calculate the result state consensus
        let mut opt_get_subkey_consensus: Option<SubkeyConsensus> = None;
        let required_strict_consensus_count = record_state.required_strict_consensus_count();

        let get_result_handler = Box::new(
            |node_transaction: &mut NodeTransaction, pnr: OutboundTransactCommandPerNodeResult| {
                // Record subkey get for this node transaction
                let opt_value = pnr.opt_value;
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(node_transaction debug target: "dht", "transact_get: node={} subkey={} returned seq={}",
                    node_transaction.rnxid(), subkey,
                    opt_value.as_ref().map(|v| v.value_data().seq().to_string()).unwrap_or_else(|| "None".to_string()));
                node_transaction.record_current_subkey_value(subkey, opt_value.clone());
                node_transaction.update_expiration(pnr.opt_expiration);

                if let Some(get_subkey_state) = &mut opt_get_subkey_consensus {
                    get_subkey_state.add_value(opt_value, required_strict_consensus_count);
                } else {
                    opt_get_subkey_consensus = Some(SubkeyConsensus::new(opt_value));
                }
                Ok(())
            },
        ) as OutboundTransactionPerNodeResultHandler;

        let _ = Self::record_transact_command_per_record_results(
            record_state,
            result,
            get_result_handler,
        )?;

        // Record the subkey consensus results
        record_state
            .current_consensus_mut()
            .record(subkey, opt_get_subkey_consensus);

        Ok(())
    }

    /// Record a keepalive result
    pub fn record_transact_keepalive_result(
        &mut self,
        lnxid: LocalNodeTransactionId,
        expiration: Timestamp,
        opt_rtt: Option<TimestampDuration>,
    ) -> VeilidAPIResult<bool> {
        #[cfg(feature = "verbose-tracing")]
        let registry = self.registry();

        // Resolve the transaction handle from the local node transaction id mapping
        let Some(mapping) = self.local_node_transaction_mappings.get(&lnxid).cloned() else {
            // Node transaction was dropped from transaction
            return Ok(false);
        };
        let transaction_handle = mapping.transaction_handle;

        // Get the record state for this node transaction
        let Some(record_state) =
            self.get_transaction_record_state_mut_by_lnxid(transaction_handle, lnxid)
        else {
            // Record was removed from transaction
            apibail_internal!(
                "record state not found for node transaction {} in transaction {}",
                lnxid,
                transaction_handle
            );
        };

        // Get the node transaction for this node transaction id
        let Some(nt) = record_state.get_node_transaction_mut(lnxid) else {
            apibail_internal!(
                "node transaction {} not found for record state {} in transaction {}",
                lnxid,
                record_state.record_key().opaque(),
                transaction_handle
            );
        };

        // Refresh the per-node RTT from this successful keepalive
        if let Some(rtt) = opt_rtt {
            nt.record_rtt(rtt);
        }

        // Update expiration only if we're still in Begin or End stage
        if matches!(
            nt.stage(),
            OutboundTransactionStage::Begin | OutboundTransactionStage::End
        ) {
            #[cfg(feature = "verbose-tracing")]
            veilid_log!(registry debug target: "dht", "Keepalive updated expiration: transaction_handle={} record={} lnxid={} rnxid={} expiration={:#}", mapping.transaction_handle, mapping.opaque_record_key, lnxid, nt.rnxid(), expiration);

            nt.update_expiration(Some(expiration));
        } else {
            #[cfg(feature = "verbose-tracing")]
            veilid_log!(registry debug target: "dht", "Keepalive update ignored: stage={} transaction_handle={} record={} lnxid={} rnxid={} expiration={:#}", nt.stage(), mapping.transaction_handle, mapping.opaque_record_key, lnxid, nt.rnxid(), expiration);
        }

        Ok(matches!(nt.stage(), OutboundTransactionStage::Begin))
    }

    /// Get an inspection report for a transaction
    /// Count begin-set nodes whose Begin answer reported this subkey at or above min_seq
    pub fn count_begin_holders(
        &self,
        transaction_handle: OutboundTransactionHandle,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
        min_seq: ValueSeqNum,
    ) -> VeilidAPIResult<usize> {
        let outbound_transaction_state = self.get_transaction_state(transaction_handle)?;
        let Some(record_state) = outbound_transaction_state.get_record_state(opaque_record_key)
        else {
            apibail_invalid_argument!(
                "record not in transaction",
                "opaque_record_key",
                opaque_record_key
            );
        };
        Ok(record_state.count_begin_holders(subkey, min_seq))
    }

    pub fn get_record_report(
        &self,
        transaction_handle: OutboundTransactionHandle,
        opaque_record_key: &OpaqueRecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
        scope: DHTReportScope,
    ) -> VeilidAPIResult<DHTRecordReport> {
        // Get transaction
        let outbound_transaction_state = self.get_transaction_state(transaction_handle)?;

        // Assert stage
        let stage = outbound_transaction_state
            .stage_consensus()
            .ok_or_else(|| VeilidAPIError::generic("transaction not started"))?
            .stage;
        match stage {
            OutboundTransactionStage::Begin => {}
            OutboundTransactionStage::End
            | OutboundTransactionStage::Failed
            | OutboundTransactionStage::Rollback
            | OutboundTransactionStage::Commit => {
                apibail_transaction_not_found!("stage was {:?}, wanted Begin", stage);
            }
        }

        let Some(record_state) = outbound_transaction_state.get_record_state(opaque_record_key)
        else {
            apibail_invalid_argument!(
                "record not in transaction",
                "opaque_record_key",
                opaque_record_key
            );
        };

        let Some(schema) = record_state.schema() else {
            apibail_internal!("no schema for transaction");
        };

        let subkeys = ValueSubkeyRangeSet::single_range(0, schema.max_subkey())
            .intersect(&subkeys.unwrap_or_else(ValueSubkeyRangeSet::full));

        let local_snapshot = record_state.local_snapshot();
        let mut local_seqs = Vec::with_capacity(subkeys.len() as usize);
        let mut network_seqs = Vec::with_capacity(subkeys.len() as usize);
        for subkey in subkeys.iter() {
            let mut local_seq = local_snapshot.seq(subkey)?;

            match scope {
                DHTReportScope::Local => {
                    local_seqs.push(local_seq);
                    network_seqs.push(ValueSeqNum::NONE);
                }
                DHTReportScope::SyncGet | DHTReportScope::SyncSet => {
                    local_seqs.push(local_seq);
                    network_seqs.push(record_state.begin_network_seq(subkey)?);
                }
                DHTReportScope::UpdateGet => {
                    let network_seq = record_state.begin_network_seq(subkey)?;
                    local_seqs.push(ValueSeqNum::max(local_seq, network_seq));
                    network_seqs.push(network_seq);
                }
                DHTReportScope::UpdateSet => {
                    let network_seq = record_state.begin_network_seq(subkey)?;
                    local_seq = local_seq.next()?;
                    local_seqs.push(local_seq);
                    network_seqs.push(ValueSeqNum::max(local_seq, network_seq));
                }
            }
        }

        DHTRecordReport::new(
            subkeys,
            // Transactions never have offline subkeys
            ValueSubkeyRangeSet::new(),
            local_seqs,
            network_seqs,
        )
    }
}
