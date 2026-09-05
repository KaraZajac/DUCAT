use super::*;

impl StorageManager {
    pub fn debug_local_records(&self) -> String {
        let Ok(local_record_store) = self.get_local_record_store() else {
            return "not initialized".to_owned();
        };
        local_record_store.debug_records()
    }
    pub fn debug_remote_records(&self) -> String {
        let Ok(remote_record_store) = self.get_remote_record_store() else {
            return "not initialized".to_owned();
        };
        remote_record_store.debug_records()
    }
    pub fn debug_opened_records(&self) -> String {
        let inner = self.inner.lock();
        let mut out = "[\n".to_owned();
        for (k, v) in &inner.opened_records {
            let writer = if let Some(w) = v.writer() {
                w.to_string()
            } else {
                "".to_owned()
            };
            let encryption_key = if let Some(e) = v.encryption_key() {
                format!(":{}", e)
            } else {
                "".to_owned()
            };
            out += &format!("  {}{} {}\n", k, encryption_key, writer);
        }
        format!("{}]\n", out)
    }
    pub fn debug_watched_records(&self) -> String {
        let inner = self.inner.lock();
        inner.outbound_watch_manager.to_string()
    }
    pub fn debug_transactions(&self) -> String {
        let inner = self.inner.lock();
        inner.outbound_transaction_manager.to_string()
    }

    pub fn debug_offline_records(&self) -> String {
        let inner = self.inner.lock();
        let Some(local_record_store) = inner.local_record_store.clone() else {
            return "not initialized".to_owned();
        };

        let mut out = "[\n".to_owned();
        for (k, v) in &inner.offline_subkey_writes {
            let record_info = local_record_store
                .peek_record(k, |r| format!("{} nodes", r.detail().nodes.len()))
                .unwrap_or("Not found".to_owned());

            out += &format!("  {}:{:?}, {}\n", k, v, record_info);
        }
        format!("{}]\n", out)
    }

    pub async fn purge_local_records_by_bytes(&self, reclaim: Option<u64>) -> String {
        let local_record_store = {
            let inner = self.inner.lock();
            let Some(local_record_store) = inner.local_record_store.clone() else {
                return "not initialized".to_owned();
            };
            if !inner.opened_records.is_empty() {
                return "records still opened".to_owned();
            }

            local_record_store
        };

        let starting_space = local_record_store.total_storage_space();
        let desired_space = if let Some(reclaim) = reclaim {
            starting_space.saturating_sub(reclaim)
        } else {
            0u64
        };

        let mut total_space = local_record_store.total_storage_space();

        loop {
            if total_space <= desired_space {
                break;
            }

            let Some(opaque_record_key) = local_record_store.peek_lru(|key, _| key.clone()) else {
                break;
            };

            let records_lock = self
                .record_lock_table
                .lock_records(
                    vec![opaque_record_key.clone()],
                    StorageManagerRecordLockPurpose::Delete,
                )
                .await;
            veilid_log!(self debug "Purging local record {}: total={} desired={}", opaque_record_key, total_space, desired_space);
            match local_record_store.delete_record(&opaque_record_key).await {
                Ok(_) => total_space = local_record_store.total_storage_space(),
                Err(VeilidAPIError::KeyNotFound { key }) => {
                    total_space = local_record_store.total_storage_space();
                    veilid_log!(self debug "Record not found: {}", key);
                    break;
                }
                Err(e) => {
                    total_space = local_record_store.total_storage_space();
                    veilid_log!(self error "Error deleting record {}: {}", opaque_record_key, e);
                    break;
                }
            }

            if let Err(e) = self.cleanup_records_locked(&records_lock) {
                veilid_log!(self error "Error cleaning up record in local purge {}: {}", opaque_record_key, e);
                total_space = local_record_store.total_storage_space();
                break;
            }
        }

        let msg = format!(
            "Local records purged: purged {} bytes, now {} bytes total",
            starting_space - total_space,
            total_space
        );

        veilid_log!(self debug "{}", msg);

        msg
    }

    pub async fn purge_remote_records_by_bytes(&self, reclaim: Option<u64>) -> String {
        let remote_record_store = {
            let inner = self.inner.lock();
            let Some(remote_record_store) = inner.remote_record_store.clone() else {
                return "not initialized".to_owned();
            };
            remote_record_store
        };

        let starting_space = remote_record_store.total_storage_space();
        let desired_space = if let Some(reclaim) = reclaim {
            starting_space.saturating_sub(reclaim)
        } else {
            0u64
        };

        let mut total_space = remote_record_store.total_storage_space();

        loop {
            if total_space <= desired_space {
                break;
            }

            let Some(opaque_record_key) = remote_record_store.peek_lru(|key, _| key.clone()) else {
                break;
            };

            veilid_log!(self debug "Purging remote record {}: total={} desired={}", opaque_record_key, total_space, desired_space);
            match remote_record_store.delete_record(&opaque_record_key).await {
                Ok(_) => total_space = remote_record_store.total_storage_space(),
                Err(VeilidAPIError::KeyNotFound { key }) => {
                    veilid_log!(self debug "Record not found: {}", key);
                    total_space = remote_record_store.total_storage_space();
                    break;
                }
                Err(e) => {
                    total_space = remote_record_store.total_storage_space();
                    veilid_log!(self error "Error deleting record {}: {}", opaque_record_key, e);
                    break;
                }
            }
        }

        let msg = format!(
            "Remote records purged: purged {} bytes, now {} bytes total",
            starting_space - total_space,
            total_space
        );

        veilid_log!(self debug "{}", msg);

        msg
    }

    pub async fn purge_local_records_by_keys(&self, keys: Vec<OpaqueRecordKey>) -> String {
        let local_record_store = {
            let inner = self.inner.lock();
            let Some(local_record_store) = inner.local_record_store.clone() else {
                return "not initialized".to_owned();
            };
            if !inner.opened_records.is_empty() {
                return "records still opened".to_owned();
            }

            local_record_store
        };

        let starting_space = local_record_store.total_storage_space();
        let mut total_space = starting_space;

        let records_lock = self
            .record_lock_table
            .lock_records(keys.clone(), StorageManagerRecordLockPurpose::Delete)
            .await;
        for opaque_record_key in keys {
            veilid_log!(self debug "Purging local record {}: total={}", opaque_record_key, total_space);
            match local_record_store.delete_record(&opaque_record_key).await {
                Ok(_) => total_space = local_record_store.total_storage_space(),
                Err(VeilidAPIError::KeyNotFound { key }) => {
                    veilid_log!(self debug "Record not found: {}", key);
                }
                Err(e) => {
                    veilid_log!(self error "Error deleting record {}: {}", opaque_record_key, e);
                    break;
                }
            }
        }

        total_space = local_record_store.total_storage_space();

        if let Err(e) = self.cleanup_records_locked(&records_lock) {
            veilid_log!(self error "Error cleaning up records: {}", e);
        }

        drop(records_lock);

        let msg = format!(
            "Local records purged: purged {} bytes, now {} bytes total",
            starting_space - total_space,
            total_space
        );

        veilid_log!(self debug "{}", msg);

        msg
    }

    pub async fn purge_remote_records_by_keys(&self, keys: Vec<OpaqueRecordKey>) -> String {
        let remote_record_store = {
            let inner = self.inner.lock();
            let Some(remote_record_store) = inner.remote_record_store.clone() else {
                return "not initialized".to_owned();
            };
            if !inner.opened_records.is_empty() {
                return "records still opened".to_owned();
            }

            remote_record_store
        };

        let starting_space = remote_record_store.total_storage_space();
        let mut total_space = starting_space;

        for opaque_record_key in keys {
            veilid_log!(self debug "Purging remote record {}: total={}", opaque_record_key, total_space);
            match remote_record_store.delete_record(&opaque_record_key).await {
                Ok(_) => total_space = remote_record_store.total_storage_space(),
                Err(VeilidAPIError::KeyNotFound { key }) => {
                    veilid_log!(self debug "Record not found: {}", key);
                }
                Err(e) => {
                    veilid_log!(self error "Error deleting record {}: {}", opaque_record_key, e);
                    break;
                }
            }
        }

        total_space = remote_record_store.total_storage_space();

        let msg = format!(
            "Remote records purged: purged {} bytes, now {} bytes total",
            starting_space - total_space,
            total_space
        );

        veilid_log!(self debug "{}", msg);

        msg
    }

    pub async fn debug_local_record_subkey_info(
        &self,
        record_key: RecordKey,
        subkey: ValueSubkey,
    ) -> String {
        let local_record_store = {
            let inner = self.inner.lock();
            let Some(local_record_store) = inner.local_record_store.clone() else {
                return "not initialized".to_owned();
            };
            local_record_store
        };
        let opaque_record_key = record_key.opaque();
        local_record_store
            .debug_record_subkey_info(&opaque_record_key, subkey)
            .await
    }
    pub async fn debug_remote_record_subkey_info(
        &self,
        record_key: RecordKey,
        subkey: ValueSubkey,
    ) -> String {
        let remote_record_store = {
            let inner = self.inner.lock();
            let Some(remote_record_store) = inner.remote_record_store.clone() else {
                return "not initialized".to_owned();
            };
            remote_record_store
        };
        let opaque_record_key = record_key.opaque();
        remote_record_store
            .debug_record_subkey_info(&opaque_record_key, subkey)
            .await
    }
    pub fn debug_local_record_info(&self, record_key: RecordKey) -> String {
        let opaque_record_key = record_key.opaque();

        let (local_record_store, opened_debug) = {
            let inner = self.inner.lock();
            let Some(local_record_store) = inner.local_record_store.clone() else {
                return "not initialized".to_owned();
            };
            let opened_debug = if let Some(o) = inner.opened_records.get(&opaque_record_key) {
                format!("Opened Record: {:#?}\n", o)
            } else {
                "".to_owned()
            };

            (local_record_store, opened_debug)
        };
        let local_debug = local_record_store.debug_record_info(&opaque_record_key);

        format!("{}\n{}", local_debug, opened_debug)
    }

    pub fn debug_remote_record_info(&self, record_key: RecordKey) -> String {
        let remote_record_store = {
            let inner = self.inner.lock();
            let Some(remote_record_store) = inner.remote_record_store.clone() else {
                return "not initialized".to_owned();
            };
            remote_record_store
        };
        let opaque_record_key = record_key.opaque();
        remote_record_store.debug_record_info(&opaque_record_key)
    }
}
