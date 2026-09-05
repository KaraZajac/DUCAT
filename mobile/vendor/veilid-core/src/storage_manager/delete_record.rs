use super::*;

impl StorageManager {
    /// Delete a local record
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn delete_record(&self, record_key: RecordKey) -> VeilidAPIResult<()> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };
        let local_record_store = self.get_local_record_store()?;

        // Ensure the record is closed
        let opaque_record_key = record_key.opaque();
        let record_lock = self
            .record_lock_table
            .lock_record(
                opaque_record_key.clone(),
                StorageManagerRecordLockPurpose::Delete,
            )
            .await;

        let opt_cleanup = self.close_record_locked(&record_lock)?;
        if let Some(cleanup) = opt_cleanup {
            cleanup.await
        }

        // Remove the record from the local store
        local_record_store.delete_record(&opaque_record_key).await?;

        let record_locks: StorageManagerRecordsLockGuard = record_lock.into();

        // Clean up the record from the storage manager
        self.cleanup_records_locked(&record_locks)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) fn cleanup_records_locked(
        &self,
        records_lock: &StorageManagerRecordsLockGuard,
    ) -> VeilidAPIResult<()> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };
        let local_record_store = self.get_local_record_store()?;

        // Ensure the records are closed
        let opaque_record_keys = records_lock.records();
        let mut inner = self.inner.lock();
        for opaque_record_key in &opaque_record_keys {
            if local_record_store.contains_record(opaque_record_key) {
                apibail_internal!(
                    "can't clean up record that is still in local record store: {}",
                    opaque_record_key
                );
            }

            if inner.opened_records.contains_key(opaque_record_key) {
                apibail_internal!(
                    "can't clean up record that is still opened: {}",
                    opaque_record_key
                );
            }
        }

        let dead_records_set: HashSet<OpaqueRecordKey> = opaque_record_keys.into_iter().collect();

        inner
            .offline_subkey_writes
            .retain(|k, _| !dead_records_set.contains(k));

        inner
            .rehydration_requests
            .retain(|k, _| !dead_records_set.contains(k));

        inner
            .flush_record_waiters
            .retain(|k, _| !dead_records_set.contains(k));

        Ok(())
    }
}
