use futures_util::future::MaybeDone;

use super::*;

/// Durability signal for the outbound transaction check task.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(in crate::storage_manager) enum DurabilityExpiration {
    /// Durability already broken; drop now.
    Lost,
    /// At least required_strict_consensus_count nodes alive on every record until this timestamp.
    AliveUntil(Timestamp),
    /// No keepalive-active nodes. Don't drop on this signal.
    NotApplicable,
}

/// Stage consensus for transaction state across all records
#[derive(Clone, Debug)]
pub(in crate::storage_manager) struct OutboundTransactionStageConsensus {
    /// The best consensus stage we could come up with for this transaction
    pub stage: OutboundTransactionStage,
    /// The list of node transactions that should be dropped at this point
    pub node_transactions_to_drop: LocalNodeTransactionIdSet,
    /// The list of node transactions that should be rolled back at this point per record
    pub node_transactions_to_rollback: LocalNodeTransactionIdSet,
}

/// Whether the application asked for a transaction or an internal task did
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::storage_manager) enum OutboundTransactionKind {
    /// Requested through the public API; pre-empts background transactions
    #[default]
    Foreground,
    /// Started by internal tasks (rehydration, change inspection); yields to foreground
    Background,
}

/// State of a single transaction across multiple records
#[derive(Debug, Serialize, Deserialize)]
pub(in crate::storage_manager) struct OutboundTransactionState {
    /// Registry for logging
    #[serde(skip)]
    opt_registry: Option<VeilidComponentRegistry>,
    /// The timestamp of when the transaction was created
    created_ts: Timestamp,
    /// Foreground or background origin, for pre-emption
    #[serde(default)]
    kind: OutboundTransactionKind,
    /// State per record
    record_states: Vec<OutboundTransactionRecordState>,
    /// Background operations to join at drop
    #[serde(skip)]
    background_tokens: Vec<MaybeDone<StopToken>>,
    /// Refs to routes used during this transaction, held to keep them locked
    #[serde(skip)]
    route_refs: Vec<AllocatedRouteRef>,
}

impl fmt::Display for OutboundTransactionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            r#"created@{} {}@{}
record_infos:
{}
"#,
            f.to_string(self.created_ts),
            self.stage_consensus()
                .map(|x| x.stage.to_string())
                .unwrap_or_else(|| "INIT".to_string()),
            self.stage_ts(),
            self.record_states
                .iter()
                .enumerate()
                .map(|x| indent_all_string(format!("{}: {}", x.0, f.to_string(x.1))))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

impl VeilidComponentRegistryAccessor for OutboundTransactionState {
    fn registry(&self) -> VeilidComponentRegistry {
        self.opt_registry.clone().unwrap_or_log()
    }
}

impl Drop for OutboundTransactionState {
    fn drop(&mut self) {
        if self.opt_registry.is_none() {
            eprintln!("BUG: OutboundTransactionState dropped without registry");
            return;
        }
        if !self.route_refs.is_empty() {
            veilid_log!(self error "OutboundTransactionState dropped with {} locked safety routes still held", self.route_refs.len());
        }
        let active_bg = self
            .background_tokens
            .iter()
            .filter(|x| matches!(x, MaybeDone::Future(_)))
            .count();
        if active_bg > 0 {
            veilid_log!(self error "OutboundTransactionState dropped with {} active background tokens", active_bg);
        }
    }
}

impl OutboundTransactionState {
    pub(super) fn new(registry: VeilidComponentRegistry, kind: OutboundTransactionKind) -> Self {
        Self {
            opt_registry: Some(registry),
            created_ts: Timestamp::now(),
            kind,
            record_states: vec![],
            background_tokens: vec![],
            route_refs: Vec::new(),
        }
    }

    pub fn kind(&self) -> OutboundTransactionKind {
        self.kind
    }

    pub(super) fn prepare(&mut self, routing_table: &RoutingTable, cur_ts: Timestamp) {
        self.opt_registry = Some(routing_table.registry());
        for record_info in &mut self.record_states {
            record_info.prepare(routing_table, cur_ts);
        }
    }

    pub fn keys(&self) -> Vec<OpaqueRecordKey> {
        let mut keys = self
            .record_states
            .iter()
            .map(|x| x.record_key().opaque())
            .collect::<Vec<_>>();
        keys.sort_unstable();
        keys
    }

    pub(super) fn merge(&mut self, mut other: OutboundTransactionState) {
        self.created_ts.min_assign(other.created_ts);
        // Foreground wins so a merged transaction is never pre-empted
        if other.kind == OutboundTransactionKind::Foreground {
            self.kind = OutboundTransactionKind::Foreground;
        }
        self.record_states.append(&mut other.record_states);
        self.background_tokens.append(&mut other.background_tokens);
        self.route_refs.append(&mut other.route_refs);
    }

    #[expect(dead_code)]
    pub fn created_ts(&self) -> Timestamp {
        self.created_ts
    }

    pub fn durability_expiration(&self) -> DurabilityExpiration {
        // Across records: Lost wins; otherwise MIN of AliveUntil values;
        // NotApplicable only if every record is NotApplicable.
        let mut earliest: Option<Timestamp> = None;
        let mut any_applicable = false;
        for record_state in &self.record_states {
            match record_state.durability_expiration() {
                DurabilityExpiration::Lost => return DurabilityExpiration::Lost,
                DurabilityExpiration::AliveUntil(t) => {
                    any_applicable = true;
                    earliest = Some(match earliest {
                        Some(prev) => prev.min(t),
                        None => t,
                    });
                }
                DurabilityExpiration::NotApplicable => {}
            }
        }
        match (any_applicable, earliest) {
            (true, Some(t)) => DurabilityExpiration::AliveUntil(t),
            _ => DurabilityExpiration::NotApplicable,
        }
    }

    pub fn stage_consensus(&self) -> Option<OutboundTransactionStageConsensus> {
        // All record stages must be the same or this is a failed state
        let mut opt_best_opt_stage: Option<Option<OutboundTransactionStage>> = None;
        let mut node_transactions_to_rollback: LocalNodeTransactionIdSet =
            LocalNodeTransactionIdSet::new();
        let mut node_transactions_to_drop: LocalNodeTransactionIdSet =
            LocalNodeTransactionIdSet::new();
        let mut force_fail = false;

        for record_state in &self.record_states {
            let Some(record_stage_consensus) = record_state.stage_consensus() else {
                // If some record has no stage consensus yet (INIT), all records must be INIT
                if let Some(best_opt_stage) = opt_best_opt_stage {
                    // Other record found that was not INIT
                    if best_opt_stage.is_some() {
                        force_fail = true;
                        break;
                    }
                } else {
                    opt_best_opt_stage = Some(None);
                }
                continue;
            };

            // If we have a record stage consensus, it must match all the other records
            let record_stage = record_stage_consensus.stage;
            if let Some(best_opt_stage) = opt_best_opt_stage {
                if best_opt_stage != Some(record_stage) {
                    force_fail = true;
                    break;
                }
            } else {
                opt_best_opt_stage = Some(Some(record_stage));
            }
            node_transactions_to_rollback
                .extend(record_stage_consensus.node_transactions_to_rollback);
            node_transactions_to_drop.extend(record_stage_consensus.node_transactions_to_drop);
        }

        // If we are forcing a failed state, sum up the rollbacks instead
        if force_fail {
            let node_transactions_to_rollback = self.get_all_rollbacks();
            return Some(OutboundTransactionStageConsensus {
                stage: OutboundTransactionStage::Failed,
                node_transactions_to_rollback,
                node_transactions_to_drop,
            });
        }

        let Some(best_opt_stage) = opt_best_opt_stage else {
            // No records means INIT stage
            return None;
        };
        let Some(stage) = best_opt_stage else {
            // All INIT means INIT stage
            return None;
        };

        // Return the summed up transaction stage consensus
        // and all of the actions to perform for reconciliation
        Some(OutboundTransactionStageConsensus {
            stage,
            node_transactions_to_rollback,
            node_transactions_to_drop,
        })
    }

    pub fn get_all_rollbacks(&self) -> LocalNodeTransactionIdSet {
        self.record_states
            .iter()
            .flat_map(|x| x.get_all_rollbacks())
            .collect()
    }

    pub fn stage_ts(&self) -> Timestamp {
        self.record_states
            .iter()
            .map(|x| x.stage_ts())
            .reduce(|a, b| a.max(b))
            .unwrap_or(self.created_ts)
    }

    pub(super) fn new_record_state(
        &mut self,
        params: OutboundTransactionRecordParams,
    ) -> VeilidAPIResult<()> {
        let opaque_record_key = params.record_key.opaque();
        if self.get_record_state(&opaque_record_key).is_some() {
            apibail_internal!("record info already exists");
        }

        self.record_states
            .push(OutboundTransactionRecordState::new(self.registry(), params));

        Ok(())
    }

    pub fn get_record_states(&self) -> &[OutboundTransactionRecordState] {
        &self.record_states
    }

    pub(super) fn get_record_states_mut(&mut self) -> &mut [OutboundTransactionRecordState] {
        &mut self.record_states
    }

    pub fn get_record_state(
        &self,
        opaque_record_key: &OpaqueRecordKey,
    ) -> Option<&OutboundTransactionRecordState> {
        self.record_states
            .iter()
            .find(|ri| &ri.record_key().opaque() == opaque_record_key)
    }

    pub(super) fn get_record_state_mut(
        &mut self,
        opaque_record_key: &OpaqueRecordKey,
    ) -> Option<&mut OutboundTransactionRecordState> {
        self.record_states
            .iter_mut()
            .find(|ri| &ri.record_key().opaque() == opaque_record_key)
    }

    pub(super) fn add_background_token(&mut self, background_token: StopToken) {
        self.background_tokens
            .push(futures_util::future::maybe_done(background_token));
    }

    pub(super) fn remove_completed_background_tokens(&mut self) {
        self.background_tokens.retain(|x| match x {
            MaybeDone::Future(_) => true,
            MaybeDone::Done(_) | MaybeDone::Gone => false,
        });
    }

    pub fn into_transaction_cleanup(mut self) -> TransactionCleanup {
        let background_tokens = std::mem::take(&mut self.background_tokens)
            .into_iter()
            .filter_map(|x| match x {
                MaybeDone::Future(fut) => Some(fut),
                MaybeDone::Done(_) | MaybeDone::Gone => None,
            })
            .collect();

        drop(std::mem::take(&mut self.route_refs));

        TransactionCleanup::new(background_tokens)
    }

    pub(super) fn add_route_refs(&mut self, route_refs: Vec<AllocatedRouteRef>) {
        self.route_refs.extend(route_refs);
    }
}
