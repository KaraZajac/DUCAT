use super::*;

impl StorageManager {
    /// Close an opened local record
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn close_record(&self, record_key: RecordKey) -> VeilidAPIResult<()> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let opaque_record_key = record_key.opaque();

        // Hold the record lock for the whole close so pending writes flush
        // before any subsequent open can race.
        let record_lock = self
            .record_lock_table
            .lock_record(
                opaque_record_key.clone(),
                StorageManagerRecordLockPurpose::Close,
            )
            .await;
        if let Some(cleanup) = self.close_record_locked(&record_lock)? {
            cleanup.await;
        }

        // Flush the local record store so pending writes for this record land
        // in the TableStore before the close returns.
        self.get_local_record_store()?.flush().await?;

        drop(record_lock);
        Ok(())
    }

    /// Close all opened records
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub async fn close_all_records(&self) -> VeilidAPIResult<()> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        // Release the record locks before awaiting cleanup
        let (all_cleanup, opt_error) = {
            let record_locks = {
                let keys = {
                    let inner = self.inner.lock();
                    inner.opened_records.keys().cloned().collect::<Vec<_>>()
                };

                self.record_lock_table
                    .lock_records(keys, StorageManagerRecordLockPurpose::Close)
                    .await
            };

            let mut all_cleanup: Option<TransactionCleanup> = None;
            let mut opt_error = None;
            for record_lock_guard in record_locks.record_lock_guards() {
                let res = self.close_record_locked(record_lock_guard);
                match res {
                    Ok(Some(cleanup)) => match &mut all_cleanup {
                        Some(existing) => existing.merge(cleanup),
                        None => all_cleanup = Some(cleanup),
                    },
                    Ok(None) => {}
                    Err(e) => {
                        opt_error = Some(e);
                    }
                }
            }
            (all_cleanup, opt_error)
        };

        if let Some(cleanup) = all_cleanup {
            cleanup.await;
        }

        // Flush the local record store so pending writes for the closed
        // records land in the TableStore before the close returns.
        self.get_local_record_store()?.flush().await?;

        if let Some(e) = opt_error {
            return Err(e);
        }

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////

    pub(super) fn close_record_locked(
        &self,
        record_lock: &StorageManagerRecordLockGuard,
    ) -> VeilidAPIResult<Option<TransactionCleanup>> {
        let opaque_record_key = record_lock.record();

        let local_record_store = self.get_local_record_store()?;
        if !local_record_store.contains_record(&opaque_record_key) {
            apibail_key_not_found!(opaque_record_key.clone());
        }

        let mut opt_cleanup = None;
        {
            let mut inner = self.inner.lock();
            if let Some(opened_record) = inner.opened_records.remove(&opaque_record_key) {
                let record_key = RecordKey::from_opaque(
                    opaque_record_key.clone(),
                    opened_record.encryption_key(),
                );

                // Set the watch to cancelled if we have one
                // Will process cancellation in the background
                inner
                    .outbound_watch_manager
                    .set_desired_watch(record_key, None);

                // Drop any transaction associated with the record
                if let Some(transaction_handle) = inner
                    .outbound_transaction_manager
                    .get_transaction_by_record(&opaque_record_key)
                {
                    if let Some(state) = inner
                        .outbound_transaction_manager
                        .drop_transaction(transaction_handle)
                    {
                        opt_cleanup = Some(state.into_transaction_cleanup());
                    }
                }
            }
        }

        Ok(opt_cleanup)
    }
}
