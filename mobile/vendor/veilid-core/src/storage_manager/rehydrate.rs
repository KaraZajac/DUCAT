use super::{inspect_record::OutboundInspectValueResult, *};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RehydrateReport {
    /// The record key rehydrated
    opaque_record_key: OpaqueRecordKey,
    /// The requested range of subkeys to rehydrate if necessary
    subkeys: ValueSubkeyRangeSet,
    /// The requested consensus count,
    consensus_count: usize,
    /// The range of subkeys that actually could be rehydrated
    rehydrated: ValueSubkeyRangeSet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct RehydrationRequest {
    pub subkeys: ValueSubkeyRangeSet,
    pub consensus_count: usize,
}

impl StorageManager {
    /// Add a background rehydration request
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub fn add_rehydration_request(
        &self,
        opaque_record_key: OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        consensus_count: usize,
    ) {
        let req = RehydrationRequest {
            subkeys,
            consensus_count,
        };
        veilid_log!(self debug "Adding rehydration request: {} {:?}", opaque_record_key, req);
        let mut inner = self.inner.lock();
        inner
            .rehydration_requests
            .entry(opaque_record_key)
            .and_modify(|r| {
                r.subkeys = r.subkeys.union(&req.subkeys);
                r.consensus_count.max_assign(req.consensus_count);
            })
            .or_insert(req);
    }

    /// Sends the local copies of all of a record's subkeys back to the network
    /// Triggers a subkey update if the consensus on the subkey is less than
    /// the specified 'consensus_count'.
    /// The subkey updates are performed in the background if rehydration was
    /// determined to be necessary.
    /// If a newer copy of a subkey's data is available online, the background
    /// write will pick up the newest subkey data as it does the SetValue fanout
    /// and will drive the newest values to consensus.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(self), ret)
    )]
    pub(super) async fn rehydrate_record(
        &self,
        stop_token: StopToken,
        opaque_record_key: OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        consensus_count: usize,
    ) -> VeilidAPIResult<RehydrateReport> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let local_record_store = self.get_local_record_store()?;

        veilid_log!(self debug "Checking for record rehydration: {} {} @ consensus {}", opaque_record_key, subkeys, consensus_count);
        // Get subkey range for consideration
        let subkeys = if subkeys.is_empty() {
            ValueSubkeyRangeSet::full()
        } else {
            subkeys
        };

        // If this record participates in a committed transaction set, rehydrate the whole set
        // transactionally rather than via individual offline writes, which would break the
        // cross-record consistency the transaction established.
        let opt_membership = local_record_store
            .with_record(&opaque_record_key, |r| {
                r.detail().transaction_membership.clone()
            })?
            .flatten();
        if let Some(membership) = opt_membership {
            return self
                .rehydrate_record_transactional(
                    stop_token,
                    opaque_record_key,
                    subkeys,
                    consensus_count,
                    membership,
                )
                .await;
        }

        let peek_lock = self
            .record_lock_table
            .peek_lock(opaque_record_key.clone())
            .await;

        let safety_selection = {
            let inner = self.inner.lock();
            // If this record key is in any transaction, disallow this operation at this time
            if inner
                .outbound_transaction_manager
                .get_transaction_by_record(&opaque_record_key)
                .is_some()
            {
                apibail_try_again!("not rehydrating while records is in transaction");
            }
            if let Some(opened_record) = inner.opened_records.get(&opaque_record_key) {
                opened_record.safety_selection()
            } else {
                // See if it's in the local record store
                let Some(safety_selection) = local_record_store
                    .with_record(&opaque_record_key, |rec| {
                        rec.detail().safety_selection.clone()
                    })?
                else {
                    apibail_key_not_found!(opaque_record_key);
                };
                safety_selection
            }
        };

        // See if the requested record is our local record store
        let local_inspect_result = self
            .handle_inspect_local_values(peek_lock.record(), subkeys.clone(), true)
            .await?;

        // Get rpc processor and drop mutex so we don't block while getting the value from the network
        if !self.dht_is_online() {
            apibail_try_again!("offline, try again later");
        };

        // Trim inspected subkey range to subkeys we have data for locally
        let local_inspect_result = local_inspect_result.strip_none_seqs();

        // Get the inspect record report from the network with only the subkeys for which we have
        // sequence numbers we have locally
        let outbound_inspect_result = self
            .outbound_inspect_value(
                &opaque_record_key,
                local_inspect_result.subkeys().clone(),
                safety_selection.clone(),
                InspectResult::default(),
                true,
            )
            .await?;

        // Keep the list of nodes that returned a value for later reference
        let results_iter = outbound_inspect_result
            .inspect_result
            .subkeys()
            .iter()
            .map(ValueSubkeyRangeSet::single)
            .zip(outbound_inspect_result.subkey_fanout_results.clone());

        let existed = self.process_fanout_results(
            opaque_record_key.clone(),
            results_iter,
            false,
            self.config().internal().network.dht.consensus_width as usize,
        )?;

        if !existed {
            apibail_internal!(
                "record was locked for inspect but is now missing: {}",
                opaque_record_key
            );
        }

        // If online result had no subkeys, then trigger writing the entire record in the background
        if outbound_inspect_result.inspect_result.subkeys().is_empty()
            || outbound_inspect_result
                .inspect_result
                .opt_descriptor()
                .is_none()
        {
            return self.rehydrate_all_subkeys(
                opaque_record_key,
                subkeys,
                consensus_count,
                safety_selection,
                local_inspect_result,
            );
        }

        self.rehydrate_required_subkeys(
            opaque_record_key,
            subkeys,
            consensus_count,
            safety_selection,
            local_inspect_result,
            outbound_inspect_result,
        )
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(self), ret, err)
    )]
    pub(super) fn rehydrate_all_subkeys(
        &self,
        opaque_record_key: OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        consensus_count: usize,
        safety_selection: SafetySelection,
        local_inspect_result: InspectResult,
    ) -> VeilidAPIResult<RehydrateReport> {
        veilid_log!(self debug "Rehydrating all subkeys: record={} subkeys={}", opaque_record_key, subkeys);

        let rehydrated =
            Self::compute_rehydration_subkeys(&local_inspect_result, None, consensus_count);
        for subkey in rehydrated.iter() {
            self.add_offline_subkey_write(
                opaque_record_key.clone(),
                subkey,
                safety_selection.clone(),
            );
        }

        if rehydrated.is_empty() {
            veilid_log!(self debug "Record wanted full rehydrating, but no subkey data available: record={} subkeys={}", opaque_record_key, subkeys);
        } else {
            veilid_log!(self debug "Record full rehydrating: record={} subkeys={} rehydrated={}", opaque_record_key, subkeys, rehydrated);
        }

        Ok(RehydrateReport {
            opaque_record_key,
            subkeys,
            consensus_count,
            rehydrated,
        })
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(self), ret, err)
    )]
    pub(super) fn rehydrate_required_subkeys(
        &self,
        opaque_record_key: OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        consensus_count: usize,
        safety_selection: SafetySelection,
        local_inspect_result: InspectResult,
        outbound_inspect_result: OutboundInspectValueResult,
    ) -> VeilidAPIResult<RehydrateReport> {
        // Determine which subkeys need rehydrating and schedule them as offline writes
        let rehydrated = Self::compute_rehydration_subkeys(
            &local_inspect_result,
            Some(&outbound_inspect_result),
            consensus_count,
        );
        for subkey in rehydrated.iter() {
            self.add_offline_subkey_write(
                opaque_record_key.clone(),
                subkey,
                safety_selection.clone(),
            );
        }

        if rehydrated.is_empty() {
            veilid_log!(self debug "Record did not need rehydrating: record={} local_subkeys={}", opaque_record_key, local_inspect_result.subkeys());
        } else {
            veilid_log!(self debug "Record rehydrating: record={} local_subkeys={} rehydrated={}", opaque_record_key, local_inspect_result.subkeys(), rehydrated);
        }

        // Keep the list of nodes that returned a value for later reference
        let results_iter = outbound_inspect_result
            .inspect_result
            .subkeys()
            .iter()
            .map(ValueSubkeyRangeSet::single)
            .zip(outbound_inspect_result.subkey_fanout_results);

        let existed = self.process_fanout_results(
            opaque_record_key.clone(),
            results_iter,
            false,
            self.config().internal().network.dht.consensus_width as usize,
        )?;

        if !existed {
            veilid_log!(self debug
                "record was rehydrated but was deleted locally: {}",
                opaque_record_key
            );
        }

        Ok(RehydrateReport {
            opaque_record_key,
            subkeys,
            consensus_count,
            rehydrated,
        })
    }

    /// Decide which subkeys need rehydrating: those newer locally than the network, or equal but
    /// with fewer than `consensus_count` nodes in agreement. With no network result (the record is
    /// absent online), every locally-present subkey is rehydrated.
    pub(super) fn compute_rehydration_subkeys(
        local_inspect_result: &InspectResult,
        opt_outbound_inspect_result: Option<&OutboundInspectValueResult>,
        consensus_count: usize,
    ) -> ValueSubkeyRangeSet {
        let mut rehydrated = ValueSubkeyRangeSet::new();
        for (n, subkey) in local_inspect_result.subkeys().iter().enumerate() {
            let local_seq = local_inspect_result.seqs()[n];
            if local_seq.is_none() {
                continue;
            }
            let needed = match opt_outbound_inspect_result {
                // No network copy: push everything we have locally
                None => true,
                Some(outbound) => {
                    let network_seq = outbound.inspect_result.seqs()[n];
                    if local_seq > network_seq {
                        // Our copy is newer
                        true
                    } else {
                        // Equal or older: rehydrate only if the network lacks strict consensus
                        outbound
                            .subkey_fanout_results
                            .get(n)
                            .map(|sfr| sfr.consensus_nodes.len() < consensus_count)
                            .unwrap_or(false)
                    }
                }
            };
            if needed {
                rehydrated.insert(subkey);
            }
        }
        rehydrated
    }

    /// Rehydrate a record that participates in a committed transaction set: begin a background
    /// transaction over the whole set, re-push each member's needed subkeys (newer-than-network or
    /// under strict consensus) as their existing signed values, and commit. All-or-nothing on
    /// presence: a missing member or stored signer cancels the whole rehydration. Peek-lock
    /// contention with an overlapping set surfaces as TryAgain and re-enqueues.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(self, membership), ret, err)
    )]
    async fn rehydrate_record_transactional(
        &self,
        stop_token: StopToken,
        opaque_record_key: OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        consensus_count: usize,
        membership: TransactionMembership,
    ) -> VeilidAPIResult<RehydrateReport> {
        // Triggering record key, with its encryption key if it is currently open
        let triggering_record_key = {
            let inner = self.inner.lock();
            let encryption_key = inner
                .opened_records
                .get(&opaque_record_key)
                .and_then(|or| or.encryption_key());
            RecordKey::from_opaque(opaque_record_key.clone(), encryption_key)
        };

        let triggering_opaque = opaque_record_key.clone();
        let rehydrated = self
            .with_background_transaction(
                triggering_record_key,
                membership,
                move |transaction_handle, members| async move {
                    // Collect across the whole set first: a network-newer subkey anywhere means
                    // our local snapshot is stale, and pushing any of it would publish a mix of
                    // states no transaction ever committed. All-or-nothing: bail and roll back.
                    let mut member_values = vec![];
                    let mut set_has_network_newer = false;
                    for member in &members {
                        let (values, has_network_newer) = self
                            .collect_member_rehydration_values(
                                transaction_handle,
                                member,
                                consensus_count,
                            )
                            .await?;
                        set_has_network_newer |= has_network_newer;
                        member_values.push((member.clone(), values));
                    }
                    if set_has_network_newer {
                        veilid_log!(self debug "Not rehydrating transaction set for {}: network is newer than local", triggering_opaque);
                        return VeilidAPIResult::Ok(ValueSubkeyRangeSet::new());
                    }

                    let triggering_rehydrated = Mutex::new(ValueSubkeyRangeSet::new());
                    for (member, values) in member_values {
                        let member_opaque = member.opaque();
                        // Push in parallel; sequential sets make large records take minutes
                        let set_futs =
                            values
                                .into_iter()
                                .map(|(subkey, signed_value_data)| {
                                    let member = member.clone();
                                    let member_opaque = member_opaque.clone();
                                    async move {
                                        let _subkey_lock = self
                                            .record_lock_table
                                            .lock_subkey(
                                                member_opaque,
                                                subkey,
                                                StorageManagerSubkeyLockPurpose::TransactSet,
                                            )
                                            .await;
                                        self.transaction_set_signed_value_data(
                                            transaction_handle,
                                            member,
                                            subkey,
                                            signed_value_data,
                                        )
                                        .await?;
                                        VeilidAPIResult::Ok(subkey)
                                    }
                                });
                        let completed = process_batched_future_queue_result(
                            set_futs,
                            REHYDRATE_SET_CONCURRENCY,
                            stop_token.clone(),
                            |res: VeilidAPIResult<ValueSubkey>| {
                                let subkey = res?;
                                if member_opaque == triggering_opaque {
                                    triggering_rehydrated.lock().insert(subkey);
                                }
                                VeilidAPIResult::Ok(())
                            },
                        )
                        .await?;
                        if !completed {
                            apibail_try_again!("rehydration stopped, try again later");
                        }
                    }

                    // Commit the whole set
                    self.end_and_commit_transaction(transaction_handle).await?;
                    VeilidAPIResult::Ok(triggering_rehydrated.into_inner())
                },
            )
            .await?;

        Ok(RehydrateReport {
            opaque_record_key,
            subkeys,
            consensus_count,
            rehydrated,
        })
    }

    /// Determine which of a transaction-set member's subkeys need rehydrating and read their
    /// existing local signed values. Uses the data the begin fanout already gathered (no second
    /// fanout): per-subkey seqs come from the transaction state via `transaction_inspect`, and the
    /// per-subkey holder count comes from each begin-set node's Begin answer seqs. Also reports
    /// whether the network is newer than local anywhere in this member (a stale local snapshot),
    /// which disqualifies the whole set from rehydrating.
    async fn collect_member_rehydration_values(
        &self,
        transaction_handle: OutboundTransactionHandle,
        member_record_key: &RecordKey,
        consensus_count: usize,
    ) -> VeilidAPIResult<(Vec<(ValueSubkey, Arc<SignedValueData>)>, bool)> {
        let opaque_record_key = member_record_key.opaque();

        // Per-subkey local and begin-time network seqs from the transaction state (no fanout)
        let report = self.transaction_inspect(
            transaction_handle,
            member_record_key.clone(),
            Some(ValueSubkeyRangeSet::full()),
            DHTReportScope::SyncGet,
        )?;

        let mut values = vec![];
        let mut has_network_newer = false;
        for (n, subkey) in report.subkeys().iter().enumerate() {
            let local_seq = report.local_seqs()[n];
            let network_seq = report.network_seqs()[n];
            if network_seq > local_seq {
                // Network is ahead of our local copy here; nothing to push for this subkey
                has_network_newer = true;
                continue;
            }
            if local_seq.is_none() {
                continue;
            }
            let needed = if local_seq > network_seq {
                // Our copy is newer than the network
                true
            } else {
                // Equal: rehydrate only if fewer than strict consensus nodes hold it
                let holder_count = {
                    let inner = self.inner.lock();
                    inner.outbound_transaction_manager.count_begin_holders(
                        transaction_handle,
                        &opaque_record_key,
                        subkey,
                        local_seq,
                    )?
                };
                holder_count < consensus_count
            };
            if needed {
                let get_result = self
                    .handle_get_single_local_value(&opaque_record_key, subkey, false)
                    .await?;
                if let Some(signed_value_data) = get_result.opt_value {
                    values.push((subkey, signed_value_data));
                }
            }
        }
        veilid_log!(self debug "Rehydration member {}: {} values to push (network_newer={} local_seqs={:?} network_seqs={:?})", opaque_record_key, values.len(), has_network_newer, report.local_seqs(), report.network_seqs());
        Ok((values, has_network_newer))
    }
}
