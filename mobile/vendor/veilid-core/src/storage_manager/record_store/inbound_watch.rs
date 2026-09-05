use super::*;

/// How a watch gets updated when a value changes
#[derive(Debug)]
pub(crate) enum InboundWatchUpdateMode {
    /// Update no watchers
    NoUpdate,
    /// Update all watchers
    UpdateAll,
    /// Update all watchers except ones that come from a specific target
    ExcludeTarget(Target),
}

impl<D> RecordStore<D>
where
    D: RecordDetail,
{
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub fn lookup_inbound_watch_id(&self, raw_id: u64) -> VeilidAPIResult<Option<InboundWatchId>> {
        self.inner.lock().lookup_inbound_watch_id(raw_id)
    }

    /// Add or update an inbound record watch for changes
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub async fn watch_record(
        &self,
        opaque_record_key: OpaqueRecordKey,
        mut params: InboundWatchParameters,
        opt_watch_id: Option<InboundWatchId>,
    ) -> VeilidAPIResult<InboundWatchValueResult> {
        let _record_lock = self
            .record_store_lock_table
            .lock_record(
                opaque_record_key.clone(),
                RecordStoreRecordLockPurpose::Watch,
            )
            .await;

        // If record doesn't exist, reject immediately, otherwise get the schema and owner
        let Some((schema, owner)) = self.with_record(&opaque_record_key, |record| {
            let schema = record.schema();
            let owner = record.owner();
            (schema, owner)
        })?
        else {
            // Record not found
            return Ok(InboundWatchValueResult::Rejected);
        };

        // If count is zero then we're cancelling a watch completely
        if params.count == 0 {
            if let Some(watch_id) = opt_watch_id {
                let cancelled = self
                    .inner
                    .lock()
                    .cancel_watch(opaque_record_key.clone(), watch_id)?;
                if cancelled {
                    return Ok(InboundWatchValueResult::Cancelled);
                }
                return Ok(InboundWatchValueResult::Rejected);
            }
            apibail_internal!("shouldn't have let a None watch id get here");
        }

        // See if expiration timestamp is too far in the future or not enough in the future
        let cur_ts = Timestamp::now_non_decreasing();
        let max_ts = cur_ts.later(self.unlocked_inner.limits.max_watch_expiration);
        let min_ts = cur_ts.later(self.unlocked_inner.limits.min_watch_expiration);

        if params.expiration.as_u64() == 0 || params.expiration > max_ts {
            // Clamp expiration max time (or set zero expiration to max)
            params.expiration = max_ts;
        } else if params.expiration < min_ts {
            // Don't add watches with too low of an expiration time
            if let Some(watch_id) = opt_watch_id {
                let cancelled = self
                    .inner
                    .lock()
                    .cancel_watch(opaque_record_key, watch_id)?;
                if cancelled {
                    return Ok(InboundWatchValueResult::Cancelled);
                }
            }
            return Ok(InboundWatchValueResult::Rejected);
        }

        // Make a closure to check for member vs anonymous
        let owner_member_id = self.storage_manager().generate_member_id(&owner)?;
        let member_check = Box::new(move |watcher: &MemberId| {
            owner_member_id == *watcher || schema.is_member(watcher.ref_value())
        });

        // Create or update depending on if a watch id is specified or not
        if let Some(watch_id) = opt_watch_id {
            self.inner
                .lock()
                .change_existing_watch(&opaque_record_key, params, watch_id)
        } else {
            self.inner
                .lock()
                .create_new_watch(&opaque_record_key, params, member_check)
        }
    }

    /// See if any watched records have expired and clear them out
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub fn drop_expired_inbound_watches(&self) {
        self.inner.lock().drop_expired_inbound_watches();
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub async fn take_value_changes(&self) -> Vec<ValueChangedInfo> {
        let mut changes = vec![];

        let evcis = self.inner.lock().take_value_changes();
        for evci in evcis {
            // Get a single subkey data if we can send it
            let value = if evci.subkeys.len() == 1 {
                let Some(first_subkey) = evci.subkeys.first() else {
                    veilid_log!(self error "first subkey should exist for value change notification");
                    continue;
                };
                let get_result = match self.get_subkey(&evci.key, first_subkey, false).await {
                    Ok(Some(skr)) => skr,
                    Ok(None) => {
                        veilid_log!(self error "subkey should have data for value change notification");
                        continue;
                    }
                    Err(e) => {
                        veilid_log!(self error "error getting subkey data for value change notification: {}", e);
                        continue;
                    }
                };
                let Some(value) = get_result.opt_value else {
                    veilid_log!(self error "first subkey should have had value for value change notification");
                    continue;
                };
                Some(value)
            } else {
                None
            };

            changes.push(ValueChangedInfo {
                target: evci.target,
                record_key: evci.key,
                subkeys: evci.subkeys,
                count: evci.count,
                watch_id: evci.watch_id,
                value,
            });
        }

        changes
    }
}
