use super::*;
use futures_util::{stream::unfold, *};
use stop_token::future::FutureExt as _;

impl_veilid_log_facility!("stor");

#[derive(Debug)]
enum OfflineSubkeyWriteResult {
    Finished(set_value::OutboundSetValueResult),
    Cancelled,
    Dropped,
}

#[derive(Debug)]
struct WorkItem {
    opaque_record_key: OpaqueRecordKey,
    safety_selection: SafetySelection,
    subkeys: ValueSubkeyRangeSet,
}

#[derive(Debug)]
struct WorkItemResult {
    work_item: WorkItem,
    written_subkeys: ValueSubkeyRangeSet,
    fanout_results: Vec<(ValueSubkeyRangeSet, FanoutResult)>,
}

impl StorageManager {
    // Write a single offline subkey
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    async fn write_single_offline_subkey(
        &self,
        stop_token: StopToken,
        subkey_lock: &StorageManagerSubkeyLockGuard,
        safety_selection: SafetySelection,
    ) -> EyreResult<OfflineSubkeyWriteResult> {
        let opaque_record_key = subkey_lock.record();
        let subkey = subkey_lock.subkey();

        if !self.dht_is_online() {
            // Cancel this operation because we're offline
            return Ok(OfflineSubkeyWriteResult::Cancelled);
        };
        let get_result = self
            .handle_get_single_local_value(&opaque_record_key, subkey, true)
            .await;
        let Ok(get_result) = get_result else {
            veilid_log!(self debug "Offline subkey write had no subkey result: {}:{}", opaque_record_key, subkey);
            // drop this one
            return Ok(OfflineSubkeyWriteResult::Dropped);
        };
        let Some(value) = get_result.opt_value else {
            veilid_log!(self debug "Offline subkey write had no subkey value: {}:{}", opaque_record_key, subkey);
            // drop this one
            return Ok(OfflineSubkeyWriteResult::Dropped);
        };
        let Some(descriptor) = get_result.opt_descriptor else {
            veilid_log!(self debug "Offline subkey write had no descriptor: {}:{}", opaque_record_key, subkey);
            return Ok(OfflineSubkeyWriteResult::Dropped);
        };
        veilid_log!(self debug "Offline subkey write: {}:{} len={}", opaque_record_key, subkey, value.value_data().data().len());
        let osvres = self
            .outbound_set_value(
                &opaque_record_key,
                subkey,
                safety_selection,
                value.clone(),
                descriptor,
            )
            .timeout_at(stop_token.clone())
            .await;
        match osvres {
            Ok(Ok(result)) => {
                // Defensive: outbound_set_value no longer emits partials, but
                // skip them if reintroduced for some fanout in the future.
                if result.fanout_result.kind.is_incomplete() {
                    veilid_log!(self debug "Offline subkey write got partial result: {}:{}", opaque_record_key, subkey);
                    return Ok(OfflineSubkeyWriteResult::Cancelled);
                }

                // Set the new value if it differs from what was asked to set
                if result.signed_value_data.value_data() != value.value_data() {
                    // Record the newer value and send and update since it is different than what we just set
                    self.handle_set_single_local_value_with_subkey_lock(
                        subkey_lock,
                        result.signed_value_data.clone(),
                    )
                    .await?;
                }

                Ok(OfflineSubkeyWriteResult::Finished(result))
            }
            Ok(Err(e)) => {
                veilid_log!(self debug "failed to write offline subkey: {}:{} {}", opaque_record_key, subkey, e);
                Ok(OfflineSubkeyWriteResult::Cancelled)
            }
            Err(_) => {
                veilid_log!(self debug "writing offline subkey did not complete {}:{}", opaque_record_key, subkey);
                Ok(OfflineSubkeyWriteResult::Cancelled)
            }
        }
    }

    // Write a set of subkeys of the same key
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    async fn process_work_item(
        &self,
        stop_token: StopToken,
        work_item: WorkItem,
    ) -> EyreResult<WorkItemResult> {
        let mut written_subkeys = ValueSubkeyRangeSet::new();
        let mut fanout_results = Vec::<(ValueSubkeyRangeSet, FanoutResult)>::new();

        for subkey in work_item.subkeys.iter() {
            if poll!(stop_token.clone()).is_ready() {
                break;
            }

            let subkey_lock = self
                .record_lock_table
                .lock_subkey(
                    work_item.opaque_record_key.clone(),
                    subkey,
                    StorageManagerSubkeyLockPurpose::Set,
                )
                .await;

            let result = match self
                .write_single_offline_subkey(
                    stop_token.clone(),
                    &subkey_lock,
                    work_item.safety_selection.clone(),
                )
                .await?
            {
                OfflineSubkeyWriteResult::Finished(r) => r,
                OfflineSubkeyWriteResult::Cancelled => {
                    // Stop now and return what we have
                    break;
                }
                OfflineSubkeyWriteResult::Dropped => {
                    // Don't process this item any more but continue
                    written_subkeys.insert(subkey);
                    continue;
                }
            };

            // Process non-partial setvalue result
            let was_offline = self.check_fanout_finished_without_consensus(
                &work_item.opaque_record_key,
                subkey,
                &result.fanout_result,
            );
            if !was_offline {
                written_subkeys.insert(subkey);
            }
            fanout_results.push((ValueSubkeyRangeSet::single(subkey), result.fanout_result));
        }

        Ok(WorkItemResult {
            work_item,
            written_subkeys,
            fanout_results,
        })
    }

    // Process all results
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    fn process_single_result(&self, result: WorkItemResult) {
        let consensus_width = self.config().internal().network.dht.consensus_width as usize;

        // Debug print the result
        veilid_log!(self debug "Offline write result: {:?}", result);

        // Mark the offline subkey write as no longer in-flight
        let subkeys_still_offline = result.work_item.subkeys.difference(&result.written_subkeys);
        self.finish_offline_subkey_writes(
            &result.work_item.opaque_record_key,
            result.written_subkeys,
            subkeys_still_offline,
        );

        // Keep the list of nodes that returned a value for later reference
        let existed = match self.process_fanout_results(
            result.work_item.opaque_record_key.clone(),
            result.fanout_results.into_iter().map(|x| (x.0, x.1)),
            true,
            consensus_width,
        ) {
            Ok(v) => v,
            Err(e) => {
                veilid_log!(self error "Error processing fanout results for offline subkey write: {}", e);
                return;
            }
        };

        if !existed {
            veilid_log!(self debug "Offline subkey write succeeded but local record was deleted: {}", result.work_item.opaque_record_key);
        }
    }

    // Get the next available work item
    fn get_next_work_item(&self) -> Option<WorkItem> {
        let mut inner = self.inner.lock();

        // Find first offline subkey write record
        // That doesn't have the maximum number of concurrent
        // in-flight subkeys right now
        for (opaque_record_key, osw) in &mut inner.offline_subkey_writes {
            if osw.subkeys_in_flight.len() < OFFLINE_SUBKEY_WRITES_SUBKEY_CHUNK_SIZE {
                // Get first subkey to process that is not already in-flight
                for sk in osw.subkeys.iter() {
                    if !osw.subkeys_in_flight.contains(sk) {
                        // Found a not-yet-in-flight subkey, move it to in-flight
                        osw.subkeys.remove(sk);
                        osw.subkeys_in_flight.insert(sk);
                        // And return a work item for it
                        return Some(WorkItem {
                            opaque_record_key: opaque_record_key.clone(),
                            safety_selection: osw.safety_selection.clone(),
                            subkeys: ValueSubkeyRangeSet::single(sk),
                        });
                    }
                }
            }
        }

        None
    }

    // Best-effort write subkeys to the network that were written offline
    //#[cfg_attr(feature = "instrument", instrument(level = "trace", target = "stor", skip_all, err))]
    pub(super) async fn offline_subkey_writes_task_routine(
        &self,
        stop_token: StopToken,
        _last_ts: Timestamp,
        _cur_ts: Timestamp,
    ) -> EyreResult<()> {
        // Produce WorkItems
        let work_item_stream = unfold((), |_| {
            let registry = self.registry();
            {
                async move {
                    let storage_manager = registry.storage_manager();
                    storage_manager.get_next_work_item().map(|x| (x, ()))
                }
            }
        });

        // WorkItem -> Work Futures
        let work_future_stream = {
            let stop_token = stop_token.clone();
            work_item_stream.map(move |work_item| {
                let stop_token = stop_token.clone();
                async move {
                    let res = self.process_work_item(stop_token.clone(), work_item).await;
                    let result = match res {
                        Ok(v) => v,
                        Err(e) => {
                            veilid_log!(self debug "Offline subkey write failed: {}", e);
                            return;
                        }
                    };
                    self.process_single_result(result);
                }
            })
        };

        // Batched parallel processed Work Futures
        process_batched_future_stream_void(
            work_future_stream,
            OFFLINE_SUBKEY_WRITES_BATCH_SIZE,
            stop_token,
        )
        .await;

        // Ensure nothing is left in-flight when returning even due to an error
        {
            let mut inner = self.inner.lock();
            inner.offline_subkey_writes.retain(|_, v| {
                v.subkeys = v.subkeys.union(&mem::take(&mut v.subkeys_in_flight));
                !v.subkeys.is_empty()
            });
            let mut waiters = mem::take(&mut inner.flush_record_waiters);
            waiters.retain(|k, _| inner.offline_subkey_writes.contains_key(k));
            inner.flush_record_waiters = waiters;
        }

        Ok(())
    }
}
