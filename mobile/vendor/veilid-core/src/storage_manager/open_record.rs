use super::*;

impl StorageManager {
    /// Open an existing local record if it exists, and if it doesnt exist locally, try to pull it from the network and open it and return the opened descriptor
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub async fn open_record(
        &self,
        record_key: RecordKey,
        writer: Option<KeyPair>,
        safety_selection: SafetySelection,
    ) -> VeilidAPIResult<DHTRecordDescriptor> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };
        let opaque_record_key = record_key.opaque();
        let record_lock = self
            .record_lock_table
            .lock_record(
                opaque_record_key.clone(),
                StorageManagerRecordLockPurpose::Open,
            )
            .await;

        // See if we have a local record already or not
        if let Some(res) = self.open_existing_record_locked(
            &record_lock,
            record_key.clone(),
            writer.clone(),
            safety_selection.clone(),
        )? {
            // We had an existing record, so check the network to see if we should
            // update it with what we have here
            let set_consensus = self.config().internal().network.dht.set_value_count as usize;

            self.add_rehydration_request(
                opaque_record_key,
                ValueSubkeyRangeSet::full(),
                set_consensus,
            );

            return Ok(res);
        }

        // No record yet, try to get it from the network
        if !self.dht_is_online() {
            apibail_try_again!("offline, try again later");
        };

        // Inspecting only subkey 0 gets the descriptor for the record but
        // minimizes the number of subkeys we wait for from the network in the event that
        // the record has many subkeys that have not yet been written to
        // This is a bit of a hack because in theory other subkeys besides 0 could have been
        // written to, but subkey 0 is the most likely to have been written to first
        // Also, we know subkey 0 must exist, and if we don't have a schema, the only other alternative
        // is a ValueSubkeyRangeSet::full() which would be more like a transact_dht_record in terms of wait time
        // No last descriptor, no last value. Use the safety selection we opened the record with.
        let outbound_inspect_result = self
            .outbound_inspect_value(
                &opaque_record_key,
                ValueSubkeyRangeSet::single(0),
                safety_selection.clone(),
                InspectResult::default(),
                false,
            )
            .await?;

        // If we got nothing back, the key wasn't found
        if outbound_inspect_result
            .inspect_result
            .opt_descriptor()
            .is_none()
        {
            // No result
            apibail_key_not_found!(opaque_record_key);
        };

        // Open the new record
        self.open_new_record_locked(
            &record_lock,
            record_key,
            writer,
            outbound_inspect_result,
            safety_selection,
        )
        .await
    }

    ////////////////////////////////////////////////////////////////////////

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub(super) fn open_existing_record_locked(
        &self,
        record_lock: &StorageManagerRecordLockGuard,
        record_key: RecordKey,
        writer: Option<KeyPair>,
        safety_selection: SafetySelection,
    ) -> VeilidAPIResult<Option<DHTRecordDescriptor>> {
        let opaque_record_key = record_lock.record();
        if record_key.opaque() != opaque_record_key {
            apibail_internal!("wrong record lock");
        }

        // Get local record store
        let local_record_store = self.get_local_record_store()?;

        // See if we have a local record already or not
        let cb = |descriptor: Arc<SignedValueDescriptor>, r: &mut LocalRecordDetail| {
            // Process local record

            // Keep the safety selection we opened the record with
            r.safety_selection = safety_selection.clone();

            // Return record details
            (descriptor.owner(), descriptor.schema().unwrap_or_log())
        };
        let (owner, schema) =
            match local_record_store.with_record_detail_mut(&opaque_record_key, cb)? {
                Some(v) => v,
                None => {
                    return Ok(None);
                }
            };
        // Had local record

        // If the writer we chose is also the owner, we have the owner secret
        // Otherwise this is just another subkey writer
        let owner_secret = if let Some(writer) = writer.clone() {
            if writer.key() == owner {
                Some(writer.secret())
            } else {
                None
            }
        } else {
            None
        };

        let crypto = self.crypto();

        let mut crypto_with_key: Option<(CryptoSystemGuard, BareSharedSecret)> = None;

        if let Some(k) = record_key.ref_value().encryption_key() {
            let Some(value_crypto) = crypto.get(record_key.kind()) else {
                apibail_generic!("unsupported cryptosystem for record encryption key");
            };
            crypto_with_key = Some((value_crypto, k));
        }

        // Write open record
        self.inner
            .lock()
            .opened_records
            .entry(opaque_record_key)
            .and_modify(|e| {
                e.set_writer(writer.clone());
                e.set_safety_selection(safety_selection.clone());
                e.set_encryption_key(crypto_with_key.as_ref().map(|(_, k)| k.clone()));
            })
            .or_insert_with(|| {
                OpenedRecord::new(
                    writer.clone(),
                    safety_selection.clone(),
                    crypto_with_key.map(|(_, k)| k),
                )
            });

        // Make DHT Record Descriptor to return
        let descriptor = DHTRecordDescriptor::new(record_key, owner, owner_secret, schema);
        Ok(Some(descriptor))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub(super) async fn open_new_record_locked(
        &self,
        record_lock: &StorageManagerRecordLockGuard,
        record_key: RecordKey,
        writer: Option<KeyPair>,
        outbound_inspect_result: OutboundInspectValueResult,
        safety_selection: SafetySelection,
    ) -> VeilidAPIResult<DHTRecordDescriptor> {
        let opaque_record_key = record_lock.record();
        if record_key.opaque() != opaque_record_key {
            apibail_internal!("wrong record lock");
        }

        let local_record_store = self.get_local_record_store()?;

        // Ensure the record is closed
        if self
            .inner
            .lock()
            .opened_records
            .contains_key(&opaque_record_key)
        {
            apibail_internal!(
                "new record {} should never be opened at this point",
                opaque_record_key
            );
        }

        // Must have descriptor
        let Some(signed_value_descriptor) = outbound_inspect_result.inspect_result.opt_descriptor()
        else {
            // No descriptor for new record, can't store this
            apibail_generic!("no descriptor");
        };
        // Get owner
        let owner = signed_value_descriptor.owner();

        // If the writer we chose is also the owner, we have the owner secret
        // Otherwise this is just another subkey writer
        let owner_secret = if let Some(writer) = &writer {
            if writer.key() == owner {
                Some(writer.secret())
            } else {
                None
            }
        } else {
            None
        };
        let schema = signed_value_descriptor.schema()?;

        // Make and store a new record for this descriptor
        let record = Record::<LocalRecordDetail>::new(
            Timestamp::now(),
            signed_value_descriptor,
            LocalRecordDetail::new(safety_selection.clone()),
        )?;

        local_record_store
            .new_record(opaque_record_key.clone(), record)
            .await?;

        let encryption_key = record_key.ref_value().encryption_key();

        // Write open record
        self.inner.lock().opened_records.insert(
            opaque_record_key.clone(),
            OpenedRecord::new(writer, safety_selection, encryption_key),
        );

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
            apibail_internal!(
                "record was locked for open new record but is now missing: {}",
                opaque_record_key
            );
        }

        // Make DHT Record Descriptor to return
        let descriptor = DHTRecordDescriptor::new(record_key, owner, owner_secret, schema);
        Ok(descriptor)
    }
}
