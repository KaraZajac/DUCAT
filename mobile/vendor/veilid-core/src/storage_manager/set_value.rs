use super::*;

impl_veilid_log_facility!("stor");

/// The context of the outbound_set_value operation
struct OutboundSetValueContext {
    /// The latest value of the subkey, may be the value passed in
    pub value: Arc<SignedValueData>,
    /// The number of non-sets since the last set we have received
    pub missed_since_last_set: usize,
}

/// The result of the outbound_set_value operation
#[derive(Clone, Debug)]
pub(super) struct OutboundSetValueResult {
    /// Fanout result
    pub fanout_result: FanoutResult,
    /// The value that was set
    pub signed_value_data: Arc<SignedValueData>,
}

/// The result of the inbound_set_value operation
#[derive(Clone, Debug)]
pub(crate) enum InboundSetValueResult {
    /// Value set successfully, or it was the same value
    Success,
    /// Newer value or conflicting value present
    Ignored(Arc<SignedValueData>),
    /// Descriptor is needed for first set
    NeedsDescriptor,
}

impl StorageManager {
    /// Set the value of a subkey on an opened local record
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub async fn set_value(
        &self,
        record_key: RecordKey,
        subkey: ValueSubkey,
        data: Vec<u8>,
        options: Option<SetDHTValueOptions>,
    ) -> VeilidAPIResult<Option<ValueData>> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let opaque_record_key = record_key.opaque();

        let subkey_lock = self
            .record_lock_table
            .lock_subkey(
                opaque_record_key.clone(),
                subkey,
                StorageManagerSubkeyLockPurpose::Set,
            )
            .await;

        // Use the specified writer, or if not specified, the default writer when the record was opened
        let (safety_selection, opt_writer) = {
            let inner = self.inner.lock();

            // If this record key is in any transaction, disallow this operation at this time
            if inner
                .outbound_transaction_manager
                .get_transaction_by_record(&opaque_record_key)
                .is_some()
            {
                apibail_try_again!("record is currently in transaction");
            }

            let Some(opened_record) = inner.opened_records.get(&opaque_record_key) else {
                apibail_invalid_argument!("record not open", "record_key", opaque_record_key);
            };
            (
                opened_record.safety_selection(),
                opened_record.writer().cloned(),
            )
        };
        let opt_writer = options
            .as_ref()
            .and_then(|o| o.writer.clone())
            .or(opt_writer);
        let allow_offline = options
            .unwrap_or_default()
            .allow_offline
            .unwrap_or_default();

        // If we don't have a writer then we can't write
        let Some(writer) = opt_writer else {
            apibail_generic!("value is not writable");
        };

        // A non-transactional set clears this record's transaction membership
        self.clear_record_transaction_set(&opaque_record_key)?;

        // Make signed value data (encrypted) and value data (unencrypted) and get descriptor for this value
        let last_get_result = self
            .handle_get_single_local_value(&opaque_record_key, subkey, true)
            .await?;

        let (signed_value_data, value_data, descriptor) = self
            .prepare_set_value_data(&record_key, subkey, data, &writer, last_get_result)
            .await?;

        // Check if we are offline (this is a race, but an optimization to avoid fanout if it is likely to fail)
        if !self.dht_is_online() {
            self.handle_offline_set_single_local_value_with_subkey_lock(
                &subkey_lock,
                signed_value_data,
                safety_selection.clone(),
                allow_offline,
            )
            .await?;

            return Ok(None);
        }

        veilid_log!(self debug "Writing subkey to the network: {}:{} len={}", opaque_record_key, subkey, signed_value_data.value_data().data().len() );

        // Gate on concurrency + the upload budget (a set pushes the value up)
        let set_value_fanout = self.config().internal().network.dht.set_value_fanout as u64;
        let value_bytes = signed_value_data.value_data().data().len() as u64;
        let _gate_permit = self
            .acquire_operation_gate(Some(value_bytes.saturating_mul(set_value_fanout)), None)
            .await;

        // Use the safety selection we opened the record with
        let result = match self
            .outbound_set_value(
                &opaque_record_key,
                subkey,
                safety_selection.clone(),
                signed_value_data.clone(),
                descriptor,
            )
            .await
        {
            Ok(v) => v,
            Err(e) => {
                // Failed to write, try again later
                self.handle_offline_set_single_local_value_with_subkey_lock(
                    &subkey_lock,
                    signed_value_data,
                    safety_selection.clone(),
                    allow_offline,
                )
                .await?;

                if matches!(e, VeilidAPIError::TryAgain { message: _ }) {
                    return Ok(None);
                }
                return Err(e);
            }
        };

        let out = self
            .process_outbound_set_value_result_locked(
                &subkey_lock,
                record_key,
                value_data,
                safety_selection,
                allow_offline,
                result,
            )
            .await;

        if matches!(out, Err(VeilidAPIError::TryAgain { message: _ })) {
            return Ok(None);
        }

        out
    }

    ////////////////////////////////////////////////////////////////////////

    pub(super) async fn prepare_set_value_data(
        &self,
        record_key: &RecordKey,
        subkey: ValueSubkey,
        data: Vec<u8>,
        writer: &KeyPair,
        last_get_result: GetResult,
    ) -> VeilidAPIResult<(Arc<SignedValueData>, ValueData, Arc<SignedValueDescriptor>)> {
        // Get cryptosystem
        let crypto = self.crypto();
        let Some(vcrypto) = crypto.get_async(record_key.kind()) else {
            apibail_generic!("unsupported cryptosystem for record key");
        };

        // See if the subkey we are modifying has a last known local value
        let GetResult {
            opt_descriptor: opt_last_descriptor,
            opt_value: opt_last_value,
        } = last_get_result;

        // Get the descriptor and schema for the key
        let Some(descriptor) = opt_last_descriptor else {
            apibail_generic!("must have a descriptor");
        };
        let schema = descriptor.schema()?;

        let mut seq = ValueSeqNum::ZERO;

        // Check if the subkey value already exists
        if let Some(last_signed_value_data) = opt_last_value {
            let decrypted = self
                .maybe_decrypt_value_data(record_key, last_signed_value_data.value_data())
                .await?;
            if decrypted.data() == data
                && last_signed_value_data.value_data().writer() == writer.key()
            {
                // Data and writer is the same, nothing is changing,
                // but it is possible the value on the network has changed,
                // So keep the same sequence number and see if a newer value is present
                seq = last_signed_value_data.value_data().seq();
            } else {
                // New value is different, increment sequence number
                seq = last_signed_value_data.value_data().seq().next()?;
            }
        };

        // Make new subkey data
        let value_data = ValueData::new_with_seq(seq, data, writer.key())?;

        let encrypted_value_data = self
            .maybe_encrypt_value_data(record_key, &value_data)
            .await?;

        // Validate with schema
        if let Err(e) = self.check_subkey_value_data(
            &schema,
            descriptor.ref_owner(),
            subkey,
            &encrypted_value_data,
        ) {
            veilid_log!(self debug "schema validation error: {}", e);
            // Validation failed, ignore this value
            apibail_generic!("failed schema validation: {}:{}", record_key, subkey);
        }

        // Sign the new value data with the writer
        let signed_value_data = Arc::new(
            SignedValueData::make_signature(
                encrypted_value_data,
                &descriptor.owner(),
                subkey,
                &vcrypto,
                &writer.secret(),
            )
            .await?,
        );

        Ok((signed_value_data, value_data, descriptor))
    }

    /// Perform a 'set value' query on the network
    /// Performs the work without a transaction
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip_all, err)
    )]
    pub(super) async fn outbound_set_value(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
        safety_selection: SafetySelection,
        value: Arc<SignedValueData>,
        descriptor: Arc<SignedValueDescriptor>,
    ) -> VeilidAPIResult<OutboundSetValueResult> {
        let routing_domain = RoutingDomain::PublicInternet;

        // Get the DHT parameters for 'SetValue'
        let config = self.config();
        let node_count = config.internal().network.dht.max_find_node_count as usize;
        let consensus_count = config.internal().network.dht.set_value_count as usize;
        let consensus_width = config.internal().network.dht.consensus_width as usize;
        let fanout_tasks = config.internal().network.dht.set_value_fanout as usize;
        let timeout =
            TimestampDuration::from(ms_to_us(config.internal().network.dht.set_value_timeout_ms));

        // Get the nodes we know are caching this value to seed the fanout
        let init_fanout_queue = self
            .get_value_nodes(opaque_record_key)?
            .unwrap_or_default()
            .into_iter()
            .filter(|x| {
                x.node_info(routing_domain)
                    .map(|ni| ni.has_all_capabilities(&[VEILID_CAPABILITY_DHT]))
                    .unwrap_or_default()
            })
            .collect();

        // Make operation context
        let schema = descriptor.schema()?;
        let context = Arc::new(Mutex::new(OutboundSetValueContext {
            value,
            missed_since_last_set: 0,
        }));
        let descriptor_cache = self.descriptor_cache.clone();

        // Routine to call to generate fanout
        let call_routine = {
            let context = context.clone();
            let registry = self.registry();
            let schema = schema.clone();
            let opaque_record_key = opaque_record_key.clone();
            let safety_selection = safety_selection.clone();
            let descriptor_cache = descriptor_cache.clone();

            Arc::new(
                move |next_node: NodeRef| -> PinBoxFutureStatic<FanoutCallResult> {
                    let registry = registry.clone();
                    let context = context.clone();
                    let schema = schema.clone();
                    let descriptor = descriptor.clone();
                    let opaque_record_key = opaque_record_key.clone();
                    let safety_selection = safety_selection.clone();
                    let descriptor_cache = descriptor_cache.clone();
                    let fut = async move {
                        let rpc_processor = registry.rpc_processor();

                        // check the cache to see if we should send the descriptor
                        let node_id = next_node
                            .node_ids()
                            .get(opaque_record_key.kind())
                            .unwrap_or_log();
                        let dc_key = DescriptorCacheKey {
                            opaque_record_key: opaque_record_key.clone(),
                            node_id,
                        };
                        let mut descriptor_mode = SetDescriptorMode::new(
                            descriptor_cache.lock().get(&dc_key).is_none(),
                            descriptor,
                        );

                        // get most recent value to send
                        let sent_value = {
                            let ctx = context.lock();
                            ctx.value.clone()
                        };

                        // send across the wire, with a retry if the remote needed the descriptor
                        let sva = loop {
                            // send across the wire
                            let sva = match rpc_processor
                                .rpc_call_set_value(
                                    Destination::direct(
                                        next_node.routing_domain_filtered(routing_domain),
                                        Some(safety_selection.clone()),
                                    ),
                                    opaque_record_key.clone(),
                                    subkey,
                                    (*sent_value).clone(),
                                    descriptor_mode.clone(),
                                )
                                .await?
                            {
                                NetworkResult::Timeout => {
                                    return Ok(FanoutCallOutput {
                                        peer_info_list: vec![],
                                        disposition: FanoutCallDisposition::Timeout,
                                    });
                                }
                                NetworkResult::ServiceUnavailable(_)
                                | NetworkResult::NoConnection(_)
                                | NetworkResult::AlreadyExists(_)
                                | NetworkResult::InvalidMessage(_) => {
                                    return Ok(FanoutCallOutput {
                                        peer_info_list: vec![],
                                        disposition: FanoutCallDisposition::Invalid,
                                    });
                                }
                                NetworkResult::Value(v) => v,
                            };

                            // Do a retry if we needed to send the descriptor
                            // (if the cache was wrong)
                            if sva.answer.accepted {
                                if sva.answer.need_descriptor {
                                    if descriptor_mode.is_have() {
                                        descriptor_mode.change_to_send();
                                        continue;
                                    } else {
                                        veilid_log!(registry error target:"network_result", "Got 'need_descriptor' when descriptor was already sent: node={} record_key={}", next_node, opaque_record_key);
                                    }
                                }
                            } else if sva.answer.need_descriptor {
                                veilid_log!(registry error target:"network_result", "Got 'need_descriptor' from node that did not accept: node={} record_key={}", next_node, opaque_record_key);
                            }

                            break sva;
                        };

                        // If the node was close enough to possibly set the value
                        if !sva.answer.accepted {
                            let missed = {
                                let mut ctx = context.lock();
                                ctx.missed_since_last_set += 1;
                                ctx.missed_since_last_set
                            };
                            // Return peers if we have some
                            veilid_log!(registry debug target:"network_result", "SetValue missed: {}, fanout call returned peers {}", missed, sva.answer.peers.len());
                            return Ok(FanoutCallOutput {
                                peer_info_list: sva.answer.peers,
                                disposition: FanoutCallDisposition::Rejected,
                            });
                        }

                        // Cache if we sent the descriptor
                        if descriptor_mode.is_send() {
                            descriptor_cache.lock().insert(dc_key, ());
                        }

                        // See if we got a newer value back
                        let Some(value) = sva.answer.value else {
                            // No newer value was found and returned, so increase our consensus count
                            context.lock().missed_since_last_set = 0;

                            // Return peers if we have some
                            veilid_log!(registry debug target:"network_result", "SetValue returned no value, fanout call returned peers {}", sva.answer.peers.len());
                            return Ok(FanoutCallOutput {
                                peer_info_list: sva.answer.peers,
                                disposition: FanoutCallDisposition::Accepted,
                            });
                        };

                        // Keep the value if we got one and it is newer and it passes schema validation

                        // Validate with schema
                        if registry
                            .storage_manager()
                            .check_subkey_value_data(
                                &schema,
                                descriptor_mode.ref_descriptor().ref_owner(),
                                subkey,
                                value.value_data(),
                            )
                            .is_err()
                        {
                            // Validation failed, ignore this value and pretend we never saw this node
                            veilid_log!(registry debug "SetValue got value back but validation failed, marking as invalid: {}", value);
                            return Ok(FanoutCallOutput {
                                peer_info_list: vec![],
                                disposition: FanoutCallDisposition::Invalid,
                            });
                        }

                        // If we got a value back it should be different than the one we sent, but because other nodes may have returned the same value
                        // we still may have that value already in our context
                        if sent_value.value_data() == value.value_data() {
                            veilid_log!(registry debug "SetValue got value back but was the same as what was, marking as invalid: {}", value);
                            return Ok(FanoutCallOutput {
                                peer_info_list: sva.answer.peers,
                                disposition: FanoutCallDisposition::Invalid,
                            });
                        }

                        // Ensure a newer sequence number was returned than what was sent
                        let sent_seq = sent_value.value_data().seq();
                        let rcvd_seq = value.value_data().seq();
                        if rcvd_seq < sent_seq {
                            // If the sequence number is older node should have not returned a value here.
                            // Skip this node and its closer list because it is misbehaving
                            // Ignore this value and pretend we never saw this node
                            veilid_log!(registry debug "SetValue got value back but was older, marking as invalid: {}", value);
                            return Ok(FanoutCallOutput {
                                peer_info_list: vec![],
                                disposition: FanoutCallDisposition::Invalid,
                            });
                        }

                        // Check the value received against our current consensus value now
                        {
                            let mut ctx = context.lock();
                            let ctx_seq = ctx.value.value_data().seq();

                            if rcvd_seq < ctx_seq {
                                // Ignore if we've got something newer already
                                veilid_log!(registry debug "SetValue got value back, but it was older than our current value ({} < {}), marking as stale: {}  ", rcvd_seq, ctx_seq, value);
                                Ok(FanoutCallOutput {
                                    peer_info_list: sva.answer.peers,
                                    disposition: FanoutCallDisposition::Stale,
                                })
                            } else if rcvd_seq > ctx_seq
                                || value.value_data() != ctx.value.value_data()
                            {
                                // If the sequence number is greater or equal than the one in our context,
                                // keep it unless the context has the exact same value, even if the sequence number is the same, accept all conflicts in an attempt to resolve them
                                veilid_log!(registry debug "SetValue got value back, (rcvd_seq={}, ctx_seq={}) restarting with newer or different value: {}", rcvd_seq, ctx_seq,  value);

                                ctx.value = Arc::new(value);
                                // One node has shown us this value so far
                                ctx.missed_since_last_set = 0;

                                Ok(FanoutCallOutput {
                                    peer_info_list: sva.answer.peers,
                                    disposition: FanoutCallDisposition::AcceptedNewerRestart,
                                })
                            } else {
                                // If the sequence numbers were the same and the values were the same, then mark it as accepted
                                // since this contributes to our consensus
                                veilid_log!(registry debug "SetValue got value back, and it matched the current value received from other nodes: {}", value);
                                Ok(FanoutCallOutput {
                                    peer_info_list: sva.answer.peers,
                                    disposition: FanoutCallDisposition::Accepted,
                                })
                            }
                        }
                    };
                    #[cfg(feature = "instrument")]
                    let fut =
                        fut.instrument(tracing::trace_span!(target: "dht", "fanout call_routine"));
                    Box::pin(fut) as PinBoxFuture<FanoutCallResult>
                },
            )
        };

        // Routine to call to check if we're done at each step
        let check_done = {
            Arc::new(
                move |fanout_result: &FanoutResult| -> FanoutDoneDisposition {
                    match fanout_result.kind {
                        FanoutResultKind::Incomplete => FanoutDoneDisposition::NotDone,
                        FanoutResultKind::Timeout
                        | FanoutResultKind::Exhausted
                        | FanoutResultKind::Consensus => FanoutDoneDisposition::DoneEarly,
                    }
                },
            )
        };

        // Call the fanout in a spawned task
        let hash_coordinate = opaque_record_key.to_hash_coordinate();
        let shutdown_stop_source = self
            .startup_lock
            .stop_token()
            .ok_or_else(VeilidAPIError::not_initialized)?;

        let routing_table = self.routing_table();
        let fanout_call = FanoutCall::new(
            &routing_table,
            FanoutCallParams {
                name: format!("outbound_set_value({})", Timestamp::now_increasing()),
                hash_coordinate,
                node_count,
                fanout_tasks,
                consensus_count,
                consensus_width,
                timeout,
            },
            capability_fanout_peer_info_filter(vec![VEILID_CAPABILITY_DHT]),
            call_routine,
            check_done,
        );

        let fanout_result = fanout_call
            .run(
                init_fanout_queue,
                FanoutQueueMode::ThrottleAtConsensus,
                shutdown_stop_source,
            )
            .await
            .inspect_err(|e| {
                veilid_log!(self debug "SetValue fanout error: {}", e);
            })?;

        veilid_log!(self debug "SetValue Fanout: {:#}", fanout_result);

        let ctx = context.lock();
        Ok(OutboundSetValueResult {
            fanout_result,
            signed_value_data: ctx.value.clone(),
        })
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub(super) async fn process_outbound_set_value_result_locked(
        &self,
        subkey_lock: &StorageManagerSubkeyLockGuard,
        record_key: RecordKey,
        requested_value_data: ValueData,
        safety_selection: SafetySelection,
        allow_offline: AllowOffline,
        result: set_value::OutboundSetValueResult,
    ) -> VeilidAPIResult<Option<ValueData>> {
        let opaque_record_key = subkey_lock.record();
        let subkey = subkey_lock.subkey();

        // See if this was finished but not
        let was_offline = self.check_fanout_finished_without_consensus(
            &opaque_record_key,
            subkey,
            &result.fanout_result,
        );

        // Keep the list of nodes that returned a value for later reference
        let existed = self.process_fanout_results(
            opaque_record_key.clone(),
            core::iter::once((ValueSubkeyRangeSet::single(subkey), result.fanout_result)),
            true,
            self.config().internal().network.dht.consensus_width as usize,
        )?;

        // Check if the record still exists before setting it locally
        if !existed {
            apibail_key_not_found!(opaque_record_key);
        }

        // Report on fanout result offline
        if was_offline {
            // Failed to write to consensus
            self.handle_offline_set_single_local_value_with_subkey_lock(
                subkey_lock,
                result.signed_value_data.clone(),
                safety_selection.clone(),
                allow_offline,
            )
            .await?;
        } else {
            // Record still exists so set it locally with the result from the network
            self.handle_set_single_local_value_with_subkey_lock(
                subkey_lock,
                result.signed_value_data.clone(),
            )
            .await?;
        }

        let value_data = self
            .maybe_decrypt_value_data(&record_key, result.signed_value_data.value_data())
            .await?;

        // Return the new value if it differs from what was asked to set
        if value_data != requested_value_data {
            return Ok(Some(value_data));
        }

        // If the original value was set, return None
        Ok(None)
    }

    /// Verify the signatures on an inbound descriptor+value before it is stored or reported
    pub(super) async fn verify_inbound_signed_value(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        descriptor: &SignedValueDescriptor,
        value: &SignedValueData,
        subkey: ValueSubkey,
    ) -> NetworkResult<()> {
        let crypto = self.crypto();
        let Some(vcrypto) = crypto.get_async(opaque_record_key.kind()) else {
            return NetworkResult::invalid_message("unsupported cryptosystem");
        };
        if descriptor
            .validate(&vcrypto, opaque_record_key)
            .await
            .is_err()
        {
            return NetworkResult::invalid_message(
                "invalid descriptor signature or record-key binding",
            );
        }
        match value
            .validate(descriptor.ref_owner(), subkey, &vcrypto)
            .await
        {
            Ok(true) => NetworkResult::value(()),
            Ok(false) | Err(_) => {
                NetworkResult::invalid_message("invalid signed value data signature")
            }
        }
    }

    /// Handle a received 'Set Value' query
    /// Returns a None if the value passed in was set
    /// Returns a Some(current value) if the value was older and the current value was kept
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip_all)
    )]
    pub async fn inbound_set_value(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
        value: Arc<SignedValueData>,
        descriptor: Option<Arc<SignedValueDescriptor>>,
        target: Target,
    ) -> VeilidAPIResult<NetworkResult<InboundSetValueResult>> {
        let remote_record_store = self.get_remote_record_store()?;

        // See if the subkey we are modifying has a last known remote value
        let last_get_result = remote_record_store
            .get_subkey(opaque_record_key, subkey, true)
            .await?
            .unwrap_or_default();

        // Make sure this value would actually be newer
        if let Some(last_value) = &last_get_result.opt_value {
            if value.value_data().seq() < last_value.value_data().seq() {
                // inbound value is older than the sequence number that we have, just return the one we have
                return Ok(NetworkResult::value(InboundSetValueResult::Ignored(
                    last_value.clone(),
                )));
            } else if value.value_data().seq() == last_value.value_data().seq() {
                // inbound value is equal to the sequence number that we have
                // if the value is the same including the writer, return nothing,
                // otherwise return the existing value because it was here first
                if value.value_data() == last_value.value_data() {
                    return Ok(NetworkResult::value(InboundSetValueResult::Success));
                }
                // sequence number is the same but there's a value conflict, return what we have
                return Ok(NetworkResult::value(InboundSetValueResult::Ignored(
                    last_value.clone(),
                )));
            }
        }

        // Get the descriptor and schema for the key
        let actual_descriptor = match last_get_result.opt_descriptor {
            Some(last_descriptor) => {
                if let Some(descriptor) = descriptor {
                    // Descriptor must match last one if it is provided
                    if descriptor.cmp_no_sig(&last_descriptor) != cmp::Ordering::Equal {
                        return Ok(NetworkResult::invalid_message(
                            "setvalue descriptor does not match last descriptor",
                        ));
                    }
                } else {
                    // Descriptor was not provided always go with last descriptor
                }
                last_descriptor
            }
            None => {
                if let Some(descriptor) = descriptor {
                    descriptor
                } else {
                    // No descriptor
                    return Ok(NetworkResult::value(InboundSetValueResult::NeedsDescriptor));
                }
            }
        };
        // Verify signatures before storing
        network_result_try!(
            self.verify_inbound_signed_value(opaque_record_key, &actual_descriptor, &value, subkey)
                .await
        );

        let Ok(schema) = actual_descriptor.schema() else {
            return Ok(NetworkResult::invalid_message("invalid schema"));
        };

        // Validate new value with schema
        if self
            .check_subkey_value_data(
                &schema,
                actual_descriptor.ref_owner(),
                subkey,
                value.value_data(),
            )
            .is_err()
        {
            // Validation failed, ignore this value
            return Ok(NetworkResult::invalid_message("failed schema validation"));
        }

        // Do the set and return no new value

        // See if we have a remote record already or not
        if !remote_record_store.contains_record(opaque_record_key) {
            // record didn't exist, make it
            let cur_ts = Timestamp::now();
            let remote_record_detail = RemoteRecordDetail {};
            let record =
                Record::<RemoteRecordDetail>::new(cur_ts, actual_descriptor, remote_record_detail)?;
            remote_record_store
                .new_record(opaque_record_key.clone(), record)
                .await?
        };

        // Write subkey to remote store
        let res = remote_record_store
            .set_single_subkey(
                opaque_record_key,
                subkey,
                value,
                InboundWatchUpdateMode::ExcludeTarget(target),
                CommitActionFlushMode::Deferred,
            )
            .await;

        match res {
            Ok(()) => Ok(NetworkResult::value(InboundSetValueResult::Success)),
            Err(VeilidAPIError::Internal { message }) => Err(VeilidAPIError::Internal { message }),
            Err(e) => Ok(NetworkResult::invalid_message(e)),
        }
    }
}
