use super::*;

impl StorageManager {
    /// Check active watches for missing ValueChanged notifications.
    /// If no ValueChanged has arrived within WATCH_FALLBACK_INSPECT_INTERVAL,
    /// enqueue a change inspection. If that inspection finds changes,
    /// it will set needs_forced_reconcile to re-establish the watch.
    fn check_fallback_inspections_inner(
        registry: VeilidComponentRegistry,
        inner: &mut StorageManagerInner,
        cur_ts: Timestamp,
    ) {
        // Collect watches needing fallback inspection
        let inspections: Vec<_> = inner
            .outbound_watch_manager
            .outbound_watches
            .values()
            .filter_map(|ow| {
                let state = ow.state()?;
                if state.needs_fallback_inspect(cur_ts) {
                    Some((ow.record_key(), state.params().subkeys.clone()))
                } else {
                    None
                }
            })
            .collect();

        if inspections.is_empty() {
            return;
        }

        // Enqueue inspections and update timestamps
        for (record_key, subkeys) in inspections {
            veilid_log!(registry debug target: "watch",
                "Fallback inspection triggered for watch: {}",
                record_key
            );
            inner
                .outbound_watch_manager
                .enqueue_change_inspect(record_key.clone(), subkeys);

            let owm = &mut inner.outbound_watch_manager;
            if let Some(ow) = owm.outbound_watches.get_mut(&record_key.opaque()) {
                if let Some(state) = ow.state_mut() {
                    state.edit(&owm.per_node_states, |editor| {
                        editor.touch_fallback_inspect(cur_ts);
                    });
                }
            }
        }
    }

    // Check if client-side watches on opened records either have dead nodes or if the watch has expired
    pub(super) fn check_outbound_watches_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        cur_ts: Timestamp,
    ) -> EyreResult<()> {
        let mut inner = self.inner.lock();
        let Some(local_record_store) = inner.local_record_store.as_ref().cloned() else {
            // No local record store, so no watches to check
            return Ok(());
        };

        // Update per-node watch states
        // Desired state updates are performed by get_next_outbound_watch_operation
        inner.outbound_watch_manager.update_per_node_states(cur_ts);

        // Check for watches that need fallback inspection
        Self::check_fallback_inspections_inner(self.registry(), &mut inner, cur_ts);

        // Iterate all outbound watches and determine what work needs doing if any
        for outbound_watch in inner.outbound_watch_manager.outbound_watches.values_mut() {
            // Operate on this watch only if it isn't already being operated on.
            // Watch mode (lifetime.read + watch_lock) is concurrent with reads/sets/transactions.
            if let Some(record_lock) = self.record_lock_table.try_lock_record(
                outbound_watch.record_key().opaque(),
                StorageManagerRecordLockPurpose::Watch,
            ) {
                // Get next work on watch and queue it if we have something to do
                if let Some(op_fut) = Self::get_next_outbound_watch_operation_inner(
                    self.registry(),
                    record_lock,
                    cur_ts,
                    outbound_watch,
                ) {
                    self.background_operation_processor.add_future(op_fut);
                };
            }
        }

        // Run queued change inspections that are due, leaving backed-off retries queued
        let nci = &mut inner.outbound_watch_manager.needs_change_inspection;
        let due_keys = nci
            .iter()
            .filter(|(_, (_, not_before))| *not_before <= cur_ts)
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>();
        for k in due_keys {
            let Some((subkeys, _)) = nci.remove(&k) else {
                continue;
            };
            let op_fut = Self::get_change_inspection_operation_inner(
                self.registry(),
                local_record_store.clone(),
                k,
                subkeys,
            );
            self.background_operation_processor.add_future(op_fut);
        }

        Ok(())
    }
}
