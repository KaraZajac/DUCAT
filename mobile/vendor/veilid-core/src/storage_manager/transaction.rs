use futures_util::StreamExt as _;

use super::*;

/// Maximum number of records per transaction
const MAX_RECORDS_PER_TRANSACTION: usize = 32;

impl_veilid_log_facility!("stor");

/// Source for resolving each record's transaction signing keypair during begin.
pub(super) enum TransactionSignerSource {
    /// Derive from the opened record's writer, the options default, or anonymous.
    OpenedRecord,
    /// Each member's stored operation signer + Peek locks, held only until signers are extracted.
    Stored(
        BTreeMap<OpaqueRecordKey, KeyPair>,
        StorageManagerPeeksLockGuard,
    ),
}

impl StorageManager {
    /// Create a new outbound transaction over a set of records
    /// If an existing transaction exists over these records
    /// or a transaction can not be performed at this time, this will fail.
    /// Returns a transaction handle if the transaction was created
    /// Returns Err(VeilidAPIError::TryAgain) if the transaction could not be created
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(self), ret)
    )]
    pub async fn begin_transaction(
        &self,
        record_keys: Vec<RecordKey>,
        options: Option<TransactDHTRecordsOptions>,
    ) -> VeilidAPIResult<OutboundTransactionHandle> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        self.begin_transaction_shared(
            record_keys,
            options.unwrap_or_default(),
            TransactionSignerSource::OpenedRecord,
        )
        .await
    }

    /// Shared core of begin_transaction used by both the public API and background transactions.
    /// `signer_source` controls how each record's transaction signing keypair is resolved, and for
    /// background transactions carries the Peek locks (released here once the TransactBegin lock is
    /// held). The caller must hold the startup lock; background callers must have all records open.
    pub(super) async fn begin_transaction_shared(
        &self,
        record_keys: Vec<RecordKey>,
        options: TransactDHTRecordsOptions,
        signer_source: TransactionSignerSource,
    ) -> VeilidAPIResult<OutboundTransactionHandle> {
        // Early rejection if no records are being transacted over
        if record_keys.is_empty() {
            apibail_missing_argument!(
                "begin_transaction requires one or more records",
                "record_keys"
            );
        }

        // Enforce record limit
        if record_keys.len() > MAX_RECORDS_PER_TRANSACTION {
            apibail_invalid_argument!(
                format!(
                    "begin_transaction has more than {} records",
                    MAX_RECORDS_PER_TRANSACTION
                ),
                "record_keys",
                record_keys.len()
            );
        }

        // Early rejection if there are duplicate records
        if record_keys.has_duplicates() {
            apibail_missing_argument!(
                "transaction can not have duplicate record keys",
                "record_keys"
            );
        }

        // Foreground begins pre-empt background transactions; background begins
        // yield on any contention
        let kind = match &signer_source {
            TransactionSignerSource::OpenedRecord => OutboundTransactionKind::Foreground,
            TransactionSignerSource::Stored(..) => OutboundTransactionKind::Background,
        };
        // Touched even on failed attempts so app retry gaps stay protected
        if kind == OutboundTransactionKind::Foreground {
            self.touch_foreground_transaction_activity();
        }
        let opaque_record_keys = record_keys.iter().map(|x| x.opaque()).collect::<Vec<_>>();

        let records_lock = match kind {
            OutboundTransactionKind::Background => {
                // Non-blocking acquire: any contention on the Transaction-mode lock
                // for any of these records returns TryAgain rather than waiting.
                // Prevents cross-record deadlocks that could otherwise form if the
                // caller racily issues overlapping begin_transaction calls.
                let Some(records_lock) = self.record_lock_table.try_lock_records(
                    opaque_record_keys.clone(),
                    StorageManagerRecordLockPurpose::TransactBegin,
                ) else {
                    veilid_log!(self debug target:"network_result", "transaction begin contended on records: [{}] (source=background)",
                        opaque_record_keys.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "));
                    apibail_try_again!("transaction begin contended");
                };
                records_lock
            }
            OutboundTransactionKind::Foreground => {
                // Blocking acquire is deadlock-safe (lock_records sorts) and bounded
                // by the per-operation lock hold of whatever it waits on
                let records_lock = self
                    .record_lock_table
                    .lock_records(
                        opaque_record_keys.clone(),
                        StorageManagerRecordLockPurpose::TransactBegin,
                    )
                    .await;
                self.preempt_background_transactions_locked(&records_lock, &opaque_record_keys)
                    .await?;
                records_lock
            }
        };
        let records_lock = Arc::new(records_lock);

        // TransactBegin now covers the set; extract signers and release Stored's Peek locks here.
        let stored_signers = match signer_source {
            TransactionSignerSource::OpenedRecord => None,
            TransactionSignerSource::Stored(signers, _peek_locks) => Some(signers),
        };

        // Early rejection if dht is not online
        if !self.dht_is_online() {
            apibail_try_again!("dht is not online");
        }

        // Resolve config
        let rpc_timeout =
            TimestampDuration::new_ms(self.config().internal().network.rpc.timeout_ms.into());
        let consensus_width = self.config().internal().network.dht.consensus_width as usize;
        let required_strict_consensus_count =
            self.config().internal().network.dht.set_value_count as usize;
        let required_get_consensus_count =
            self.config().internal().network.dht.get_value_count as usize;

        // Snapshot records for begin
        let local_snapshots = self
            .save_local_snapshots(record_keys.iter().map(|x| x.opaque()).collect())
            .await?;

        // Get opened records and construct record states
        let (transaction_handle, begin_params_list) = {
            let mut inner = self.inner.lock();

            let mut record_params_list = vec![];
            for record_key in record_keys {
                let opaque_record_key = record_key.opaque();

                // Can't begin a transaction on a record with pending offline subkey writes
                if inner.offline_subkey_writes.contains_key(&opaque_record_key) {
                    apibail_try_again!("record has pending offline writes, try again later");
                }

                let Some(opened_record) = inner.opened_records.get(&opaque_record_key) else {
                    apibail_invalid_argument!("record not open", "record_key", opaque_record_key);
                };
                if record_key.encryption_key().map(|x| x.value()) != opened_record.encryption_key()
                {
                    apibail_generic!(
                        "record encryption key does not match opened record encryption key: {}",
                        opaque_record_key
                    );
                }

                // Get signing keypair for this transaction
                let signing_keypair = match &stored_signers {
                    None => opened_record
                        .writer()
                        .cloned()
                        .or_else(|| options.default_signing_keypair.clone())
                        .unwrap_or_else(|| {
                            self.anonymous_signing_keys
                                .get(opaque_record_key.kind())
                                .unwrap_or_log()
                        }),
                    Some(signers) => {
                        let Some(kp) = signers.get(&opaque_record_key) else {
                            apibail_internal!(
                                "missing stored operation signer for background transaction: {}",
                                opaque_record_key
                            );
                        };
                        kp.clone()
                    }
                };

                // Get safety selection for this record
                let safety_selection = opened_record.safety_selection();

                // Take local snapshot of this record
                let local_snapshot = local_snapshots
                    .get(&opaque_record_key)
                    .cloned()
                    .ok_or_else(|| VeilidAPIError::internal("missing local snapshot"))?;

                // Add parameters for this record
                record_params_list.push(OutboundTransactionRecordParams {
                    record_key,
                    signing_keypair,
                    required_strict_consensus_count,
                    required_get_consensus_count,
                    safety_selection,
                    local_snapshot,
                });
            }

            // Obtain the outbound transaction manager
            let otm = &mut inner.outbound_transaction_manager;

            // Start a new transaction
            let transaction_handle = otm.new_transaction(record_params_list, kind)?;

            // Get parameters for beginning a transaction
            let begin_params_list = match otm.prepare_transact_begin_params(transaction_handle) {
                Ok(v) => v,
                Err(e) => {
                    veilid_log!(self debug "error in prepare_transact_begin_params: {}", e);

                    // Drop the transaction and ignore the result because there can't be any background tokens yet
                    let _ = otm.drop_transaction(transaction_handle);

                    return Err(e);
                }
            };

            (transaction_handle, begin_params_list)
        };

        let (opt_cleanup, transaction_handle) = self
            .rollback_guard_locked(records_lock.clone(), transaction_handle, async {
                let mut opt_begin_error: Option<VeilidAPIError> = None;
                let mut unord = FuturesUnordered::new();

                // Send outbound begin transactions on pending records
                for begin_params in begin_params_list {
                    let registry = self.registry();
                    let fut = async move {
                        let this = registry.storage_manager();
                        this.outbound_transact_begin(begin_params)
                            .measure_debug(
                                rpc_timeout,
                                veilid_log_dbg!(
                                    this,
                                    "StorageManager::begin_transaction outbound_transact_begin"
                                ),
                            )
                            .await
                    };
                    #[cfg(feature = "instrument")]
                    let fut = fut.instrument(tracing::trace_span!(
                        target: "dht",
                        "begin_transaction per-record outbound"
                    ));
                    unord.push(Box::pin(fut));
                }

                let mut begin_results = vec![];
                while let Some(res) = unord.next().await {
                    match res {
                        Ok(result) => {
                            // Process fanout results for cache regardless of consensus
                            let subkey_count = result.descriptor.schema()?.subkey_count();
                            if result.seqs.len() != subkey_count
                                && !result.fanout_result.value_nodes.is_empty()
                            {
                                apibail_internal!(
                                    "seqs returned does not match subkey count: {} != {}: {:?}",
                                    result.seqs.len(),
                                    subkey_count,
                                    result
                                );
                            }
                            let max_subkey = result.descriptor.schema()?.max_subkey();

                            let existed = self.process_fanout_results(
                                result.params.record_params.record_key.opaque(),
                                core::iter::once((
                                    ValueSubkeyRangeSet::single_range(0, max_subkey),
                                    result.fanout_result.clone(),
                                )),
                                false,
                                consensus_width,
                            )?;
                            if !existed {
                                apibail_internal!(
                                    "Record went missing during transaction despite lock: {}",
                                    result.params.record_params.record_key.opaque()
                                );
                            }

                            begin_results.push(result);
                        }
                        Err(e) => {
                            veilid_log!(self debug "error in outbound_transact_begin: {}", e);
                            if opt_begin_error.is_none() {
                                opt_begin_error = Some(e);
                            }
                        }
                    }
                }

                if let Err(e) = self
                    .inner
                    .lock()
                    .outbound_transaction_manager
                    .record_transact_begin_results(begin_results)
                {
                    veilid_log!(self debug "error in record_transact_begin_results: {}", e);
                    if opt_begin_error.is_none() {
                        opt_begin_error = Some(e);
                    }
                }

                // Rollback if any errors happened
                if let Some(begin_error) = opt_begin_error {
                    return Err(begin_error);
                }

                // Otherwise return handle
                Ok(transaction_handle)
            })
            .await?;

        // Cleanup runs in the background processor.
        if let Some(cleanup) = opt_cleanup {
            self.background_operation_processor
                .add_future(cleanup.into_future());
        }
        drop(records_lock);

        Ok(transaction_handle)
    }

    /// Roll back and drop any background transactions registered on these records so a
    /// foreground begin can take them. Waiting out the rollback commands under the held lock
    /// keeps the re-begin from reaching nodes that still hold the old node-transactions.
    /// A doomed transaction's records outside this set are freed before their nodes confirm;
    /// begins landing there see busy nodes and TryAgain.
    async fn preempt_background_transactions_locked(
        &self,
        records_lock: &StorageManagerRecordsLockGuard,
        opaque_record_keys: &[OpaqueRecordKey],
    ) -> VeilidAPIResult<()> {
        let preempt_handles = {
            let inner = self.inner.lock();
            let otm = &inner.outbound_transaction_manager;
            let mut preempt_handles = vec![];
            for opaque_record_key in opaque_record_keys {
                let Some(transaction_handle) = otm.get_transaction_by_record(opaque_record_key)
                else {
                    continue;
                };
                match otm.get_transaction_kind(transaction_handle) {
                    Some(OutboundTransactionKind::Background) => {
                        if !preempt_handles.contains(&transaction_handle) {
                            preempt_handles.push(transaction_handle);
                        }
                    }
                    Some(OutboundTransactionKind::Foreground) => {
                        veilid_log!(self debug target:"network_result", "transaction begin contended on record {} (foreground holder)", opaque_record_key);
                        apibail_try_again!("transaction begin contended");
                    }
                    None => {}
                }
            }
            preempt_handles
        };

        for transaction_handle in preempt_handles {
            veilid_log!(self debug "pre-empting background transaction for foreground begin: {}", transaction_handle);
            if let Err(e) = self
                .rollback_transaction_locked(records_lock, transaction_handle)
                .await
            {
                veilid_log!(self debug "pre-empted transaction rollback failed: {}", e);
            }
            self.drop_transaction_and_wait_locked(records_lock, transaction_handle)
                .await;
        }
        Ok(())
    }

    /// Run a closure inside a background transaction over a record's stored transaction set.
    ///
    /// Peek-locks the whole set (blocking open/close/delete while still allowing transaction
    /// locks), temporarily opens any members that are not currently open (without a writer key),
    /// begins a transaction signed with each member's stored operation signer, and runs `f` with
    /// the transaction handle and the member record keys (in stored set order). On return the
    /// transaction is rolled back if the closure left it active, temporarily-opened members are
    /// closed, and the peek locks are released.
    ///
    /// The set is all-or-nothing: contention returns TryAgain; a member with no local record or no
    /// stored operation signer (a broken set) returns a terminal error and runs nothing.
    pub(super) async fn with_background_transaction<T, F, Fut>(
        &self,
        triggering_record_key: RecordKey,
        membership: TransactionMembership,
        f: F,
    ) -> VeilidAPIResult<T>
    where
        F: FnOnce(OutboundTransactionHandle, Vec<RecordKey>) -> Fut,
        Fut: Future<Output = VeilidAPIResult<T>>,
    {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        // Yield to foreground transaction activity
        if !self.background_transactions_allowed() {
            apibail_try_again!("background transaction deferred for foreground activity");
        }

        let local_record_store = self.get_local_record_store()?;

        // Peek-lock the set so members can't be opened/closed/deleted while we resolve and begin;
        // handed to begin_transaction_shared, which releases them once TransactBegin is held.
        let Some(peek_locks) = self
            .record_lock_table
            .try_peek_locks(membership.record_set.clone())
        else {
            apibail_try_again!("background transaction set peek contended");
        };

        // Safety selection for temporarily-opened members comes from the triggering record.
        let triggering_opaque = triggering_record_key.opaque();

        // Resolve every member, temp-opening the unopened ones, all under the inner lock.
        let (member_record_keys, signers, temp_opened) = {
            let mut inner = self.inner.lock();

            let triggering_safety_selection =
                if let Some(or) = inner.opened_records.get(&triggering_opaque) {
                    or.safety_selection()
                } else {
                    local_record_store
                        .with_record(&triggering_opaque, |r| r.detail().safety_selection.clone())?
                        .ok_or_else(|| {
                            VeilidAPIError::generic(
                                "triggering record missing for background transaction",
                            )
                        })?
                };

            let mut member_record_keys = vec![];
            let mut signers = BTreeMap::<OpaqueRecordKey, KeyPair>::new();
            let mut temp_opened = vec![];

            for opaque in &membership.record_set {
                // Disallow if any member already participates in an active transaction
                if inner
                    .outbound_transaction_manager
                    .get_transaction_by_record(opaque)
                    .is_some()
                {
                    apibail_try_again!(
                        "background transaction member already in a transaction: {}",
                        opaque
                    );
                }

                // The member must have a local record and a stored operation signer
                let opt_signer = local_record_store.with_record(opaque, |r| {
                    r.detail()
                        .transaction_membership
                        .as_ref()
                        .map(|m| m.operation_signer.clone())
                })?;
                let Some(opt_signer) = opt_signer else {
                    apibail_generic!(
                        "background transaction member has no local record: {}",
                        opaque
                    );
                };
                let Some(signer) = opt_signer else {
                    apibail_generic!(
                        "background transaction member has no stored operation signer: {}",
                        opaque
                    );
                };
                signers.insert(opaque.clone(), signer);

                // Members already open keep their open state and encryption key; the rest are
                // temporarily opened without a writer using the triggering record's safety.
                let record_key = if let Some(or) = inner.opened_records.get(opaque) {
                    RecordKey::from_opaque(opaque.clone(), or.encryption_key())
                } else {
                    inner.opened_records.insert(
                        opaque.clone(),
                        OpenedRecord::new(None, triggering_safety_selection.clone(), None),
                    );
                    temp_opened.push(opaque.clone());
                    RecordKey::from_opaque(opaque.clone(), None)
                };
                member_record_keys.push(record_key);
            }

            (member_record_keys, signers, temp_opened)
        };

        // Begin, run the closure, and ensure the transaction is gone, all behind a teardown.
        let result = async {
            let transaction_handle = self
                .begin_transaction_shared(
                    member_record_keys.clone(),
                    TransactDHTRecordsOptions::default(),
                    TransactionSignerSource::Stored(signers, peek_locks),
                )
                .await?;

            let r = f(transaction_handle, member_record_keys.clone()).await;

            // If the closure didn't commit or roll back, roll back now so nothing persists.
            let still_active = self
                .inner
                .lock()
                .outbound_transaction_manager
                .get_transaction_keys(transaction_handle)
                .is_some();
            if still_active {
                if let Err(e) = self.rollback_transaction(transaction_handle).await {
                    veilid_log!(self debug "background transaction teardown rollback failed: {}", e);
                }
            }

            r
        }
        .await;

        // Restore open state: close the members we temporarily opened.
        {
            let mut inner = self.inner.lock();
            for opaque in &temp_opened {
                inner.opened_records.remove(opaque);
            }
        }

        result
    }

    /// Extend an existing outbound transaction with additional records.
    ///
    /// If an existing transaction does not exist over these records
    /// or a transaction can not be performed at this time, this will fail, and
    /// the original transaction will remain unchanged and intact.
    ///
    /// Idempotent: If no additional records are provided, this will do nothing and return Ok(()).
    ///
    /// Returns Err(VeilidAPIError::TryAgain) if the transaction could not be extended at this time
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(self), ret)
    )]
    pub async fn extend_transaction(
        &self,
        transaction_handle: OutboundTransactionHandle,
        record_keys: Vec<RecordKey>,
        options: Option<TransactDHTRecordsOptions>,
    ) -> VeilidAPIResult<()> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        // Early rejection if there are duplicate records
        if record_keys.has_duplicates() {
            apibail_missing_argument!(
                "transaction can not have duplicate record keys",
                "record_keys"
            );
        }

        // Get original transaction keys
        let Some(original_transaction_keys) = self
            .inner
            .lock()
            .outbound_transaction_manager
            .get_transaction_keys(transaction_handle)
        else {
            return Err(VeilidAPIError::transaction_not_found("missing transaction"));
        };

        // Early rejection if no records are being transacted over
        let mut additional_record_keys = record_keys.clone();
        additional_record_keys.retain(|x| !original_transaction_keys.contains(&x.opaque()));
        if additional_record_keys.is_empty() {
            // Idempotent return if no changes would be made
            return Ok(());
        }

        // Enforce record limit
        let all_transaction_keys = original_transaction_keys
            .iter()
            .cloned()
            .chain(additional_record_keys.iter().map(|x| x.opaque()))
            .collect::<Vec<_>>();
        if all_transaction_keys.len() > MAX_RECORDS_PER_TRANSACTION {
            apibail_invalid_argument!(
                format!(
                    "extend_transaction would exceed record limit: {} + {} = {} > {}",
                    original_transaction_keys.len(),
                    additional_record_keys.len(),
                    all_transaction_keys.len(),
                    MAX_RECORDS_PER_TRANSACTION
                ),
                "record_keys",
                record_keys.len()
            );
        }

        // Start a transaction with the new records
        let additional_transaction_handle = self
            .begin_transaction(additional_record_keys, options)
            .await?;

        // Blocking acquire for all records in the transaction
        let records_lock = self
            .record_lock_table
            .lock_records(
                all_transaction_keys,
                StorageManagerRecordLockPurpose::TransactExtend,
            )
            .await;

        // Merge under the inner lock so keepalive replies see the post-merge state.
        let mut opt_cleanup: Option<TransactionCleanup> = None;
        let merge_result = {
            let mut inner = self.inner.lock();
            let otm = &mut inner.outbound_transaction_manager;
            match otm.merge_transactions(transaction_handle, additional_transaction_handle) {
                Ok(()) => Ok(()),
                Err(e) => {
                    if let Some(state) = otm.drop_transaction(additional_transaction_handle) {
                        opt_cleanup = Some(state.into_transaction_cleanup());
                    }
                    Err(e)
                }
            }
        };

        // Cleanup runs in the background processor.
        if let Some(cleanup) = opt_cleanup {
            self.background_operation_processor
                .add_future(cleanup.into_future());
        }
        drop(records_lock);

        merge_result
    }

    /// Save the local snapshot for a new transaction
    async fn save_local_snapshots(
        &self,
        opaque_record_keys: Vec<OpaqueRecordKey>,
    ) -> VeilidAPIResult<BTreeMap<OpaqueRecordKey, Arc<RecordSnapshot>>> {
        let local_record_store = self.get_local_record_store()?;

        let local_snapshot_locks = local_record_store
            .prepare_snapshot_locks(opaque_record_keys)
            .await;

        let mut local_snapshots = BTreeMap::new();
        for local_snapshot_lock in local_snapshot_locks.record_lock_guards() {
            // XXX: Should this be parallelized? Would it matter?
            if let Some(local_snapshot) = local_record_store
                .snapshot_record_locked(local_snapshot_lock)
                .await?
            {
                local_snapshots.insert(local_snapshot_lock.record(), local_snapshot);
            }
        }

        Ok(local_snapshots)
    }

    /// Finalize a transaction over a set of records
    /// If an existing transaction does not exist over these records
    /// or a transaction can not be performed at this time, this will fail.
    /// Returns Err(VeilidAPIError::TryAgain) if the transaction could not be finalized at this time
    /// Returns Err(_) if the transaction finalize failed and resulted in rollback or drop
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(self))
    )]
    pub async fn end_and_commit_transaction(
        &self,
        transaction_handle: OutboundTransactionHandle,
    ) -> VeilidAPIResult<()> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        // Early rejection if dht is not online
        if !self.dht_is_online() {
            apibail_try_again!("dht is not online");
        }

        let (transaction_keys, transaction_kind) = {
            let inner = self.inner.lock();
            let otm = &inner.outbound_transaction_manager;
            let Some(transaction_keys) = otm.get_transaction_keys(transaction_handle) else {
                return Err(VeilidAPIError::transaction_not_found("missing transaction"));
            };
            (
                transaction_keys,
                otm.get_transaction_kind(transaction_handle),
            )
        };
        if transaction_kind == Some(OutboundTransactionKind::Foreground) {
            self.touch_foreground_transaction_activity();
        }

        let records_lock = Arc::new(
            self.record_lock_table
                .lock_records(
                    transaction_keys,
                    StorageManagerRecordLockPurpose::TransactEndAndCommit,
                )
                .await,
        );

        DurationRecorder::new(
            "StorageManager::end_transaction_locked",
            |name, start| {
                veilid_log!(self debug "{}[start={:#}](transaction_handle: {})", name, start, transaction_handle);
            },
        )
        .record_fut(
            self.end_transaction_locked(records_lock.clone(), transaction_handle),
            |name, start, dur, ret| {
                veilid_log!(self debug "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                ret
            },
        )
        .await?;

        DurationRecorder::new(
            "StorageManager::commit_transaction_locked",
            |name, start| {
                veilid_log!(self debug "{}[start={:#}](transaction_handle: {})", name, start, transaction_handle);
            },
        )
        .record_fut(
            self.commit_transaction_locked(records_lock.clone(), transaction_handle),
            |name, start, dur, ret| {
                veilid_log!(self debug "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                ret
            },
        )
        .await?;

        // Push everything to the local record store and drop the transaction
        DurationRecorder::new(
            "StorageManager::flush_committed_transaction_locked",
            |name, start| {
                veilid_log!(self debug "{}[start={:#}](transaction_handle: {})", name, start, transaction_handle);
            },
        )
        .record_fut(
            self.flush_committed_transaction_locked(records_lock, transaction_handle),
            |name, start, dur, ret| {
                veilid_log!(self debug "{}[start={:#} dur={:#}](ret: ())", name, start, dur);
                ret
            },
        )
        .await;

        Ok(())
    }

    /// End a transaction over a set of records
    /// If an existing transaction does not exist over these records
    /// or a transaction can not be performed at this time, this will fail.
    /// Returns Err(VeilidAPIError::TryAgain) if the transaction could not be ended at this time
    /// Returns Err(_) if the transaction end failed and resulted in rollback or drop
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(self, records_lock))
    )]
    pub(super) async fn end_transaction_locked(
        &self,
        records_lock: Arc<StorageManagerRecordsLockGuard>,
        transaction_handle: OutboundTransactionHandle,
    ) -> VeilidAPIResult<()> {
        let (opt_cleanup, ()) = Box::pin(self.rollback_guard_locked(
            records_lock.clone(),
            transaction_handle,
            async move {
                let command_params_list = {
                    let mut inner = self.inner.lock();

                    // Obtain the outbound transaction manager
                    let otm = &mut inner.outbound_transaction_manager;

                    // Prepare for rollback
                    otm.prepare_transact_end_params(transaction_handle)
                        .inspect_err(|e| {
                            veilid_log!(self debug "error in prepare_transact_end_params: {}", e);
                        })?
                };

                let rpc_timeout = TimestampDuration::new_ms(
                    self.config().internal().network.rpc.timeout_ms.into(),
                );

                // End transactions on all records.
                let mut unord = FuturesUnordered::new();
                for command_params in command_params_list {
                    let fut = self
                        .outbound_transact_command(command_params)
                        .measure_debug(
                            rpc_timeout,
                            veilid_log_dbg!(
                                self,
                                "StorageManager::end_transaction_locked outbound_transact_command"
                            ),
                        );
                    unord.push(fut);
                }
                let mut results = vec![];
                let mut opt_end_error = None;
                while let Some(res) = unord.next().await {
                    match res {
                        Ok(v) => {
                            //
                            results.push(v);
                        }
                        Err(e) => {
                            veilid_log!(self debug "error in end transaction: {}", e);
                            if opt_end_error.is_none() {
                                opt_end_error = Some(e);
                            }
                        }
                    }
                }

                // Store end results
                {
                    let mut inner = self.inner.lock();
                    let otm = &mut inner.outbound_transaction_manager;
                    if let Err(e) = otm.record_transact_end_results(transaction_handle, results) {
                        veilid_log!(self debug "Recording end transaction failed: {}", e);
                        if opt_end_error.is_none() {
                            opt_end_error = Some(e);
                        }
                    }
                };

                // Rollback if any errors happened
                if let Some(end_error) = opt_end_error {
                    return Err(end_error);
                }

                Ok(())
            },
        ))
        .await?;

        // We're inside the END→COMMIT barrier, so await cleanup directly to
        // ensure all node-transaction states are settled before COMMIT issues.
        if let Some(cleanup) = opt_cleanup {
            cleanup.into_future().await;
        }

        Ok(())
    }

    /// Commit a transaction over a set of records
    /// If an existing transaction does not exist over these records
    /// or a transaction can not be performed at this time, this will fail.
    /// Returns Err(VeilidAPIError::TryAgain) if the transaction could not be committed at this time
    /// Returns Err(_) if the transaction commit failed and resulted in rollback or drop
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(self, records_lock))
    )]
    pub(super) async fn commit_transaction_locked(
        &self,
        records_lock: Arc<StorageManagerRecordsLockGuard>,
        transaction_handle: OutboundTransactionHandle,
    ) -> VeilidAPIResult<()> {
        let (opt_cleanup, ()) = Box::pin(self.rollback_guard_locked(
            records_lock.clone(),
            transaction_handle,
            async move {
                let command_params_list = {
                    let mut inner = self.inner.lock();

                    // Obtain the outbound transaction manager
                    let otm = &mut inner.outbound_transaction_manager;

                    // Prepare for commit
                    otm.prepare_transact_commit_params(transaction_handle)
                    .inspect_err(|e| {
                        veilid_log!(self debug "error in prepare_transact_commit_params: {}", e);
                    })?
                };

                let rpc_timeout = TimestampDuration::new_ms(
                    self.config().internal().network.rpc.timeout_ms.into(),
                );

                // Commit transactions on all records
                let mut unord = FuturesUnordered::new();
                for command_params in command_params_list {
                    let fut = self
                        .outbound_transact_command(command_params)
                        .measure_debug(
                            rpc_timeout,
                            veilid_log_dbg!(
                            self,
                            "StorageManager::commit_transaction_locked outbound_transact_command"
                        ),
                        );
                    unord.push(fut);
                }
                let mut results = vec![];
                let mut opt_commit_error = None;
                while let Some(res) = unord.next().await {
                    match res {
                        Ok(v) => {
                            //
                            results.push(v);
                        }
                        Err(e) => {
                            veilid_log!(self debug "Commit transaction failed: {}", e);

                            if opt_commit_error.is_none() {
                                opt_commit_error = Some(e);
                            }
                        }
                    }
                }

                // Store commit results
                {
                    let mut inner = self.inner.lock();
                    if let Err(e) = inner
                        .outbound_transaction_manager
                        .record_transact_commit_results(transaction_handle, results)
                    {
                        veilid_log!(self debug "Recording commit transaction failed: {}", e);

                        if opt_commit_error.is_none() {
                            opt_commit_error = Some(e);
                        }
                    }

                    if let Some(err) = opt_commit_error {
                        return Err(err);
                    }
                }
                Ok(())
            },
        ))
        .await?;

        // Cleanup runs in the background processor.
        if let Some(cleanup) = opt_cleanup {
            self.background_operation_processor
                .add_future(cleanup.into_future());
        }
        drop(records_lock);

        Ok(())
    }

    /// Removes the transaction from the transaction manager
    /// and flushes its contents to the storage manager
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip(self, records_lock))
    )]
    pub(super) async fn flush_committed_transaction_locked(
        &self,
        records_lock: Arc<StorageManagerRecordsLockGuard>,
        transaction_handle: OutboundTransactionHandle,
    ) {
        let drop_recorder = DurationRecorder::new(
            "StorageManager::flush_drop_transaction",
            |name, start| {
                veilid_log!(self debug "{}[start={:#}](handle: {})", name, start, transaction_handle);
            },
        );
        let drop_result = drop_recorder.record(|| {
            let mut inner = self.inner.lock();

            let Some(transaction_state) = inner
                .outbound_transaction_manager
                .drop_transaction(transaction_handle)
            else {
                veilid_log!(self error "missing transaction in flush: {}", transaction_handle);
                return None;
            };

            let mut record_commit_values = vec![];
            for record_state in transaction_state.get_record_states() {
                let opaque_record_key = record_state.record_key().opaque();
                let operation_signer = record_state.operation_signer().clone();
                let local_commit_results = match record_state.local_commit_results() {
                    Ok(v) => v,
                    Err(e) => {
                        veilid_log!(self error "failed to get local commit results for transaction {}: {}", transaction_handle, e);
                        return None;
                    }
                };

                #[cfg(feature = "verbose-tracing")]
                {
                    veilid_log!(self debug "Flush commit for handle={} record={}: {} subkeys to write locally",
                        transaction_handle,
                        opaque_record_key,
                        local_commit_results.len()
                    );
                    for (subkey, svd) in &local_commit_results {
                        veilid_log!(self debug "  subkey {} seq={}", subkey, svd.value_data().seq());
                    }
                }
                record_commit_values.push(RecordCommitValues {
                    opaque_record_key,
                    operation_signer,
                    subkey_values: local_commit_results,
                });
            }

            let cleanup = transaction_state.into_transaction_cleanup();
            Some((record_commit_values, cleanup))
        }, |name, start, dur, ret| {
            veilid_log!(self debug "{}[start={:#} dur={:#}](ret: {})", name, start, dur, if ret.is_some() { "drained" } else { "missing" });
            ret
        });
        let Some((record_commit_values, cleanup)) = drop_result else {
            return;
        };

        // Record the set values locally since they were successfully set online
        if let Err(e) = DurationRecorder::new("StorageManager::flush_local_set", |name, start| {
            veilid_log!(self debug "{}[start={:#}](handle: {}, record_count: {})", name, start, transaction_handle, record_commit_values.len());
        })
        .record_fut(
            self.handle_set_local_values_with_multiple_records_lock(&records_lock, record_commit_values),
            |name, start, dur, ret| {
                veilid_log!(self debug "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                ret
            },
        )
        .await
        {
            veilid_log!(self error "failed to set local values with commit results for transaction {}: {}", transaction_handle, e);
        }

        // Cleanup runs in the background processor.
        self.background_operation_processor
            .add_future(cleanup.into_future());
    }

    /// Roll back a transaction
    /// If the transaction no longer exists, this does nothing.
    /// If an error is returned, the transaction is left in a failed state and can either
    /// * be dropped/ignored and the remote transaction will time out
    /// * another rollback attempt can be made, which may result in a more polite termination of the remote transaction
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip(self))
    )]
    pub async fn rollback_transaction(
        &self,
        transaction_handle: OutboundTransactionHandle,
    ) -> VeilidAPIResult<()> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let (transaction_keys, transaction_kind) = {
            let inner = self.inner.lock();
            let otm = &inner.outbound_transaction_manager;
            let Some(transaction_keys) = otm.get_transaction_keys(transaction_handle) else {
                // Early exit if transaction is already gone
                return Ok(());
            };
            (
                transaction_keys,
                otm.get_transaction_kind(transaction_handle),
            )
        };
        // Background teardown also comes through here; only foreground rollbacks defer background work
        if transaction_kind == Some(OutboundTransactionKind::Foreground) {
            self.touch_foreground_transaction_activity();
        }

        let records_lock = self
            .record_lock_table
            .lock_records(
                transaction_keys,
                StorageManagerRecordLockPurpose::TransactRollback,
            )
            .await;

        // Early rejection if dht is not online
        if !self.dht_is_online() {
            apibail_try_again!("dht is not online");
        }

        // Send all rollbacks to the network
        self.rollback_transaction_locked(&records_lock, transaction_handle)
            .await?;

        // Transaction is done successfully, drop it and wait for background tasks to complete if any
        self.drop_transaction_and_wait_locked(&records_lock, transaction_handle)
            .await;

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip(self, _records_lock))
    )]
    pub(super) async fn rollback_transaction_locked(
        &self,
        _records_lock: &StorageManagerRecordsLockGuard,
        transaction_handle: OutboundTransactionHandle,
    ) -> VeilidAPIResult<()> {
        let command_params_list = {
            let mut inner = self.inner.lock();

            // Obtain the outbound transaction manager
            let otm = &mut inner.outbound_transaction_manager;

            // Prepare for rollback
            otm.prepare_rollback_transact_value_params(transaction_handle, None)
                .inspect_err(|e| {
                    veilid_log!(self debug "error in prepare_rollback_transact_value_params: {}", e);
                })?
        };

        let rpc_timeout =
            TimestampDuration::new_ms(self.config().internal().network.rpc.timeout_ms.into());

        // Rollback transactions on all records
        let mut unord = FuturesUnordered::new();
        for command_params in command_params_list {
            let fut = self
                .outbound_transact_command(command_params)
                .measure_debug(
                    rpc_timeout,
                    veilid_log_dbg!(
                        self,
                        "StorageManager::rollback_transaction_locked outbound_transact_command"
                    ),
                );
            unord.push(fut);
        }
        let mut results = vec![];
        let mut opt_rollback_error = None;
        while let Some(res) = unord.next().await {
            match res {
                Ok(v) => {
                    //
                    results.push(v);
                }
                Err(e) => {
                    if opt_rollback_error.is_none() {
                        opt_rollback_error = Some(e);
                    }
                }
            }
        }

        // Store rollback results
        {
            let mut inner = self.inner.lock();
            let otm = &mut inner.outbound_transaction_manager;
            if let Err(e) = otm.record_transact_rollback_results(transaction_handle, results) {
                if opt_rollback_error.is_none() {
                    opt_rollback_error = Some(e);
                }
            }
        }

        if let Some(rberr) = opt_rollback_error {
            return Err(rberr);
        }

        Ok(())
    }

    /// Get a value within a transaction
    /// Does not use fanout
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip(self), ret)
    )]
    pub async fn transaction_get(
        &self,
        transaction_handle: OutboundTransactionHandle,
        record_key: RecordKey,
        subkey: ValueSubkey,
    ) -> VeilidAPIResult<Option<ValueData>> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let rpc_timeout =
            TimestampDuration::new_ms(self.config().internal().network.rpc.timeout_ms.into());

        let _subkey_lock = self
            .record_lock_table
            .lock_subkey(
                record_key.opaque(),
                subkey,
                StorageManagerSubkeyLockPurpose::TransactGet,
            )
            .measure_debug(
                rpc_timeout,
                veilid_log_dbg!(self, "StorageManager::transaction_get lock_subkey"),
            )
            .await;

        // Early rejection if dht is not online
        if !self.dht_is_online() {
            apibail_try_again!("dht is not online");
        }

        let command_params = {
            let opaque_record_key = record_key.opaque();

            let mut inner = self.inner.lock();
            let otm = &mut inner.outbound_transaction_manager;

            // Note: in theory, we could return a null value here if no sequence number existed at begin time.
            // But we are reserving the right to include transactional 'gets' in the -remote transaction state-.
            // Which means we have to send the get request to the network even if nothing should be returned.

            // Prepare for get value
            otm.prepare_transact_get_params(transaction_handle, &opaque_record_key, subkey)
                .inspect_err(|e| {
                    veilid_log!(self debug "error in prepare_transact_get_params: {}", e);
                })?
        };

        // Gate on concurrency + the download budget (a get pulls a full value down)
        let gate_cost = (MAX_SUBKEY_SIZE as u64).saturating_mul(command_params.nodes.len() as u64);
        let gate_permit = self.acquire_operation_gate(None, Some(gate_cost)).await;

        // Send all get commands
        let result = self
            .outbound_transact_command(command_params)
            .measure_debug(
                rpc_timeout,
                veilid_log_dbg!(
                    self,
                    "StorageManager::transaction_get outbound_transact_command"
                ),
            )
            .await
            .inspect_err(|e| {
                veilid_log!(self debug "Transaction get failed: {}", e);
            })?;

        // Done with network access, release the semaphore
        drop(gate_permit);

        let get_signed_value_data = {
            let mut inner = self.inner.lock();
            let otm = &mut inner.outbound_transaction_manager;
            otm.record_transact_get_result(transaction_handle, result)
                .inspect_err(|e| {
                    veilid_log!(self debug "Recording get transaction failed: {}", e);
                })?;

            // Return newest value
            let outbound_transaction_state = otm
                .get_transaction_state(transaction_handle)
                .inspect_err(|e| {
                    veilid_log!(self debug "Missing transaction state: {}", e);
                })?;
            let Some(record_state) =
                outbound_transaction_state.get_record_state(&record_key.opaque())
            else {
                apibail_internal!("missing record in get: {}", record_key.opaque());
            };
            let subkey_get_result = record_state.current_subkey_get_result(subkey)?;
            let Some(get_signed_value_data) = subkey_get_result.opt_value else {
                // Check if the transaction should have had a value because the begin had a sequence number
                let begin_network_seq = record_state.begin_network_seq(subkey)?;
                if begin_network_seq.is_some() {
                    // Sequence number existed at begin time, so we should have gotten a value
                    apibail_try_again!(
                        "sequence number existed at transaction begin, but no value was returned"
                    );
                }
                return Ok(None);
            };
            get_signed_value_data
        };

        let get_value_data = self
            .maybe_decrypt_value_data(&record_key, get_signed_value_data.value_data())
            .await?;

        // Return the value we got
        Ok(Some(get_value_data))
    }

    /// Set a value within a transaction
    /// Does not use fanout
    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "dht", skip(self, data), fields(data.len = data.len()), ret))]
    pub async fn transaction_set(
        &self,
        transaction_handle: OutboundTransactionHandle,
        record_key: RecordKey,
        subkey: ValueSubkey,
        data: Vec<u8>,
        options: Option<DHTTransactionSetValueOptions>,
    ) -> VeilidAPIResult<Option<ValueData>> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let rpc_timeout =
            TimestampDuration::new_ms(self.config().internal().network.rpc.timeout_ms.into());

        let _subkey_lock = self
            .record_lock_table
            .lock_subkey(
                record_key.opaque(),
                subkey,
                StorageManagerSubkeyLockPurpose::TransactSet,
            )
            .measure_debug(
                rpc_timeout,
                veilid_log_dbg!(self, "StorageManager::transaction_set lock_subkey"),
            )
            .await;

        let opaque_record_key = record_key.opaque();

        // Early rejection if dht is not online
        if !self.dht_is_online() {
            apibail_try_again!("dht is not online");
        }

        // Build the signed value data for this set from the raw data and the record's writer
        let signed_value_data = {
            let (writer, last_get_result) = {
                let inner = &*self.inner.lock();
                let otm = &inner.outbound_transaction_manager;

                // Get last known value for this subkey from the transaction
                let last_get_result = {
                    let outbound_transaction_state =
                        otm.get_transaction_state(transaction_handle)?;
                    let record_state = outbound_transaction_state
                        .get_record_state(&opaque_record_key)
                        .ok_or_else(|| VeilidAPIError::internal("missing record state"))?;

                    record_state.current_subkey_get_result(subkey)?
                };

                // Use the specified writer, or if not specified, the default writer when the record was opened
                let opt_writer = {
                    let Some(opened_record) = inner.opened_records.get(&opaque_record_key) else {
                        apibail_invalid_argument!(
                            "record not open",
                            "record_key",
                            opaque_record_key
                        );
                    };
                    opened_record.writer().cloned()
                };
                let opt_writer = options
                    .as_ref()
                    .and_then(|o| o.writer.clone())
                    .or(opt_writer);

                // If we don't have a writer then we can't write
                let Some(writer) = opt_writer else {
                    apibail_generic!("value is not writable");
                };
                (writer, last_get_result)
            };

            // Make signed value data (encrypted) and value data (unencrypted) and get descriptor for this value
            let (signed_value_data, _, _) = self
                .prepare_set_value_data(&record_key, subkey, data, &writer, last_get_result)
                .await?;
            signed_value_data
        };

        // Perform the set within the transaction using the prepared signed value
        let opt_current_signed_value_data = self
            .transaction_set_signed_value_data(
                transaction_handle,
                record_key.clone(),
                subkey,
                signed_value_data,
            )
            .await?;

        // Decrypt any newer value found online for return to the caller
        match opt_current_signed_value_data {
            Some(svd) => Ok(Some(
                self.maybe_decrypt_value_data(&record_key, svd.value_data())
                    .await?,
            )),
            None => Ok(None),
        }
    }

    /// The second half of `transaction_set`: set an already-signed value within the transaction.
    /// Used by `transaction_set` (freshly signed) and by rehydration (an existing SignedValueData
    /// from the local store, pushed unchanged). The caller must hold the TransactSet subkey lock.
    /// Returns the raw signed value found online if it was newer/different than what was set; the
    /// caller is responsible for any decryption.
    pub(super) async fn transaction_set_signed_value_data(
        &self,
        transaction_handle: OutboundTransactionHandle,
        record_key: RecordKey,
        subkey: ValueSubkey,
        signed_value_data: Arc<SignedValueData>,
    ) -> VeilidAPIResult<Option<Arc<SignedValueData>>> {
        let opaque_record_key = record_key.opaque();
        let rpc_timeout =
            TimestampDuration::new_ms(self.config().internal().network.rpc.timeout_ms.into());

        // Uplink cost of this set: the value sent to every node in the command
        let set_value_bytes = signed_value_data.total_size() as u64;

        // Prepare for set value
        let command_params = {
            let inner = &mut *self.inner.lock();
            let otm = &mut inner.outbound_transaction_manager;
            otm.prepare_transact_set_params(
                transaction_handle,
                &opaque_record_key,
                subkey,
                signed_value_data,
            )
            .inspect_err(|e| {
                veilid_log!(self debug "error in prepare_transact_set_params: {}", e);
            })?
        };

        // Gate on concurrency + the upload budget (a set pushes the value up)
        let gate_cost = set_value_bytes.saturating_mul(command_params.nodes.len() as u64);
        let gate_permit = self.acquire_operation_gate(Some(gate_cost), None).await;

        // Send all set commands
        let result = self
            .outbound_transact_command(command_params)
            .measure_debug(
                rpc_timeout,
                veilid_log_dbg!(
                    self,
                    "StorageManager::transaction_set outbound_transact_command"
                ),
            )
            .await
            .inspect_err(|e| {
                veilid_log!(self debug "Transaction set failed: {}", e);
            })?;

        // Done with network access, release the semaphore
        drop(gate_permit);

        let opt_current_signed_value_data = {
            let mut inner = self.inner.lock();
            let otm = &mut inner.outbound_transaction_manager;
            otm.record_transact_set_result(transaction_handle, result)
                .inspect_err(|e| {
                    veilid_log!(self debug "Recording set transaction failed: {}", e);
                })?;

            // Return newer value if it is not what we set
            let outbound_transaction_state = otm
                .get_transaction_state(transaction_handle)
                .inspect_err(|e| {
                    veilid_log!(self debug "Missing transaction state: {}", e);
                })?;

            let record_state = outbound_transaction_state
                .get_record_state(&opaque_record_key)
                .ok_or_else(|| VeilidAPIError::internal("missing record state"))?;

            // If there is an updated value, it means the set succeeded
            // If the set found a newer value online then this gets cleared for the subkey
            if let Some(updated_consensus) = record_state.updated_consensus().get(subkey) {
                // Ensure the updated consensus meets the strict consensus requirement.
                let required = record_state.required_strict_consensus_count();
                if updated_consensus.strict_consensus_count < required {
                    // Otherwise, ask the app to try the set again to continue to attempt consensus
                    apibail_try_again!("set did not reach consensus");
                }
                // Return that the set updated with consensus successfully
                return Ok(None);
            };

            // If the set found a newer value it would be recorded in the current consensus
            // unless an error condition was hit, in which case we should have failed out with an error
            let Some(current_subkey_consensus) = record_state.current_consensus().get(subkey)
            else {
                apibail_internal!(
                    "record subkey {} should have a current consensus: {}",
                    subkey,
                    record_key.opaque()
                );
            };

            // Return current subkey consensus value data
            current_subkey_consensus.opt_value.clone()
        };

        let Some(current_signed_value_data) = opt_current_signed_value_data else {
            apibail_internal!(
                "record subkey {} consensus value should not be missing: {}",
                subkey,
                record_key.opaque()
            );
        };

        // Return the newer or different signed value that was found online (caller decrypts)
        Ok(Some(current_signed_value_data))
    }

    /// Inspect a record within a transaction, does not perform any network
    /// activity, as the transaction state keeps all of the required information
    /// after the begin.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip(self), ret)
    )]
    pub fn transaction_inspect(
        &self,
        transaction_handle: OutboundTransactionHandle,
        record_key: RecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
        scope: DHTReportScope,
    ) -> VeilidAPIResult<DHTRecordReport> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let inner = self.inner.lock();
        inner.outbound_transaction_manager.get_record_report(
            transaction_handle,
            &record_key.opaque(),
            subkeys,
            scope,
        )
    }

    /// Background rollback function used to remove nodes from a transaction
    /// and speculatively issue rollback RPCs to them to help them release their server
    /// side transactions early. Runs detached in the background as we never care about
    /// the result.
    pub(super) fn partial_drop_and_background_rollback_locked(
        &self,
        _records_lock: &StorageManagerRecordsLockGuard,
        transaction_handle: OutboundTransactionHandle,
        node_transactions_to_drop: LocalNodeTransactionIdSet,
        node_transactions_to_rollback: LocalNodeTransactionIdSet,
    ) -> VeilidAPIResult<()> {
        let command_params_list = {
            let mut inner = self.inner.lock();

            // Obtain the outbound transaction manager
            let otm = &mut inner.outbound_transaction_manager;

            // Prepare all rollbacks -first-
            let command_params_list = otm.prepare_rollback_transact_value_params(
                transaction_handle,
                Some(node_transactions_to_rollback),
            )
            .inspect_err(|e| {
                veilid_log!(self debug "error in prepare_rollback_transact_value_params: {}", e);
            })?;

            // Then process all node transaction drops -second-
            otm.drop_node_transactions(transaction_handle, node_transactions_to_drop)?;

            command_params_list
        };

        // Process background rollbacks -third-
        let rpc_timeout =
            TimestampDuration::new_ms(self.config().internal().network.rpc.timeout_ms.into());
        let registry = self.registry();
        let transaction_handle_clone = transaction_handle;
        let background_rollback_fut = async move {
            let this = registry.storage_manager();

            // Rollback transactions on all records
            let mut unord = FuturesUnordered::new();
            for command_params in command_params_list {
                let fut = this
                    .outbound_transact_command(command_params)
                    .measure_debug(
                        rpc_timeout,
                        veilid_log_dbg!(
                            this,
                            "StorageManager::partial_drop_and_background_rollback_locked outbound_transact_command"
                        ),
                    );
                unord.push(fut);
            }
            while let Some(res) = unord.next().await {
                match res {
                    Ok(result) => {
                        let mut command_node_lnxids = result.get_command_lnxids();
                        for pnr in result.per_node_results {
                            if !command_node_lnxids.remove(&pnr.lnxid) {
                                veilid_log!(this debug
                                    "node transaction has multiple results: {} pnr={:?}",
                                    result.params.opaque_record_key,
                                    pnr
                                );
                            }
                        }

                        // Any commands that did not return a result the background rollback
                        if !command_node_lnxids.is_empty() {
                            veilid_log!(this debug "Partial rollback of {} failed for: {:?}", transaction_handle_clone, command_node_lnxids);
                        }
                    }
                    Err(e) => {
                        veilid_log!(this debug "Error in partial_drop_and_background_rollback_locked: {}", e);
                    }
                }
            }
        };

        // Add the background task to the transaction
        let mut inner = self.inner.lock();
        let otm = &mut inner.outbound_transaction_manager;
        otm.add_transaction_background_task(transaction_handle, background_rollback_fut)
    }

    /// Guard function used to ensure that errors on whole-transaction operations cause rollback attempts
    /// Also validates that the state is the same for all records in the transaction and attempts to
    /// reconcile node states that are different.
    /// For example, if a single node ends up in an 'End' state while other nodes end up in 'Rollback'
    /// this routine will make a best-effort attempt to rollback the 'End' state node.
    pub(super) async fn rollback_guard_locked<V, F: Future<Output = VeilidAPIResult<V>>>(
        &self,
        records_lock: Arc<StorageManagerRecordsLockGuard>,
        transaction_handle: OutboundTransactionHandle,
        future: F,
    ) -> VeilidAPIResult<(Option<TransactionCleanup>, V)> {
        let res = future.await;

        let mut opt_cleanup = None;

        let v = match res {
            Ok(v) => self.rollback_guard_locked_success(
                &records_lock,
                transaction_handle,
                v,
                &mut opt_cleanup,
            )?,
            Err(e) => {
                veilid_log!(self debug target: "network_result", "Rolling back due to error: {:?}: {}", transaction_handle, e);

                // Sync drop of local state; rollback fanout runs in the background holding the moved records_lock
                self.rollback_guard_locked_failure(records_lock, transaction_handle);

                return Err(e);
            }
        };

        Ok((opt_cleanup, v))
    }

    // Process stage consensus operations
    // Returns either the value or an error if consensus operations could not be performed
    // Also returns cleanup to process through the mutable reference parameter
    fn rollback_guard_locked_success<V>(
        &self,
        records_lock: &StorageManagerRecordsLockGuard,
        transaction_handle: OutboundTransactionHandle,
        value: V,
        opt_cleanup: &mut Option<TransactionCleanup>,
    ) -> VeilidAPIResult<V> {
        let stage_consensus = {
            let mut inner = self.inner.lock();
            let res = inner
                .outbound_transaction_manager
                .get_transaction_state(transaction_handle);
            let state = match res {
                Ok(state) => state,
                Err(e) => {
                    veilid_log!(self debug "Error getting transaction state in guard: {}", e);

                    // Drop the transaction and return cleanup to process
                    if let Some(state) = inner
                        .outbound_transaction_manager
                        .drop_transaction(transaction_handle)
                    {
                        *opt_cleanup = Some(state.into_transaction_cleanup());
                    }

                    return Err(e);
                }
            };

            let Some(stage_consensus) = state.stage_consensus() else {
                // Should not be trying to roll back something that is still in the INIT state
                apibail_internal!(
                    "no stage consensus yet for rollback guard: {}",
                    transaction_handle
                );
            };
            stage_consensus
        };

        let drop_ids = stage_consensus.node_transactions_to_drop;
        let rollback_ids = stage_consensus.node_transactions_to_rollback;
        if !rollback_ids.is_empty() || !drop_ids.is_empty() {
            // Perform partial speculative rollback and drop from transaction
            if let Err(e) = self.partial_drop_and_background_rollback_locked(
                records_lock,
                transaction_handle,
                drop_ids,
                rollback_ids,
            ) {
                veilid_log!(self debug "Error in partial drop and roll back transaction: {}", e);

                // Drop the transaction and return cleanup to process
                if let Some(state) = self
                    .inner
                    .lock()
                    .outbound_transaction_manager
                    .drop_transaction(transaction_handle)
                {
                    *opt_cleanup = Some(state.into_transaction_cleanup());
                }

                return Err(e);
            }
        }

        Ok(value)
    }

    /// Failure-case counterpart to rollback_guard_locked_success.
    ///
    /// Drops local transaction state synchronously, then runs the Rollback RPCs in the background
    /// holding the records_lock until they finish — so a retry can't re-begin on these records
    /// while their nodes still hold this transaction's node-transactions (which starves begin
    /// consensus).
    fn rollback_guard_locked_failure(
        &self,
        records_lock: Arc<StorageManagerRecordsLockGuard>,
        transaction_handle: OutboundTransactionHandle,
    ) {
        let (rollback_params, opt_cleanup) = {
            let mut inner = self.inner.lock();
            let otm = &mut inner.outbound_transaction_manager;
            let rollback_params = otm
                .prepare_rollback_transact_value_params(transaction_handle, None)
                .inspect_err(|e| {
                    veilid_log!(self debug "error preparing rollback after failure: {}", e);
                })
                .unwrap_or_default();
            let opt_cleanup = otm
                .drop_transaction(transaction_handle)
                .map(|state| state.into_transaction_cleanup());
            (rollback_params, opt_cleanup)
        };

        if rollback_params.is_empty() && opt_cleanup.is_none() {
            return;
        }

        let rpc_timeout =
            TimestampDuration::new_ms(self.config().internal().network.rpc.timeout_ms.into());
        let registry = self.registry();
        let background_rollback_fut = async move {
            let this = registry.storage_manager();

            let mut unord = FuturesUnordered::new();
            for params in rollback_params {
                let fut = this.outbound_transact_command(params).measure_debug(
                    rpc_timeout,
                    veilid_log_dbg!(
                        this,
                        "StorageManager::rollback_guard_locked_failure outbound_transact_command"
                    ),
                );
                unord.push(fut);
            }
            while let Some(res) = unord.next().await {
                if let Err(e) = res {
                    veilid_log!(this debug "Error in rollback_guard_locked_failure: {}", e);
                }
            }

            if let Some(cleanup) = opt_cleanup {
                cleanup.await;
            }

            // Hold the records lock until the rollback RPCs actually finish, so a
            // retry can't re-begin on these records while their nodes still hold
            // this transaction's node-transactions (they'd report busy and starve
            // begin consensus).
            drop(records_lock);
        };

        self.background_operation_processor
            .add_future(background_rollback_fut);
    }

    /// Convenience function to drop transaction and wait for background tasks to complete
    async fn drop_transaction_and_wait_locked(
        &self,
        _records_lock: &StorageManagerRecordsLockGuard,
        transaction_handle: OutboundTransactionHandle,
    ) {
        let opt_cleanup = {
            let mut inner = self.inner.lock();
            inner
                .outbound_transaction_manager
                .drop_transaction(transaction_handle)
                .map(|state| state.into_transaction_cleanup())
        };
        if let Some(cleanup) = opt_cleanup {
            cleanup.await;
        }
    }

    /// Schedule a transaction to be dropped
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip(self))
    )]
    pub fn drop_transaction_sync(&self, transaction_handle: OutboundTransactionHandle) {
        let registry = self.registry();
        self.background_operation_processor.add_future(async move {
            let this = registry.storage_manager();

            let Some(transaction_keys) = this
                .inner
                .lock()
                .outbound_transaction_manager
                .get_transaction_keys(transaction_handle)
            else {
                veilid_log!(this debug "Transaction already dropped in drop_transaction_sync: {}", transaction_handle);
                return;
            };

            let records_lock = this
                .record_lock_table
                .lock_records(
                    transaction_keys,
                    StorageManagerRecordLockPurpose::TransactDrop,
                )
                .await;

            // Drop the transaction and wait for background tasks to complete if any
            this.drop_transaction_and_wait_locked(&records_lock, transaction_handle)
                .await;
        });
    }
}
