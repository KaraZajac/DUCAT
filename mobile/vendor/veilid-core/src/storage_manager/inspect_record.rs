use super::*;

impl_veilid_log_facility!("stor");

/// The fully parsed descriptor
struct InspectDescriptorInfo {
    /// The descriptor itself
    descriptor: Arc<SignedValueDescriptor>,

    /// The in-schema subkeys that overlap the inspected range
    subkeys: ValueSubkeyRangeSet,
}

impl InspectDescriptorInfo {
    pub fn new(
        descriptor: Arc<SignedValueDescriptor>,
        subkeys: &ValueSubkeyRangeSet,
    ) -> VeilidAPIResult<Self> {
        let schema = descriptor.schema().map_err(RPCError::invalid_format)?;
        let subkeys = schema.truncate_subkeys(subkeys, Some(DHTSchema::MAX_SUBKEY_COUNT));
        Ok(Self {
            descriptor,
            subkeys,
        })
    }
}

/// Info tracked per subkey
struct SubkeySeqCount {
    /// The newest sequence number found for a subkey
    pub seq: ValueSeqNum,
    /// The set of nodes that had the most recent value for this subkey
    pub consensus_nodes: Vec<NodeRef>,
    /// The set of nodes that had any value for this subkey
    pub value_nodes: Vec<NodeRef>,
}

/// The context of the outbound_inspect_value operation
struct OutboundInspectValueContext {
    /// The combined sequence numbers and result counts so far
    pub seqcounts: Vec<SubkeySeqCount>,
    /// The descriptor if we got a fresh one or empty if no descriptor was needed
    pub opt_descriptor_info: Option<InspectDescriptorInfo>,
}

/// The result of the outbound_inspect_value operation
#[derive(Debug, Clone)]
pub(in crate::storage_manager) struct OutboundInspectValueResult {
    /// Fanout results for each subkey
    pub subkey_fanout_results: Vec<FanoutResult>,
    /// The inspection that was retrieved
    pub inspect_result: InspectResult,
}

/// The result of the inbound_inspect_value operation
#[derive(Clone, Debug)]
pub(crate) enum InboundInspectValueResult {
    /// Value inspected successfully
    Success(InspectResult),
}

impl StorageManager {
    /// Inspect an opened DHT record for its subkey sequence numbers
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn inspect_record(
        &self,
        record_key: RecordKey,
        subkeys: ValueSubkeyRangeSet,
        scope: DHTReportScope,
    ) -> VeilidAPIResult<DHTRecordReport> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let opaque_record_key = record_key.opaque();

        let subkeys = if subkeys.is_empty() {
            ValueSubkeyRangeSet::full()
        } else {
            subkeys
        };

        // Local scope skips peek_lock; inner RecordStore lock keeps local reads consistent
        let is_local_scope = match scope {
            DHTReportScope::Local => true,
            DHTReportScope::SyncGet
            | DHTReportScope::SyncSet
            | DHTReportScope::UpdateGet
            | DHTReportScope::UpdateSet => false,
        };

        let _opt_peek_lock = if is_local_scope {
            None
        } else {
            Some(
                self.record_lock_table
                    .peek_lock(opaque_record_key.clone())
                    .measure_debug(
                        TimestampDuration::new_ms(200),
                        veilid_log_dbg!(self, "StorageManager::inspect_record lock"),
                    )
                    .await,
            )
        };

        let (safety_selection, offline_subkey_writes) = {
            let inner = self.inner.lock();
            let Some(opened_record) = inner.opened_records.get(&opaque_record_key) else {
                apibail_invalid_argument!("record not open", "record_key", opaque_record_key);
            };
            let safety_selection = opened_record.safety_selection();

            let in_flight_transaction = inner
                .outbound_transaction_manager
                .get_transaction_by_record(&opaque_record_key)
                .is_some();

            let purpose_state = self
                .record_lock_table
                .get_record_lock_purpose_state(&opaque_record_key);

            let mut active_subkey_writes = ValueSubkeyRangeSet::new();
            let mut transaction_record_lock = false;
            if let Some(p) = &purpose_state.whole_record_lock_purpose {
                match p {
                    StorageManagerRecordLockPurpose::TransactBegin
                    | StorageManagerRecordLockPurpose::TransactExtend
                    | StorageManagerRecordLockPurpose::TransactEndAndCommit
                    | StorageManagerRecordLockPurpose::TransactRollback
                    | StorageManagerRecordLockPurpose::TransactDrop => {
                        transaction_record_lock = true;
                        if !in_flight_transaction {
                            veilid_log!(self debug
                                "whole-record lock purpose {:?} on {} without an in-flight transaction",
                                p, opaque_record_key);
                        }
                    }
                    StorageManagerRecordLockPurpose::Create
                    | StorageManagerRecordLockPurpose::Open
                    | StorageManagerRecordLockPurpose::Close
                    | StorageManagerRecordLockPurpose::Delete
                    | StorageManagerRecordLockPurpose::Watch => {}
                }
            }

            if in_flight_transaction || transaction_record_lock {
                active_subkey_writes = ValueSubkeyRangeSet::full();
            } else if purpose_state.whole_record_lock_purpose.is_none() {
                for (subkey, purpose) in purpose_state.subkey_lock_purpose.into_iter() {
                    match purpose {
                        StorageManagerSubkeyLockPurpose::TransactSet => {
                            if !in_flight_transaction {
                                veilid_log!(self debug
                                    "subkey lock purpose TransactSet on {}/{} without an in-flight transaction",
                                    opaque_record_key, subkey);
                            }
                            active_subkey_writes.insert(subkey);
                        }
                        StorageManagerSubkeyLockPurpose::TransactGet => {
                            if !in_flight_transaction {
                                veilid_log!(self debug
                                    "subkey lock purpose TransactGet on {}/{} without an in-flight transaction",
                                    opaque_record_key, subkey);
                            }
                        }
                        StorageManagerSubkeyLockPurpose::Set => {
                            active_subkey_writes.insert(subkey);
                        }
                        StorageManagerSubkeyLockPurpose::Get => {}
                    }
                }
            }

            let offline_subkey_writes = inner
                .offline_subkey_writes
                .get(&opaque_record_key)
                .map(|o| o.subkeys.union(&o.subkeys_in_flight))
                .unwrap_or_default()
                .union(&active_subkey_writes)
                .intersect(&subkeys);
            (safety_selection, offline_subkey_writes)
        };

        let mut local_inspect_result = self
            .handle_inspect_local_values(opaque_record_key.clone(), subkeys.clone(), true)
            .await?;

        // If this is the maximum scope we're interested in, return the report
        if matches!(scope, DHTReportScope::Local) {
            return DHTRecordReport::new(
                local_inspect_result.subkeys().clone(),
                offline_subkey_writes,
                local_inspect_result.seqs().to_vec(),
                vec![ValueSeqNum::NONE; local_inspect_result.seqs().len()],
            )
            .inspect_err(|e| {
                veilid_log!(self error "invalid record report generated: {}", e);
            });
        }

        // Get rpc processor and drop mutex so we don't block while getting the value from the network
        if !self.dht_is_online() {
            apibail_try_again!("offline, try again later");
        };

        // If we're simulating a set, increase the previous sequence number we have by 1
        if matches!(scope, DHTReportScope::UpdateSet) {
            for seq in local_inspect_result.seqs_mut() {
                if let Ok(next) = seq.next() {
                    *seq = next;
                }
            }
        }

        // Get the inspect record report from the network
        let outbound_inspect_result = self
            .outbound_inspect_value(
                &opaque_record_key,
                subkeys,
                safety_selection,
                if matches!(scope, DHTReportScope::SyncGet | DHTReportScope::SyncSet) {
                    InspectResult::default()
                } else {
                    local_inspect_result.clone()
                },
                matches!(scope, DHTReportScope::UpdateSet | DHTReportScope::SyncSet),
            )
            .await?;

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
                "record was locked for inspect but is now missing: {}",
                opaque_record_key
            );
        }

        if outbound_inspect_result.inspect_result.subkeys().is_empty() {
            DHTRecordReport::new(
                local_inspect_result.subkeys().clone(),
                offline_subkey_writes,
                local_inspect_result.seqs().to_vec(),
                vec![ValueSeqNum::NONE; local_inspect_result.seqs().len()],
            )
        } else {
            DHTRecordReport::new(
                outbound_inspect_result.inspect_result.subkeys().clone(),
                offline_subkey_writes,
                local_inspect_result.seqs().to_vec(),
                outbound_inspect_result.inspect_result.seqs().to_vec(),
            )
        }
    }

    ////////////////////////////////////////////////////////////////////////

    /// Perform a 'inspect value' query on the network
    /// Performs the work without a transaction
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) async fn outbound_inspect_value(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        safety_selection: SafetySelection,
        local_inspect_result: InspectResult,
        use_set_scope: bool,
    ) -> VeilidAPIResult<OutboundInspectValueResult> {
        let routing_domain = RoutingDomain::PublicInternet;
        let requested_subkeys = subkeys.clone();

        // Get the DHT parameters for 'InspectValue'
        // Can use either 'get scope' or 'set scope' depending on the purpose of the inspection
        let (node_count, consensus_count, consensus_width, fanout_tasks, timeout) = if use_set_scope
        {
            let config = self.config();
            (
                config.internal().network.dht.max_find_node_count as usize,
                config.internal().network.dht.set_value_count as usize,
                config.internal().network.dht.consensus_width as usize,
                config.internal().network.dht.set_value_fanout as usize,
                TimestampDuration::from(ms_to_us(
                    config.internal().network.dht.set_value_timeout_ms,
                )),
            )
        } else {
            let config = self.config();
            (
                config.internal().network.dht.max_find_node_count as usize,
                config.internal().network.dht.get_value_count as usize,
                config.internal().network.dht.consensus_width as usize,
                config.internal().network.dht.get_value_fanout as usize,
                TimestampDuration::from(ms_to_us(
                    config.internal().network.dht.get_value_timeout_ms,
                )),
            )
        };

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
        let opt_descriptor_info = if let Some(descriptor) = local_inspect_result.opt_descriptor() {
            // Get the descriptor info. This also truncates the subkeys list to what can be returned from the network.
            Some(InspectDescriptorInfo::new(descriptor, &subkeys)?)
        } else {
            None
        };

        let context = Arc::new(Mutex::new(OutboundInspectValueContext {
            seqcounts: local_inspect_result
                .seqs()
                .iter()
                .map(|s| SubkeySeqCount {
                    seq: *s,
                    consensus_nodes: vec![],
                    value_nodes: vec![],
                })
                .collect(),
            opt_descriptor_info,
        }));

        // Routine to call to generate fanout
        let call_routine = {
            let context = context.clone();
            let registry = self.registry();
            let opaque_record_key = opaque_record_key.clone();
            let safety_selection = safety_selection.clone();
            Arc::new(
                move |next_node: NodeRef| -> PinBoxFutureStatic<FanoutCallResult> {
                    let context = context.clone();
                    let registry = registry.clone();
                    let subkeys = subkeys.clone();
                    let opaque_record_key = opaque_record_key.clone();
                    let safety_selection = safety_selection.clone();
                    let fut = async move {
                        let rpc_processor = registry.rpc_processor();

                        let descriptor_mode = GetDescriptorMode::new(
                            context
                                .lock()
                                .opt_descriptor_info
                                .as_ref()
                                .map(|x| x.descriptor.clone()),
                        );

                        let iva = match rpc_processor
                            .rpc_call_inspect_value(
                                Destination::direct(
                                    next_node.routing_domain_filtered(routing_domain),
                                    Some(safety_selection),
                                ),
                                opaque_record_key.clone(),
                                subkeys.clone(),
                                descriptor_mode,
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

                        let answer = iva.answer;

                        // Check if we got an accepted result
                        if !answer.accepted {
                            // Return peers if we have some
                            veilid_log!(registry debug target:"network_result", "InspectValue missed, fanout call returned peers {}", answer.peers.len());
                            return Ok(FanoutCallOutput {
                                peer_info_list: answer.peers,
                                disposition: FanoutCallDisposition::Rejected,
                            });
                        }

                        // Keep the descriptor if we got one. If we had a last_descriptor it will
                        // already be validated by rpc_call_inspect_value
                        if let Some(descriptor) = answer.descriptor {
                            let mut ctx = context.lock();
                            if ctx.opt_descriptor_info.is_none() {
                                // Get the descriptor info. This also truncates the subkeys list to what can be returned from the network.
                                let descriptor_info = match InspectDescriptorInfo::new(
                                    Arc::new(descriptor.clone()),
                                    &subkeys,
                                ) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        veilid_log!(registry debug target:"network_result", "InspectValue returned an invalid descriptor: {}", e);
                                        return Ok(FanoutCallOutput {
                                            peer_info_list: vec![],
                                            disposition: FanoutCallDisposition::Invalid,
                                        });
                                    }
                                };
                                ctx.opt_descriptor_info = Some(descriptor_info);
                            }
                        }

                        // Keep the value if we got one and it is newer and it passes schema validation
                        if answer.seqs.is_empty() {
                            veilid_log!(registry debug target:"network_result", "InspectValue returned no seq, fanout call returned peers {}", answer.peers.len());
                            return Ok(FanoutCallOutput {
                                peer_info_list: answer.peers,
                                disposition: FanoutCallDisposition::Rejected,
                            });
                        }

                        veilid_log!(registry debug target:"network_result", "Got seqs back: len={}", answer.seqs.len());
                        let mut ctx = context.lock();

                        // Ensure we have a schema and descriptor etc
                        let Some(descriptor_info) = &ctx.opt_descriptor_info else {
                            // Got a value but no descriptor for it
                            // Move to the next node
                            veilid_log!(registry debug target:"network_result", "InspectValue returned a value with no descriptor invalid descriptor");
                            return Ok(FanoutCallOutput {
                                peer_info_list: vec![],
                                disposition: FanoutCallDisposition::Invalid,
                            });
                        };

                        // Get number of subkeys from schema and ensure we are getting the
                        // right number of sequence numbers betwen that and what we asked for
                        #[allow(clippy::unnecessary_cast)]
                        if answer.seqs.len() as u64 != descriptor_info.subkeys.len() as u64 {
                            // Not the right number of sequence numbers
                            // Move to the next node
                            veilid_log!(registry debug target:"network_result", "wrong number of seqs returned {} (wanted {})",
                                answer.seqs.len(),
                                descriptor_info.subkeys.len());
                            return Ok(FanoutCallOutput {
                                peer_info_list: vec![],
                                disposition: FanoutCallDisposition::Invalid,
                            });
                        }

                        // If we have a prior seqs list, merge in the new seqs
                        if ctx.seqcounts.is_empty() {
                            ctx.seqcounts = answer
                                .seqs
                                .iter()
                                .map(|s| SubkeySeqCount {
                                    seq: *s,
                                    // One node has shown us the newest sequence numbers so far
                                    consensus_nodes: vec![next_node.clone()],
                                    value_nodes: vec![next_node.clone()],
                                })
                                .collect();
                        } else {
                            if ctx.seqcounts.len() != answer.seqs.len() {
                                veilid_log!(registry debug target:"network_result", "seqs list length should always be equal by now: {} (wanted {})",
                                    answer.seqs.len(),
                                    ctx.seqcounts.len());
                                return Ok(FanoutCallOutput {
                                    peer_info_list: vec![],
                                    disposition: FanoutCallDisposition::Invalid,
                                });
                            }
                            for pair in ctx.seqcounts.iter_mut().zip(answer.seqs.iter()) {
                                let ctx_seqcnt = pair.0;
                                let answer_seq = *pair.1;

                                // If we already have consensus for this subkey, don't bother updating it any more
                                // While we may find a better sequence number if we keep looking, this does not mimic the behavior
                                // of get and set unless we stop here
                                if ctx_seqcnt.consensus_nodes.len() >= consensus_count {
                                    continue;
                                }

                                // If the new seq isn't undefined and is better than the old seq (either greater or old is undefined)
                                // Then take that sequence number and note that we have gotten newer sequence numbers so we keep
                                // looking for consensus
                                // If the sequence number matches the old sequence number, then we keep the value node for reference later
                                if answer_seq.is_some() {
                                    if answer_seq > ctx_seqcnt.seq {
                                        // One node has shown us the latest sequence numbers so far
                                        ctx_seqcnt.seq = answer_seq;
                                        ctx_seqcnt.consensus_nodes = vec![next_node.clone()];
                                    } else if answer_seq == ctx_seqcnt.seq {
                                        // Keep the nodes that showed us the latest values
                                        ctx_seqcnt.consensus_nodes.push(next_node.clone());
                                    }
                                }
                                ctx_seqcnt.value_nodes.push(next_node.clone());
                            }
                        }

                        // Return peers if we have some
                        veilid_log!(registry debug target:"network_result", "InspectValue fanout call returned peers {}", answer.peers.len());

                        // Inspect doesn't actually use the fanout queue consensus tracker
                        Ok(FanoutCallOutput {
                            peer_info_list: answer.peers,
                            disposition: FanoutCallDisposition::Accepted,
                        })
                    };
                    #[cfg(feature = "instrument")]
                    let fut = fut.instrument(tracing::trace_span!(
                        target: "dht",
                        "outbound_inspect_value fanout call"
                    ));
                    Box::pin(fut) as PinBoxFuture<FanoutCallResult>
                },
            )
        };

        // Routine to call to check if we're done at each step
        // For inspect, we are tracking consensus externally from the FanoutCall,
        // for each subkey, rather than a single consensus, so the single fanoutresult
        // that is passed in here is ignored in favor of our own per-subkey tracking
        let check_done = {
            let context = context.clone();
            Arc::new(move |_: &FanoutResult| {
                // If we have reached sufficient consensus on all subkeys, return done
                let ctx = context.lock();
                let mut has_consensus = true;
                for cs in ctx.seqcounts.iter() {
                    if cs.consensus_nodes.len() < consensus_count {
                        has_consensus = false;
                        break;
                    }
                }

                if !ctx.seqcounts.is_empty() && ctx.opt_descriptor_info.is_some() && has_consensus {
                    FanoutDoneDisposition::DoneEarly
                } else {
                    FanoutDoneDisposition::NotDone
                }
            })
        };

        // Call the fanout
        let routing_table = self.routing_table();
        let hash_coordinate = opaque_record_key.to_hash_coordinate();
        let fanout_call = FanoutCall::new(
            &routing_table,
            FanoutCallParams {
                name: format!("outbound_inspect_value({})", Timestamp::now_increasing()),
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

        let shutdown_stop_source = self
            .startup_lock
            .stop_token()
            .ok_or_else(VeilidAPIError::not_initialized)?;
        let fanout_result = fanout_call
            .run(
                init_fanout_queue,
                FanoutQueueMode::Unthrottled,
                shutdown_stop_source,
            )
            .await?;

        let ctx = context.lock();
        let mut subkey_fanout_results = vec![];
        for cs in &ctx.seqcounts {
            let has_consensus = cs.consensus_nodes.len() >= consensus_count;
            let subkey_fanout_result = FanoutResult {
                kind: if has_consensus {
                    FanoutResultKind::Consensus
                } else {
                    fanout_result.kind
                },
                consensus_nodes: cs.consensus_nodes.clone(),
                value_nodes: cs.value_nodes.clone(),
            };
            subkey_fanout_results.push(subkey_fanout_result);
        }

        if subkey_fanout_results.len() == 1 {
            veilid_log!(self debug "InspectValue Fanout: {:#}\n{:#}", fanout_result, subkey_fanout_results.first().unwrap_or_log());
        } else {
            veilid_log!(self debug "InspectValue Fanout: {:#}:\n{}", fanout_result, debug_fanout_results(&subkey_fanout_results));
        }

        let result = OutboundInspectValueResult {
            subkey_fanout_results,
            inspect_result: InspectResult::new(
                self,
                requested_subkeys,
                "outbound_inspect_value",
                ctx.opt_descriptor_info
                    .as_ref()
                    .map(|d| d.subkeys.clone())
                    .unwrap_or_default(),
                ctx.seqcounts.iter().map(|cs| cs.seq).collect(),
                ctx.opt_descriptor_info
                    .as_ref()
                    .map(|d| d.descriptor.clone()),
            )?,
        };

        #[allow(clippy::unnecessary_cast)]
        {
            if result.inspect_result.subkeys().len() as u64
                != result.subkey_fanout_results.len() as u64
            {
                veilid_log!(self error "mismatch between subkeys returned and fanout results returned: {}!={}", result.inspect_result.subkeys().len(), result.subkey_fanout_results.len());
                apibail_internal!("subkey and fanout list length mismatched");
            }
        }

        Ok(result)
    }

    /// Handle a received 'Inspect Value' query
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn inbound_inspect_value(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        want_descriptor: bool,
    ) -> VeilidAPIResult<NetworkResult<InboundInspectValueResult>> {
        let subkeys = if subkeys.is_empty() {
            ValueSubkeyRangeSet::full()
        } else {
            subkeys
        };

        // See if the subkey we are getting has a last known remote value
        let remote_record_store = self.get_remote_record_store()?;
        let inspect_result = remote_record_store
            .inspect_record(opaque_record_key, &subkeys, want_descriptor)
            .await?
            .unwrap_or_default();

        Ok(NetworkResult::value(InboundInspectValueResult::Success(
            inspect_result,
        )))
    }
}
