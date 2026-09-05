use super::*;

impl StorageManager {
    /// A non-transactional set, or a network-enabled get, clears a record's transaction membership
    pub(super) fn clear_record_transaction_set(
        &self,
        opaque_record_key: &OpaqueRecordKey,
    ) -> VeilidAPIResult<()> {
        let local_record_store = self.get_local_record_store()?;
        let is_transactional = local_record_store
            .with_record(opaque_record_key, |r| {
                r.detail().transaction_membership.is_some()
            })?
            .unwrap_or(false);
        if is_transactional {
            local_record_store.with_record_detail_mut(
                opaque_record_key,
                |_descriptor, detail| {
                    detail.transaction_membership = None;
                },
            )?;
        }
        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) async fn handle_get_single_local_value(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
        want_descriptor: bool,
    ) -> VeilidAPIResult<GetResult> {
        let local_record_store = self.get_local_record_store()?;

        // See if it's in the local record store
        if let Some(get_result) = local_record_store
            .get_subkey(opaque_record_key, subkey, want_descriptor)
            .await?
        {
            return Ok(get_result);
        }

        Ok(GetResult::default())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) async fn handle_offline_set_single_local_value_with_subkey_lock(
        &self,
        subkey_lock: &StorageManagerSubkeyLockGuard,
        value: Arc<SignedValueData>,
        safety_selection: SafetySelection,
        allow_offline: AllowOffline,
    ) -> VeilidAPIResult<()> {
        // Don't do this if we are disallowing offline writes
        if allow_offline == AllowOffline(false) {
            apibail_try_again!("offline, try again later");
        }

        let opaque_record_key = subkey_lock.record();
        let subkey = subkey_lock.subkey();

        veilid_log!(self debug "Writing subkey offline: {}:{} len={}", opaque_record_key, subkey, value.value_data().data().len() );

        // Write subkey to local store
        let local_record_store = self.get_local_record_store()?;
        local_record_store
            .set_single_subkey(
                &opaque_record_key,
                subkey,
                value.clone(),
                InboundWatchUpdateMode::NoUpdate,
                CommitActionFlushMode::Immediate,
            )
            .await?;

        // Ensure we come back to put this to the network later
        // (it may already be added but this ensures we try again)
        self.add_offline_subkey_write(opaque_record_key, subkey, safety_selection);

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) async fn handle_set_single_local_value_with_subkey_lock(
        &self,
        subkey_lock: &StorageManagerSubkeyLockGuard,
        value: Arc<SignedValueData>,
    ) -> VeilidAPIResult<()> {
        let opaque_record_key = subkey_lock.record();
        let subkey = subkey_lock.subkey();

        // Remove any offline writes to this subkey since we're rewriting it
        {
            let mut inner = self.inner.lock();
            self.remove_offline_subkey_write_inner(&mut inner, &opaque_record_key, subkey);
        }

        // Write subkey to local store
        let local_record_store = self.get_local_record_store()?;
        local_record_store
            .set_single_subkey(
                &opaque_record_key,
                subkey,
                value.clone(),
                InboundWatchUpdateMode::NoUpdate,
                CommitActionFlushMode::Immediate,
            )
            .await?;

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) async fn handle_set_single_local_value_with_single_record_lock(
        &self,
        record_lock: &StorageManagerRecordLockGuard,
        subkey: ValueSubkey,
        value: Arc<SignedValueData>,
    ) -> VeilidAPIResult<()> {
        let opaque_record_key = record_lock.record();

        // Remove any offline writes to this subkey since we're rewriting it
        {
            let mut inner = self.inner.lock();
            self.remove_offline_subkey_write_inner(&mut inner, &opaque_record_key, subkey);
        }

        // Write subkey to local store
        let local_record_store = self.get_local_record_store()?;
        local_record_store
            .set_single_subkey(
                &opaque_record_key,
                subkey,
                value.clone(),
                InboundWatchUpdateMode::NoUpdate,
                CommitActionFlushMode::Immediate,
            )
            .await?;

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    #[expect(dead_code)]
    pub(super) async fn handle_set_local_values_with_single_record_lock(
        &self,
        record_lock: &StorageManagerRecordLockGuard,
        subkey_values: SubkeyValueList,
    ) -> VeilidAPIResult<()> {
        let opaque_record_key = record_lock.record();

        // Remove any offline writes to this subkey since we're rewriting it
        {
            let mut inner = self.inner.lock();
            for subkey in subkey_values.iter().map(|x| x.0) {
                self.remove_offline_subkey_write_inner(&mut inner, &opaque_record_key, subkey);
            }
        }

        // Write subkey to local store
        let local_record_store = self.get_local_record_store()?;
        local_record_store
            .set_subkeys_single_record(
                &opaque_record_key,
                &subkey_values,
                InboundWatchUpdateMode::NoUpdate,
                CommitActionFlushMode::Immediate,
            )
            .await?;

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) async fn handle_set_local_values_with_multiple_records_lock(
        &self,
        records_lock: &StorageManagerRecordsLockGuard,
        record_commit_values: Vec<RecordCommitValues>,
    ) -> VeilidAPIResult<()> {
        let records = records_lock.records().into_iter().collect::<BTreeSet<_>>();
        for rcv in record_commit_values.iter() {
            if !records.contains(&rcv.opaque_record_key) {
                apibail_internal!("invalid records lock")
            }
        }

        // See if this new data supercedes any offline subkey writes
        {
            let mut inner = self.inner.lock();
            for rcv in record_commit_values.iter() {
                for subkey in rcv.subkey_values.iter().map(|x| x.0) {
                    self.remove_offline_subkey_write_inner(
                        &mut inner,
                        &rcv.opaque_record_key,
                        subkey,
                    );
                }
            }
        }

        // Write subkeys to local store
        let record_subkey_values: RecordSubkeyValueList = record_commit_values
            .iter()
            .map(|rcv| (rcv.opaque_record_key.clone(), rcv.subkey_values.clone()))
            .collect();
        let local_record_store = self.get_local_record_store()?;
        local_record_store
            .set_subkeys_multiple_records(
                &record_subkey_values,
                InboundWatchUpdateMode::NoUpdate,
                CommitActionFlushMode::Immediate,
            )
            .await?;

        // Stamp transaction membership (the full committed set + each record's operation signer)
        let record_set: Vec<OpaqueRecordKey> = record_commit_values
            .iter()
            .map(|rcv| rcv.opaque_record_key.clone())
            .collect();
        for rcv in &record_commit_values {
            local_record_store.with_record_detail_mut(
                &rcv.opaque_record_key,
                |_descriptor, detail| {
                    detail.transaction_membership = Some(TransactionMembership {
                        record_set: record_set.clone(),
                        operation_signer: rcv.operation_signer.clone(),
                    });
                },
            )?;
        }

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) async fn handle_inspect_local_values(
        &self,
        opaque_record_key: OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        want_descriptor: bool,
    ) -> VeilidAPIResult<InspectResult> {
        let local_record_store = self.get_local_record_store()?;

        if let Some(inspect_result) = local_record_store
            .inspect_record(&opaque_record_key, &subkeys, want_descriptor)
            .await?
        {
            return Ok(inspect_result);
        }

        Ok(InspectResult::default())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) fn get_value_nodes(
        &self,
        opaque_record_key: &OpaqueRecordKey,
    ) -> VeilidAPIResult<Option<Vec<NodeRef>>> {
        // Get local record store
        let local_record_store = self.get_local_record_store()?;

        // Get routing table to see if we still know about these nodes
        let routing_table = self.routing_table();

        let cur_ts = Timestamp::now_non_decreasing();
        let opt_value_nodes = local_record_store.peek_record(opaque_record_key, |r| {
            let d = r.detail();
            d.nodes
                .keys()
                .cloned()
                .filter_map(|nr| routing_table.lookup_node_id(nr).ok().flatten())
                // A known-dead cached value node wastes a full RPC timeout in
                // every fanout it seeds; it can re-earn a place via discovery
                .filter(|nr| nr.state(cur_ts).maybe_live())
                .collect()
        });

        Ok(opt_value_nodes)
    }
}
