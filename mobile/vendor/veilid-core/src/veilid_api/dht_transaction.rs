use super::*;
use crate::storage_manager::OutboundTransactionHandle;

impl_veilid_log_facility!("veilid_api");

///////////////////////////////////////////////////////////////////////////////////////

/// DHT Transactions the way you perform multiple simulateous atomic operations over a set of DHT records.
///
/// DHT operations performed out of a transaction may be processed in any order, and only operate on one subkey at a time
/// for a given record. Transactions allow you to bind a set of operations so they all succeed, or fail together, and at the same time.
///
/// Transactional DHT operations can only be performed when the node is online, and will error with [VeilidAPIError::TryAgain] if offline.
///
/// Transactions must be committed when all of their operations are registered, or rolled back if the group of operations is to be cancelled.
///
/// Each transaction holds a network-side resource that the caller must release by calling [DHTTransaction::commit] or [DHTTransaction::rollback]. Dropping a [DHTTransaction] without doing either logs a warning and tears the transaction down in the background.
#[derive(Clone)]
#[must_use]
pub struct DHTTransaction {
    /// API in use
    api: VeilidAPI,
    /// Inner transaction
    inner: Arc<Mutex<DHTTransactionInner>>,
}

impl fmt::Debug for DHTTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DHTTransaction")
            .field("handle", &self.inner.lock().opt_transaction_handle)
            .finish()
    }
}

impl DHTTransaction {
    ////////////////////////////////////////////////////////////////

    pub(super) fn new(api: VeilidAPI, handle: OutboundTransactionHandle) -> VeilidAPIResult<Self> {
        let registry = api.core_context()?.registry();
        Ok(Self {
            api,
            inner: Arc::new(Mutex::new(DHTTransactionInner {
                registry,
                opt_transaction_handle: Some(handle),
            })),
        })
    }

    /// Get the [VeilidAPI] object that created this [DHTTransaction].
    pub fn api(&self) -> VeilidAPI {
        self.api.clone()
    }

    #[must_use]
    pub(crate) fn log_key(&self) -> &str {
        self.api.log_key()
    }

    /// Extend the transaction with additional record keys
    ///
    /// Blocks on a network begin fanout for the added records and requires the node to be online. Idempotent for keys already in the transaction: returns `Ok(())` without network activity if no new records would be added.
    ///
    /// Errors with [VeilidAPIError::TransactionNotFound] if the transaction handle is already completed or unknown, [VeilidAPIError::MissingArgument] if `record_keys` contains duplicates, [VeilidAPIError::InvalidArgument] if the merged record set would exceed the per-transaction record limit, and [VeilidAPIError::TryAgain] (retry) if the node is offline or the begin fanout for the added records could not reach consensus.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key(), transaction_handle), skip(self), ret))]
    pub async fn extend(
        &self,
        record_keys: Vec<RecordKey>,
        options: Option<TransactDHTRecordsOptions>,
    ) -> VeilidAPIResult<()> {
        async move {
            let storage_manager = self.api.core_context()?.storage_manager();
            let transaction_handle = {
                let inner = self.inner.lock();
                inner.opt_transaction_handle.ok_or_else(|| VeilidAPIError::transaction_not_found("transaction already completed"))?
            };
            tracing::Span::current().record("transaction_handle", transaction_handle.to_string());

            let recorder = DurationRecorder::new("DHTTransaction::extend", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](transaction_handle: {}, record_keys: {:?}, options: {:?})", name, start, transaction_handle, record_keys, options);
            });
            recorder.record_fut(
                storage_manager.extend_transaction(transaction_handle, record_keys, options),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    /// Commit the transaction
    /// All write operations are performed atomically
    ///
    /// Consumes the transaction and releases its network-side resource (the other half is [DHTTransaction::rollback]). Blocks on the end and commit consensus barriers and requires the node to be online. Completes the transaction exactly once: a second commit or a rollback errors with `transaction_not_found`.
    ///
    /// Errors with [VeilidAPIError::TransactionNotFound] if the transaction was already committed, rolled back, or is unknown, and [VeilidAPIError::TryAgain] (retry) if the node is offline or the end/commit barriers could not reach consensus.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key(), transaction_handle), skip(self), ret))]
    pub async fn commit(self) -> VeilidAPIResult<()> {
        async {
            let storage_manager = self.api.core_context()?.storage_manager();
            let transaction_handle = {
                let mut inner = self.inner.lock();
                inner.opt_transaction_handle.take().ok_or_else(|| {
                    VeilidAPIError::transaction_not_found("transaction already completed")
                })?
            };
            tracing::Span::current().record("transaction_handle", transaction_handle.to_string());

            let recorder = DurationRecorder::new("DHTTransaction::commit", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](transaction_handle: {})", name, start, transaction_handle);
            });
            recorder
                .record_fut(
                    Box::pin(storage_manager.end_and_commit_transaction(transaction_handle)),
                    |name, start, dur, ret| {
                        veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                        ret
                    },
                )
                .await
        }
        .await
        .inspect_err(log_veilid_api_error!(self))
    }

    /// Rollback the transaction
    /// No write operations are performed,
    ///
    /// Consumes the transaction and releases its network-side resource (the other half is [DHTTransaction::commit]). Blocks on sending rollbacks to the network and requires the node to be online. Completes the transaction exactly once: a second rollback or a commit errors with `transaction_not_found`.
    ///
    /// Errors with [VeilidAPIError::TransactionNotFound] if the transaction was already committed, rolled back, or is unknown, and [VeilidAPIError::TryAgain] (retry) if the node is offline.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key(), transaction_handle), skip(self), ret))]
    pub async fn rollback(self) -> VeilidAPIResult<()> {
        async {
            let storage_manager = self.api.core_context()?.storage_manager();
            let transaction_handle = {
                let mut inner = self.inner.lock();
                inner.opt_transaction_handle.take().ok_or_else(|| {
                    VeilidAPIError::transaction_not_found("transaction already completed")
                })?
            };
            tracing::Span::current().record("transaction_handle", transaction_handle.to_string());

            let recorder = DurationRecorder::new("DHTTransaction::rollback", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](transaction_handle: {})", name, start, transaction_handle);
            });
            recorder
                .record_fut(
                    Box::pin(storage_manager.rollback_transaction(transaction_handle)),
                    |name, start, dur, ret| {
                        veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                        ret
                    },
                )
                .await
        }
        .await
        .inspect_err(log_veilid_api_error!(self))
    }

    /// Add a set_dht_value operation to the transaction
    ///
    /// * Will fail if performed offline
    /// * Will fail if existing offline writes exist for this record key
    ///
    /// The writer, if specified, will override the 'default_writer' specified when the record is opened.
    ///
    /// Returns `None` if the value was successfully set.
    /// Returns `Some(data)` if the value set was older than the one available on the network.
    ///
    /// Blocks on the per-subkey lock (unbounded) and the set RPC to the transaction's node set, which retries non-responding nodes. Each per-node RPC is bounded by `network.rpc.timeout_ms`, but the lock wait and retry rounds are not, so the whole call has no single-timeout bound.
    ///
    /// Errors with [VeilidAPIError::TransactionNotFound] if the transaction handle is already completed or no longer in the Begin stage, [VeilidAPIError::InvalidArgument] if `record_key` is not open in the transaction or `subkey` is outside the schema range, [VeilidAPIError::Generic] if `record_key` is malformed (unsupported kind or bad length) or the subkey has no writer, and [VeilidAPIError::TryAgain] (retry) if the node is offline or write consensus was not reached this round. A non-responding node is retried rather than surfaced as a timeout.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key(), transaction_handle, data.len = data.len()), skip(self, data), ret))]
    pub async fn set(
        &self,
        record_key: RecordKey,
        subkey: ValueSubkey,
        data: Vec<u8>,
        options: Option<DHTTransactionSetValueOptions>,
    ) -> VeilidAPIResult<Option<ValueData>> {
        async move {
            let storage_manager = self.api.core_context()?.storage_manager();
            let transaction_handle = {
                let inner = self.inner.lock();
                inner
                    .opt_transaction_handle
                    .ok_or_else(|| VeilidAPIError::transaction_not_found("transaction already completed"))?
            };
            tracing::Span::current().record("transaction_handle", transaction_handle.to_string());
            storage_manager.check_record_key(&record_key)?;

            let data_len = data.len();
            let recorder = DurationRecorder::new("DHTTransaction::set", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](transaction_handle: {}, key: {}, subkey: {}, data: len={}, options: {:?})", name, start, transaction_handle, record_key, subkey, data_len, options);
            });
            recorder.record_fut(
                Box::pin(storage_manager.transaction_set(
                    transaction_handle,
                    record_key,
                    subkey,
                    data,
                    options,
                )),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    /// Perform a get_dht_value operation inside the transaction
    ///
    /// * Will fail if performed offline
    /// * Will pull the latest value from the network, will fail if the local value is newer
    /// * Will fail if existing offline writes exist for this record key
    ///
    /// Returns `None` if the value subkey has not yet been set.
    /// Returns `Some(data)` if the value subkey has valid data.
    ///
    /// Blocks on the per-subkey lock (unbounded) and the get RPC to the transaction's node set, which retries non-responding nodes. Each per-node RPC is bounded by `network.rpc.timeout_ms`, but the lock wait and retry rounds are not, so the whole call has no single-timeout bound.
    ///
    /// Errors with [VeilidAPIError::TransactionNotFound] if the transaction handle is already completed or no longer in the Begin stage, [VeilidAPIError::InvalidArgument] if `record_key` is not in the transaction or `subkey` is outside the schema range, [VeilidAPIError::Generic] if `record_key` is malformed (unsupported kind or bad length), and [VeilidAPIError::TryAgain] (retry) if the node is offline or the network did not return the value that existed at begin time. A non-responding node is retried rather than surfaced as a timeout.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), skip(self), ret))]
    pub async fn get(
        &self,
        record_key: RecordKey,
        subkey: ValueSubkey,
    ) -> VeilidAPIResult<Option<ValueData>> {
        async move {
            let storage_manager = self.api.core_context()?.storage_manager();
            let transaction_handle = {
                let inner = self.inner.lock();
                inner
                    .opt_transaction_handle
                    .ok_or_else(|| VeilidAPIError::transaction_not_found("transaction already completed"))?
            };
            tracing::Span::current().record("transaction_handle", transaction_handle.to_string());
            storage_manager.check_record_key(&record_key)?;

            let recorder = DurationRecorder::new("DHTTransaction::get", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](transaction_handle: {}, key: {}, subkey: {})", name, start, transaction_handle, record_key, subkey);
            });
            recorder.record_fut(
                Box::pin(storage_manager.transaction_get(transaction_handle, record_key, subkey)),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    /// Perform a inspect_dht_record operation inside the transaction
    ///
    /// * Does not perform any network activity, as the transaction state keeps all of the required information after the begin
    ///
    /// For information on arguments, see [RoutingContext::inspect_dht_record]
    ///
    /// Returns a DHTRecordReport with the subkey ranges that were returned that overlapped the schema, and sequence numbers for each of the subkeys in the range.
    ///
    /// Errors with [VeilidAPIError::TransactionNotFound] if the transaction handle is already completed, unknown, or no longer in the Begin stage (End, Commit, Rollback, or Failed), [VeilidAPIError::InvalidArgument] if `record_key` is not in the transaction, and [VeilidAPIError::Generic] if `record_key` is malformed (unsupported kind or bad length) or the transaction has not started. Performs no network activity and cannot time out.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key(), transaction_handle), skip(self), ret))]
    pub async fn inspect(
        &self,
        record_key: RecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
        scope: DHTReportScope,
    ) -> VeilidAPIResult<DHTRecordReport> {
        async move {
            let storage_manager = self.api.core_context()?.storage_manager();
            let transaction_handle = {
                let inner = self.inner.lock();
                inner
                    .opt_transaction_handle
                    .ok_or_else(|| VeilidAPIError::transaction_not_found("transaction already completed"))?
            };
            tracing::Span::current().record("transaction_handle", transaction_handle.to_string());
            storage_manager.check_record_key(&record_key)?;

            let recorder = DurationRecorder::new("DHTTransaction::inspect", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](transaction_handle: {}, record_key: {}, subkeys: {}, scope: {:?})", name, start, transaction_handle, record_key, subkeys.as_ref().map(|x| x.to_string()).unwrap_or_else(|| "None".to_string()), scope);
            });
            recorder.record(
                || storage_manager.transaction_inspect(transaction_handle, record_key, subkeys, scope),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            )
        }.await.inspect_err(log_veilid_api_error!(self))
    }
}
//////////////////////////////////////////////////////////////////////////////////////

struct DHTTransactionInner {
    registry: VeilidComponentRegistry,
    opt_transaction_handle: Option<OutboundTransactionHandle>,
}

impl Drop for DHTTransactionInner {
    fn drop(&mut self) {
        if let Some(transaction_handle) = self.opt_transaction_handle.take() {
            let registry = &self.registry;
            veilid_log!(registry warn "Dropped DHT transaction without commit or rollback");

            let storage_manager = registry.storage_manager();
            storage_manager.drop_transaction_sync(transaction_handle);
        }
    }
}
