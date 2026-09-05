//! RecordStore
//! Keeps an LRU cache of dht keys and their associated subkey valuedata.
//! Instances of this store are used for 'local' (persistent) and 'remote' (ephemeral) dht key storage.
//! This store does not perform any validation on the schema, and all ValueRecordData passed in must have been previously validated.
//! Uses an in-memory store for the records, backed by the TableStore. Subkey data is LRU cached and rotated out by a limits policy,
//! and backed to the TableStore for persistence.

mod debug;
mod inbound_watch;
mod opened_record;
mod record;
mod record_snapshot;
mod record_store_inner;
mod record_store_limits;
mod record_store_locks;
mod results;

#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod tests;

use super::*;

pub(super) use inbound_watch::*;
pub(super) use opened_record::*;
pub(super) use record::*;
pub(super) use record_snapshot::*;
pub(super) use record_store_inner::{InboundTransactionId, InboundWatchId};
pub(super) use record_store_limits::*;
pub(super) use results::*;

use record_store_inner::*;
use record_store_locks::*;

impl_veilid_log_facility!("stor");

/// Whether to flush the commit action immediately or defer it to the background
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum CommitActionFlushMode {
    /// Flush the commit action immediately
    Immediate,
    /// Defer the commit action to the background
    Deferred,
}

pub(super) struct RecordStoreUnlockedInner {
    registry: VeilidComponentRegistry,
    name: String,
    limits: RecordStoreLimits,

    /// The tabledb used for record data
    record_table: TableDB,
    /// The tabledb used for subkey data
    subkey_table: TableDB,
}

/// Record detail trait
pub(super) trait RecordDetail:
    fmt::Debug + Clone + PartialEq + Eq + Serialize + for<'d> Deserialize<'d> + GetSize
{
    fn is_new(&self) -> bool;
}

/// Record store interface
#[derive(Clone)]
pub(super) struct RecordStore<D>
where
    D: RecordDetail,
{
    // Immutable record store data
    unlocked_inner: Arc<RecordStoreUnlockedInner>,

    // Mutable record store data
    inner: Arc<Mutex<RecordStoreInner<D>>>,

    // Async record locks
    record_store_lock_table: RecordStoreRecordLockTable,
}

impl<D> VeilidComponentRegistryAccessor for RecordStore<D>
where
    D: RecordDetail,
{
    fn registry(&self) -> VeilidComponentRegistry {
        self.unlocked_inner.registry.clone()
    }
}

impl<D> fmt::Debug for RecordStore<D>
where
    D: RecordDetail,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecordStore")
            .field("name", &self.unlocked_inner.name)
            .field("limits", &self.unlocked_inner.limits)
            .field("inner", &self.inner)
            .field("record_lock_table", &self.record_store_lock_table)
            .finish()
    }
}

pub(super) type SubkeyValueList = Vec<(ValueSubkey, Arc<SignedValueData>)>;
pub(super) type RecordSubkeyValueList = Vec<(OpaqueRecordKey, SubkeyValueList)>;

/// A record's committed subkey values plus the operation signer used, for the commit-time local
/// write and for stamping the record's transaction membership
#[derive(Clone)]
pub(super) struct RecordCommitValues {
    pub opaque_record_key: OpaqueRecordKey,
    pub operation_signer: KeyPair,
    pub subkey_values: SubkeyValueList,
}

impl<D> RecordStore<D>
where
    D: RecordDetail,
{
    pub async fn try_new(
        table_store: &TableStore,
        name: &str,
        limits: RecordStoreLimits,
    ) -> EyreResult<Self> {
        let record_table = table_store.open(&format!("{}_records", name), 1).await?;
        let subkey_table = table_store
            .open_pooled(&format!("{}_subkeys", name), 1, limits.pool_concurrency)
            .await?;

        let registry = table_store.registry();

        let unlocked_inner = Arc::new(RecordStoreUnlockedInner {
            registry,
            name: name.to_owned(),
            limits,
            record_table,
            subkey_table,
        });

        let inner = RecordStoreInner::<D>::try_new(unlocked_inner.clone()).await?;

        let record_store_lock_table =
            RecordStoreRecordLockTable::new(unlocked_inner.registry.clone());

        Ok(Self {
            unlocked_inner,
            inner: Arc::new(Mutex::new(inner)),
            record_store_lock_table,
        })
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub async fn flush(&self) -> VeilidAPIResult<()> {
        let opt_commit_action = self.inner.lock().flush();
        if let Some(commit_action) = opt_commit_action {
            self.process_commit_action(commit_action).await?;
        };
        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub async fn new_record(
        &self,
        opaque_record_key: OpaqueRecordKey,
        record: Record<D>,
    ) -> VeilidAPIResult<()> {
        let _record_lock = self
            .record_store_lock_table
            .lock_record(opaque_record_key.clone(), RecordStoreRecordLockPurpose::New)
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::new_record lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        let opt_commit_action = self.inner.lock().new_record(opaque_record_key, record)?;
        if let Some(commit_action) = opt_commit_action {
            self.process_commit_action(commit_action).await?;
        };

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub async fn delete_record(&self, opaque_record_key: &OpaqueRecordKey) -> VeilidAPIResult<()> {
        let _record_lock = self
            .record_store_lock_table
            .lock_record(
                opaque_record_key.clone(),
                RecordStoreRecordLockPurpose::Delete,
            )
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::delete_record lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        let opt_commit_action = self.inner.lock().delete_record(opaque_record_key)?;
        if let Some(commit_action) = opt_commit_action {
            self.process_commit_action(commit_action).await?;
        };

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub async fn get_subkey(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
        want_descriptor: bool,
    ) -> VeilidAPIResult<Option<GetResult>> {
        let _subkey_lock = self
            .record_store_lock_table
            .lock_subkey(
                opaque_record_key.clone(),
                subkey,
                RecordStoreSubkeyLockPurpose::Get,
            )
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::get_subkey lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        let load_action_result = {
            let mut inner = self.inner.lock();
            inner.prepare_get_load_action(opaque_record_key, subkey)?
        };

        match load_action_result {
            LoadActionResult::NoRecord => Ok(None),
            LoadActionResult::NoSubkey { descriptor } => Ok(Some(GetResult {
                opt_value: None,
                opt_descriptor: if want_descriptor {
                    Some(descriptor)
                } else {
                    None
                },
            })),
            LoadActionResult::Subkey {
                descriptor,
                mut load_action,
            } => {
                let res = load_action.load().await;
                {
                    let mut inner = self.inner.lock();
                    inner.finish_load_action(load_action);
                }
                let opt_value = res?.map(|x| x.signed_value_data());

                Ok(Some(GetResult {
                    opt_value,
                    opt_descriptor: if want_descriptor {
                        Some(descriptor)
                    } else {
                        None
                    },
                }))
            }
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub async fn peek_subkey(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
        want_descriptor: bool,
    ) -> VeilidAPIResult<Option<GetResult>> {
        let _subkey_lock = self
            .record_store_lock_table
            .lock_subkey(
                opaque_record_key.clone(),
                subkey,
                RecordStoreSubkeyLockPurpose::Peek,
            )
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::peek_subkey lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        let load_action_result = {
            let mut inner = self.inner.lock();
            inner.prepare_peek_load_action(opaque_record_key, subkey)?
        };

        match load_action_result {
            LoadActionResult::NoRecord => Ok(None),
            LoadActionResult::NoSubkey { descriptor } => Ok(Some(GetResult {
                opt_value: None,
                opt_descriptor: if want_descriptor {
                    Some(descriptor)
                } else {
                    None
                },
            })),
            LoadActionResult::Subkey {
                descriptor,
                mut load_action,
            } => {
                let res = load_action.load().await;
                {
                    let mut inner = self.inner.lock();
                    inner.finish_load_action(load_action);
                }
                let opt_value = res?.map(|x| x.signed_value_data());

                Ok(Some(GetResult {
                    opt_value,
                    opt_descriptor: if want_descriptor {
                        Some(descriptor)
                    } else {
                        None
                    },
                }))
            }
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub async fn set_single_subkey(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
        value: Arc<SignedValueData>,
        watch_update_mode: InboundWatchUpdateMode,
        commit_action_flush_mode: CommitActionFlushMode,
    ) -> VeilidAPIResult<()> {
        let _subkey_lock = self
            .record_store_lock_table
            .lock_subkey(
                opaque_record_key.clone(),
                subkey,
                RecordStoreSubkeyLockPurpose::Set,
            )
            .await;

        let opt_commit_action = self.inner.lock().set_single_subkey(
            opaque_record_key,
            subkey,
            value,
            &watch_update_mode,
            commit_action_flush_mode,
        )?;

        if let Some(commit_action) = opt_commit_action {
            self.process_commit_action(commit_action).await?;
        };

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub async fn set_subkeys_single_record(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkey_values: &SubkeyValueList,
        watch_update_mode: InboundWatchUpdateMode,
        commit_action_flush_mode: CommitActionFlushMode,
    ) -> VeilidAPIResult<()> {
        let _record_lock = self
            .record_store_lock_table
            .lock_record(opaque_record_key.clone(), RecordStoreRecordLockPurpose::Set)
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::set_subkeys_single_record lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        let opt_commit_action = self.inner.lock().set_subkeys_single_record(
            opaque_record_key,
            subkey_values,
            &watch_update_mode,
            commit_action_flush_mode,
        )?;

        if let Some(commit_action) = opt_commit_action {
            self.process_commit_action(commit_action).await?;
        };

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub async fn set_subkeys_multiple_records(
        &self,
        keys_and_subkeys: &RecordSubkeyValueList,
        watch_update_mode: InboundWatchUpdateMode,
        commit_action_flush_mode: CommitActionFlushMode,
    ) -> VeilidAPIResult<()> {
        let _records_lock = self
            .record_store_lock_table
            .lock_records(
                keys_and_subkeys.iter().map(|x| x.0.clone()).collect(),
                RecordStoreRecordLockPurpose::Set,
            )
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::set_subkeys_multiple_records lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        let opt_commit_action = self.inner.lock().set_subkeys_multiple_records(
            keys_and_subkeys,
            &watch_update_mode,
            commit_action_flush_mode,
        )?;

        if let Some(commit_action) = opt_commit_action {
            self.process_commit_action(commit_action).await?;
        };

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub async fn inspect_record(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkeys: &ValueSubkeyRangeSet,
        want_descriptor: bool,
    ) -> VeilidAPIResult<Option<InspectResult>> {
        let _peek_lock = self
            .record_store_lock_table
            .peek_lock(opaque_record_key.clone())
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::inspect_record lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        let res = self.with_record(opaque_record_key, |record| {
            // Get number of subkeys from schema and ensure we are getting the
            // right number of sequence numbers betwen that and what we asked for
            let schema_subkeys = record
                .schema()
                .truncate_subkeys(subkeys, Some(DHTSchema::MAX_SUBKEY_COUNT));
            let opt_descriptor = if want_descriptor {
                Some(record.descriptor().clone())
            } else {
                None
            };

            // Check if we can return some subkeys
            if schema_subkeys.is_empty() {
                // No overlapping keys
                return Ok(None);
            }

            // Collect the requested subkey sequence numbers
            let seqs = schema_subkeys
                .iter()
                .map(|subkey| record.subkey_seq(subkey))
                .collect::<VeilidAPIResult<Vec<ValueSeqNum>>>()?;

            Ok(Some(InspectResult::new(
                self,
                subkeys.clone(),
                "inspect_record",
                schema_subkeys,
                seqs,
                opt_descriptor,
            )?))
        })?;

        match res {
            None => Ok(None),
            Some(out) => out,
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub fn peek_record<F, R>(&self, opaque_record_key: &OpaqueRecordKey, func: F) -> Option<R>
    where
        F: FnOnce(&Record<D>) -> R,
    {
        let inner = self.inner.lock();
        inner.peek_record(opaque_record_key, func)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub fn peek_lru<F, R>(&self, func: F) -> Option<R>
    where
        F: FnOnce(&OpaqueRecordKey, &Record<D>) -> R,
    {
        let inner = self.inner.lock();
        inner.peek_lru(func)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub fn with_record<F, R>(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        func: F,
    ) -> VeilidAPIResult<Option<R>>
    where
        F: FnOnce(&Record<D>) -> R,
    {
        let mut inner = self.inner.lock();
        inner.with_record(opaque_record_key, func)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub fn with_record_detail_mut<R, F>(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        func: F,
    ) -> VeilidAPIResult<Option<R>>
    where
        F: FnOnce(Arc<SignedValueDescriptor>, &mut D) -> R,
    {
        let mut inner = self.inner.lock();
        inner.with_record_detail_mut(opaque_record_key, func)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub fn lookup_inbound_transaction_id(
        &self,
        raw_id: u64,
    ) -> VeilidAPIResult<Option<InboundTransactionId>> {
        self.inner.lock().lookup_inbound_transaction_id(raw_id)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "debug", target = "stor", skip_all, err)
    )]
    pub async fn begin_inbound_transaction(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        opt_descriptor: Option<SignedValueDescriptor>,
        want_descriptor: bool,
        signing_member_id: MemberId,
    ) -> VeilidAPIResult<InboundTransactBeginResult> {
        let record_lock = self
            .record_store_lock_table
            .lock_record(
                opaque_record_key.clone(),
                RecordStoreRecordLockPurpose::TransactBegin,
            )
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::begin_inbound_transaction lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        // prepare
        let begin_res = self.inner.lock().prepare_begin_inbound_transaction(
            opaque_record_key,
            opt_descriptor,
            want_descriptor,
            signing_member_id,
        )?;

        let begin_context = match begin_res {
            PrepareBeginInboundTransactionResult::Done(inbound_transact_begin_result) => {
                return Ok(inbound_transact_begin_result);
            }
            PrepareBeginInboundTransactionResult::Continue(
                prepare_begin_inbound_transaction_context,
            ) => prepare_begin_inbound_transaction_context,
        };

        // Transaction can be added, so let's get a snapshot if the record exists already
        let opt_snapshot = self.snapshot_record_locked(&record_lock).await?;

        // finish
        self.inner
            .lock()
            .finish_begin_inbound_transaction(begin_context, opt_snapshot)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "debug", target = "stor", skip_all, err)
    )]
    pub async fn end_inbound_transaction(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        transaction_id: InboundTransactionId,
    ) -> VeilidAPIResult<InboundTransactCommandResult> {
        let record_lock = self
            .record_store_lock_table
            .lock_record(
                opaque_record_key.clone(),
                RecordStoreRecordLockPurpose::TransactEnd,
            )
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::end_inbound_transaction lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        // prepare
        let end_res = self
            .inner
            .lock()
            .prepare_end_inbound_transaction(opaque_record_key, transaction_id)?;

        let end_context = match end_res {
            PrepareEndInboundTransactionResult::Done(inbound_transact_end_result) => {
                return Ok(inbound_transact_end_result);
            }
            PrepareEndInboundTransactionResult::Continue(
                prepare_end_inbound_transaction_context,
            ) => prepare_end_inbound_transaction_context,
        };

        // snapshot
        let res_opt_end_snapshot = self.snapshot_record_locked(&record_lock).await;

        // finish
        self.inner
            .lock()
            .finish_end_inbound_transaction(end_context, res_opt_end_snapshot)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "debug", target = "stor", skip_all, err)
    )]
    pub async fn commit_inbound_transaction<C: FnOnce() -> D>(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        transaction_id: InboundTransactionId,
        make_record_detail: C,
    ) -> VeilidAPIResult<InboundTransactCommandResult> {
        let _record_lock = self
            .record_store_lock_table
            .lock_record(
                opaque_record_key.clone(),
                RecordStoreRecordLockPurpose::TransactCommit,
            )
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::commit_inbound_transaction lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        self.inner.lock().commit_inbound_transaction(
            opaque_record_key,
            transaction_id,
            make_record_detail,
        )
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "debug", target = "stor", skip_all, err)
    )]
    pub async fn rollback_inbound_transaction(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        transaction_id: InboundTransactionId,
    ) -> VeilidAPIResult<InboundTransactCommandResult> {
        let _record_lock = self
            .record_store_lock_table
            .lock_record(
                opaque_record_key.clone(),
                RecordStoreRecordLockPurpose::TransactRollback,
            )
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::rollback_inbound_transaction lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        self.inner
            .lock()
            .rollback_inbound_transaction(opaque_record_key, transaction_id)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "debug", target = "stor", skip_all, err)
    )]
    pub async fn inbound_transaction_get(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        transaction_id: InboundTransactionId,
        opt_subkey: Option<ValueSubkey>,
    ) -> VeilidAPIResult<InboundTransactCommandResult> {
        let _record_lock = self
            .record_store_lock_table
            .lock_record(
                opaque_record_key.clone(),
                RecordStoreRecordLockPurpose::TransactGet,
            )
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::inbound_transaction_get lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        self.inner
            .lock()
            .inbound_transaction_get(opaque_record_key, transaction_id, opt_subkey)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "debug", target = "stor", skip_all, err)
    )]
    pub async fn inbound_transaction_set(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        transaction_id: InboundTransactionId,
        subkey: ValueSubkey,
        value: Arc<SignedValueData>,
    ) -> VeilidAPIResult<InboundTransactCommandResult> {
        let _record_lock = self
            .record_store_lock_table
            .lock_record(
                opaque_record_key.clone(),
                RecordStoreRecordLockPurpose::TransactSet,
            )
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "RecordStore({})::inbound_transaction_set lock",
                    self.unlocked_inner.name
                ),
            )
            .await;

        self.inner
            .lock()
            .inbound_transaction_set(opaque_record_key, transaction_id, subkey, value)
    }

    /// See if any inbound transactions have expired and clear them out
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub fn drop_expired_inbound_transactions(&self) {
        self.inner.lock().drop_expired_inbound_transactions();
    }

    pub fn total_storage_space(&self) -> u64 {
        self.inner.lock().total_storage_space()
    }

    //////////////////////////////////////////////////////////////////////////////////

    async fn process_commit_action(
        &self,
        mut commit_action: CommitAction<D>,
    ) -> VeilidAPIResult<()> {
        let res = commit_action.commit().await;

        if let Err(e) = res.as_ref() {
            veilid_log!(self error "Error committing record index: {}\n{:#?}", e, commit_action);
        }

        let mut inner = self.inner.lock();
        inner.finish_commit_action(commit_action);

        res
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub(super) fn contains_record(&self, opaque_record_key: &OpaqueRecordKey) -> bool {
        self.inner
            .lock()
            .peek_record(opaque_record_key, |_| true)
            .unwrap_or_default()
    }
}
