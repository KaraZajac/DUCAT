use super::*;

// ValueChangedInfo but without the subkey data that requires an async operation to get
#[derive(Debug)]
pub(in super::super) struct EarlyValueChangedInfo {
    pub target: Target,
    pub key: OpaqueRecordKey,
    pub subkeys: ValueSubkeyRangeSet,
    pub count: u32,
    pub watch_id: InboundWatchId,
}

impl<D> RecordStoreInner<D>
where
    D: RecordDetail,
{
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub fn lookup_inbound_watch_id(
        &mut self,
        raw_id: u64,
    ) -> VeilidAPIResult<Option<InboundWatchId>> {
        self.inbound_watches.lookup_id(raw_id)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub fn update_watched_value(
        &mut self,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
        watch_update_mode: &InboundWatchUpdateMode,
    ) {
        let (do_update, opt_ignore_target) = match watch_update_mode {
            InboundWatchUpdateMode::NoUpdate => (false, None),
            InboundWatchUpdateMode::UpdateAll => (true, None),
            InboundWatchUpdateMode::ExcludeTarget(target) => (true, Some(target)),
        };
        if !do_update {
            return;
        }

        let rtk = RecordTableKey {
            record_key: opaque_record_key.clone(),
        };
        let Some(inbound_watch_list) = self.inbound_watches.get_mut(&rtk) else {
            return;
        };

        // Update all watchers
        let mut changed_watched = false;
        for w in &mut inbound_watch_list.watches_mut() {
            // If this watcher is watching the changed subkey then add to the watcher's changed list
            // Don't bother marking changes for value sets coming from the same watching node/target because they
            // are already going to be aware of the changes in that case
            if Some(&w.params().target) != opt_ignore_target && w.params().subkeys.contains(subkey)
            {
                w.add_changed_subkey(subkey);
                changed_watched = true;
            }
        }
        if changed_watched {
            self.inbound_watches.insert_changed_record(rtk);
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub fn create_new_watch(
        &mut self,
        opaque_record_key: &OpaqueRecordKey,
        params: InboundWatchParameters,
        member_check: Box<dyn Fn(&MemberId) -> bool + Send>,
    ) -> VeilidAPIResult<InboundWatchValueResult> {
        // Generate a record-unique watch id > 0
        let rtk = RecordTableKey {
            record_key: opaque_record_key.clone(),
        };

        // Calculate watch limits
        let mut watch_count = 0;
        let mut target_watch_count = 0;

        let is_member = member_check(&params.watcher_member_id);

        if let Some(inbound_watch_list) = self.inbound_watches.get_mut(&rtk) {
            // Total up the number of watches for this key
            for w in &mut inbound_watch_list.watches_mut() {
                // See if this watch should be counted toward any limits
                let count_watch = if is_member {
                    // If the watcher is a member of the schema, then consider the total per-watcher key
                    w.params().watcher_member_id == params.watcher_member_id
                } else {
                    // If the watcher is not a member of the schema, the check if this watch is an anonymous watch and contributes to per-record key total
                    !member_check(&w.params().watcher_member_id)
                };

                // For any watch, if the target matches our also tally that separately
                // If the watcher is a member of the schema, then consider the total per-target-per-watcher key
                // If the watcher is not a member of the schema, then it is an anonymous watch and the total is per-target-per-record key
                if count_watch {
                    watch_count += 1;
                    if w.params().target == params.target {
                        target_watch_count += 1;
                    }
                }
            }
        }

        // For members, no more than one watch per target per watcher per record
        // For anonymous, no more than one watch per target per record
        if target_watch_count > 0 {
            // Too many watches
            return Ok(InboundWatchValueResult::Rejected);
        }

        // Check watch table for limits
        let watch_limit = if is_member {
            self.unlocked_inner.limits.member_watch_limit
        } else {
            self.unlocked_inner.limits.public_watch_limit
        };
        if watch_count >= watch_limit {
            return Ok(InboundWatchValueResult::Rejected);
        }

        // Allocate watch
        let expiration = params.expiration;
        let id = self.inbound_watches.allocate(opaque_record_key, params)?;

        Ok(InboundWatchValueResult::Created { id, expiration })
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub fn change_existing_watch(
        &mut self,
        opaque_record_key: &OpaqueRecordKey,
        params: InboundWatchParameters,
        watch_id: InboundWatchId,
    ) -> VeilidAPIResult<InboundWatchValueResult> {
        if params.count == 0 {
            apibail_internal!("cancel watch should not have gotten here");
        }
        if params.expiration.as_u64() == 0 {
            apibail_internal!("zero expiration should have been resolved to max by now");
        }
        // Get the watch list for this record
        let rtk = RecordTableKey {
            record_key: opaque_record_key.clone(),
        };
        let Some(watch_list) = self.inbound_watches.get_mut(&rtk) else {
            // No watches, nothing to change
            return Ok(InboundWatchValueResult::Rejected);
        };

        // Check each watch to see if we have an exact match for the id to change
        if let Some(w) = watch_list.get_mut(watch_id) {
            // If the watch id doesn't match, then we're not updating
            // Also do not allow the watcher key to change
            if w.params().watcher_member_id == params.watcher_member_id {
                // Updating an existing watch
                w.update_params(params);
                return Ok(InboundWatchValueResult::Changed {
                    expiration: w.params().expiration,
                });
            }
        }

        // No existing watch found
        Ok(InboundWatchValueResult::Rejected)
    }

    /// Clear a specific watch for a record
    /// returns true if the watch was found and cancelled
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub fn cancel_watch(
        &mut self,
        record_key: OpaqueRecordKey,
        watch_id: InboundWatchId,
    ) -> VeilidAPIResult<bool> {
        // See if we are cancelling an existing watch
        let rtk = RecordTableKey { record_key };

        if !self.inbound_watches.check_id(watch_id, &rtk) {
            return Ok(false);
        }

        self.inbound_watches.remove_watch(watch_id)?;

        Ok(true)
    }

    /// See if any watched records have expired and clear them out
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub fn drop_expired_inbound_watches(&mut self) {
        let now = Timestamp::now_non_decreasing();
        let registry = self.registry();
        let debug_logger = |id| {
            veilid_log!(registry debug "dropped expired watch: {}", id);
        };
        let error_logger = |e| {
            veilid_log!(registry error "error in drop_expired_inbound_watches: {}", e);
        };

        self.inbound_watches
            .remove_expired_watches(now, &debug_logger, &error_logger);
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub fn take_value_changes(&mut self) -> Vec<EarlyValueChangedInfo> {
        let changed_records = self.inbound_watches.take_changed_records();

        let mut evcis = vec![];
        let mut dead_watch_ids = vec![];
        for rtk in changed_records {
            if let Some(inbound_watch_list) = self.inbound_watches.get_mut(&rtk) {
                // Process watch notifications
                for w in inbound_watch_list.watches_mut() {
                    // Get the subkeys that have changed
                    let subkeys = w.take_changed_subkeys();

                    // If no subkeys on this watcher have changed then skip it
                    if subkeys.is_empty() {
                        continue;
                    }

                    // Reduce the count of changes sent
                    // if count goes to zero mark this watcher dead
                    let params = w.params();
                    let id = w.id();
                    let target = params.target.clone();

                    let count = params.count.saturating_sub(1);
                    w.update_count(count);

                    if count == 0 {
                        dead_watch_ids.push(id);
                    }

                    evcis.push(EarlyValueChangedInfo {
                        target,
                        key: rtk.record_key.clone(),
                        subkeys,
                        count,
                        watch_id: id,
                    });
                }
            }
        }
        for dead_watch_id in dead_watch_ids {
            self.inbound_watches
                .remove_watch(dead_watch_id)
                .unwrap_or_else(veilid_log_err!(self));
        }

        evcis
    }
}
