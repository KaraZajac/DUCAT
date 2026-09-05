use std::sync::Arc;

use super::*;

impl_veilid_log_facility!("stor");

/// The context of the outbound_transact_begin operation
struct OutboundTransactBeginContext {
    /// The descriptor we have
    pub opt_descriptor: Option<Arc<SignedValueDescriptor>>,
    /// The best sequence numbers so far
    pub seqs: Vec<ValueSeqNum>,
    /// Refs to the routes used by this transaction, held to keep them locked
    pub route_refs: Vec<AllocatedRouteRef>,
    /// Observed RTT per accepted node, used to bound the post-consensus deadline
    pub accepted_rtts: HashMap<NodeId, TimestampDuration>,
}

/// parameters required to begin a transaction
#[derive(Debug, Clone)]
pub(super) struct OutboundTransactBeginParams {
    /// The transaction handle to use
    pub transaction_handle: OutboundTransactionHandle,
    /// The parameters to create the transaction record state
    pub record_params: OutboundTransactionRecordParams,
}

/// The result of the outbound_transact_begin operation
#[derive(Debug)]
pub(super) struct OutboundTransactBeginResult {
    /// The parameters used for this begin
    pub params: Arc<OutboundTransactBeginParams>,
    /// Fanout result
    pub fanout_result: FanoutResult,
    /// The combined list of newest sequence numbers from the transaction nodes
    pub seqs: Vec<ValueSeqNum>,
    /// The descriptor for the record
    pub descriptor: Arc<SignedValueDescriptor>,
    /// Refs to the routes used by this transaction, held to keep them locked
    pub route_refs: Vec<AllocatedRouteRef>,
}

/// The result of the inbound_transact_begin operation
#[derive(Clone, Debug)]
pub(crate) enum InboundTransactBeginResult {
    /// Value transacted successfully
    Success(TransactBeginSuccess),
    /// Transaction unavailable due to limits
    TransactionUnavailable,
    /// Descriptor required but not provided,
    NeedDescriptor,
}

/// The result of a single successful transaction begin
#[derive(Debug, Clone)]
pub(crate) struct TransactBeginSuccess {
    /// Transaction id
    pub transaction_id: InboundTransactionId,
    /// Expiration timestamp
    pub expiration: Timestamp,
    /// Descriptor
    pub opt_descriptor: Option<Arc<SignedValueDescriptor>>,
    /// Sequence numbers for record
    pub seqs: Vec<ValueSeqNum>,
}

impl StorageManager {
    ////////////////////////////////////////////////////////////////////////

    /// Perform a transact begin query on the network for a single record
    /// This routine uses fanout to find nodes and begin their server-side node transactions
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip_all, err)
    )]
    pub(super) async fn outbound_transact_begin(
        &self,
        params: Arc<OutboundTransactBeginParams>,
    ) -> VeilidAPIResult<OutboundTransactBeginResult> {
        let opaque_record_key = params.record_params.record_key.opaque();
        let crypto_kind = opaque_record_key.kind();
        let routing_domain = RoutingDomain::PublicInternet;

        // Get the DHT parameters for 'TransactBegin'
        let config = self.config();
        let (node_count, consensus_width, fanout_tasks) = (
            config.internal().network.dht.max_find_node_count as usize,
            config.internal().network.dht.consensus_width as usize,
            config.internal().network.dht.set_value_fanout as usize,
        );
        let required_strict_consensus_count = params.record_params.required_strict_consensus_count;

        let timeout = self
            .rpc_processor()
            .get_safety_selection_timeout(&params.record_params.safety_selection);

        // Get the nodes we know are caching this value to seed the fanout
        let init_fanout_queue: Vec<NodeRef> = self
            .get_value_nodes(&opaque_record_key)?
            .unwrap_or_default()
            .into_iter()
            .filter(|x| {
                x.node_info(routing_domain)
                    .map(|ni| ni.has_all_capabilities(&[VEILID_CAPABILITY_DHT]))
                    .unwrap_or_default()
            })
            .collect();

        // DIAG: seed value-nodes are the only tx1/tx2 difference across delete+reopen
        veilid_log!(self debug target:"network_result",
            "TransactBegin[{}] seed value-nodes ({}): [{}]",
            opaque_record_key,
            init_fanout_queue.len(),
            init_fanout_queue.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "));

        // Get the descriptor for this record if we have it
        let opt_descriptor = {
            let local_record_store = self.get_local_record_store()?;
            local_record_store.with_record(&opaque_record_key, |record| record.descriptor())?
        };

        // Make operation context, seeded with prior state
        let context = Arc::new(Mutex::new(OutboundTransactBeginContext {
            opt_descriptor,
            seqs: vec![],
            route_refs: Vec::new(),
            accepted_rtts: HashMap::new(),
        }));

        let descriptor_cache = self.descriptor_cache.clone();

        // Routine to call to generate fanout
        let call_routine = {
            let context = context.clone();
            let registry = self.registry();
            let descriptor_cache = descriptor_cache.clone();
            let params = params.clone();
            let opaque_record_key = opaque_record_key.clone();

            Arc::new(
                move |next_node: NodeRef| -> PinBoxFutureStatic<FanoutCallResult> {
                    let context = context.clone();
                    let registry = registry.clone();
                    let descriptor_cache = descriptor_cache.clone();
                    let params = params.clone();
                    let opaque_record_key = opaque_record_key.clone();

                    let fut = async move {
                        let rpc_processor = registry.rpc_processor();
                        let storage_manager = registry.storage_manager();

                        // check the cache to see if we should send the descriptor
                        let node_id = next_node.node_ids().get(crypto_kind).unwrap_or_log();
                        let dc_key = DescriptorCacheKey {
                            opaque_record_key: opaque_record_key.clone(),
                            node_id,
                        };
                        let mut descriptor_mode = DescriptorMode::new(
                            descriptor_cache.lock().get(&dc_key).is_none(),
                            context.lock().opt_descriptor.clone(),
                        );

                        let dest = Destination::direct(
                            next_node.routing_domain_filtered(routing_domain),
                            Some(params.record_params.safety_selection.clone()),
                        );

                        // Time the begin RPC round-trip so subsequent TransactCommand
                        // calls can use it as an RTT baseline.
                        let node_start = Timestamp::now();

                        // send across the wire, with a retry if the remote needed the descriptor
                        let tva = loop {
                            // send across the wire
                            let tva = match rpc_processor
                                .rpc_call_transact_begin(
                                    dest.clone(),
                                    opaque_record_key.clone(),
                                    descriptor_mode.clone(),
                                    params.record_params.signing_keypair.clone(),
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
                            if tva.answer.accepted && tva.answer.descriptor_mode.is_want() {
                                match descriptor_mode {
                                    DescriptorMode::Want => {
                                        // If both sides want the descriptor but do not have it then the record does not exist
                                    }
                                    DescriptorMode::Have(signed_value_descriptor) => {
                                        // If the server wants the descriptor and we have it, then send it
                                        descriptor_mode =
                                            DescriptorMode::Send(signed_value_descriptor);

                                        veilid_log!(registry debug target:"network_result", "Retrying to send descriptor");
                                        // Hold the route refs used by this transaction
                                        {
                                            let mut ctx = context.lock();
                                            ctx.route_refs.extend(
                                                tva.answer_context
                                                    .waitable_reply_context
                                                    .into_allocated_route_refs(),
                                            );
                                        }
                                        continue;
                                    }
                                    DescriptorMode::Send(_) => {
                                        // If the server wants the descriptor and we already sent it, then something is wrong
                                        veilid_log!(registry error target:"network_result", "Got 'need_descriptor' when descriptor was already sent: node={} record_key={}", next_node, opaque_record_key);
                                    }
                                }
                            }

                            break tva;
                        };

                        let node_elapsed = Timestamp::now().duration_since(node_start);

                        // Hold the route refs used by this transaction
                        {
                            let mut ctx = context.lock();
                            ctx.route_refs.extend(
                                tva.answer_context
                                    .waitable_reply_context
                                    .into_allocated_route_refs(),
                            );
                        }

                        let answer = tva.answer;

                        // Check if we got an accepted result
                        if !answer.accepted {
                            // Return peers if we have some
                            veilid_log!(registry debug target:"network_result", "TransactBegin missed, fanout call returned peers {}", answer.peers.len());
                            return Ok(FanoutCallOutput {
                                peer_info_list: answer.peers,
                                disposition: FanoutCallDisposition::Rejected,
                            });
                        }

                        // Get the transaction id
                        let Some(xid) = answer.transaction_id else {
                            veilid_log!(registry debug target:"network_result", "TransactBegin accepted but returned no transaction id, try again later");
                            return Ok(FanoutCallOutput {
                                peer_info_list: answer.peers,
                                disposition: FanoutCallDisposition::Rejected,
                            });
                        };

                        // If the node was close enough to accept the value and we got a transaction id
                        let descriptor = {
                            let mut ctx = context.lock();

                            // Get the descriptor and cache if we sent the descriptor or if we received one
                            let Some(descriptor) = ctx
                                .opt_descriptor
                                .clone()
                                .or(answer.descriptor_mode.opt_arc_descriptor())
                            else {
                                // Record does not exist
                                veilid_log!(registry debug target:"network_result", "TransactBegin record did not exist, fanout call returned peers {}", answer.peers.len());
                                return Ok(FanoutCallOutput {
                                    peer_info_list: answer.peers,
                                    disposition: FanoutCallDisposition::Rejected,
                                });
                            };
                            if descriptor_mode.is_send()
                                || answer.descriptor_mode.is_send()
                                || answer.descriptor_mode.is_have()
                            {
                                descriptor_cache.lock().insert(dc_key, ());
                            }

                            let schema = match descriptor.schema() {
                                Ok(s) => s,
                                Err(_) => {
                                    veilid_log!(registry debug target:"network_result", "TransactBegin received invalid schema");
                                    return Ok(FanoutCallOutput {
                                        peer_info_list: vec![],
                                        disposition: FanoutCallDisposition::Invalid,
                                    });
                                }
                            };
                            let subkey_count = schema.subkey_count();

                            // Get the sequence number state at the point of the transaction
                            if answer.seqs.len() != subkey_count {
                                veilid_log!(registry debug target:"network_result", "wrong number of seqs returned {} (wanted {})",
                                    answer.seqs.len(),
                                    subkey_count);
                                return Ok(FanoutCallOutput {
                                    peer_info_list: answer.peers,
                                    disposition: FanoutCallDisposition::Invalid,
                                });
                            }

                            #[cfg(feature = "verbose-tracing")]
                            veilid_log!(registry debug "Begin fanout accepted: record={} node={} xid={} seqs={}", opaque_record_key, next_node, xid, answer.seqs.to_table_string());

                            // Update descriptor in context so we don't send/want it more than necessary
                            ctx.opt_descriptor = Some(descriptor.clone());

                            // If we have a prior seqs list, merge in the new seqs
                            if ctx.seqs.is_empty() {
                                ctx.seqs = answer.seqs.clone()
                            } else {
                                for pair in ctx.seqs.iter_mut().zip(answer.seqs.iter()) {
                                    let ctx_seq = pair.0;
                                    let answer_seq = *pair.1;

                                    ctx_seq.max_assign(answer_seq);
                                }
                            }

                            descriptor
                        };

                        // Add transaction id node to record state immediately
                        // rather than waiting for the fanout to complete, so we can start handling the keepalives right away
                        {
                            let mut inner = storage_manager.inner.lock();

                            let otm = &mut inner.outbound_transaction_manager;
                            otm.add_node_transaction(AddNodeTransactionParams {
                                transaction_handle: params.transaction_handle,
                                opaque_record_key: opaque_record_key.clone(),
                                xid,
                                node_ref: next_node.clone(),
                                expiration: answer.expiration,
                                dest,
                                descriptor,
                                opt_begin_rtt: Some(node_elapsed),
                                begin_seqs: answer.seqs.clone(),
                            })
                            .map_err(|e| RPCError::internal(e.to_string()))?;
                        }

                        // Record this accepted node's RTT so the fanout can compute
                        // a tight post-consensus deadline from real measurements.
                        if let Some(node_id) = next_node.node_ids().get(crypto_kind) {
                            context.lock().accepted_rtts.insert(node_id, node_elapsed);
                        }

                        // Return peers if we have some
                        veilid_log!(registry debug target:"network_result", "TransactBegin fanout call returned peers {}", answer.peers.len());

                        // Transact doesn't actually use the fanout queue consensus tracker
                        Ok(FanoutCallOutput {
                            peer_info_list: answer.peers,
                            disposition: FanoutCallDisposition::Accepted,
                        })
                    };
                    #[cfg(feature = "instrument")]
                    let fut = fut.instrument(tracing::trace_span!(
                        target: "dht",
                        "outbound_begin_transact_value fanout call"
                    ));
                    Box::pin(fut) as PinBoxFuture<FanoutCallResult>
                },
            )
        };

        // Number of accepters Begin aims to collect: consensus_count plus the
        // CONSENSUS_TRIM_PERCENT headroom. Once reached, stop all lanes; the
        // post-consensus deadline keeps gathering accepts up to this cap.
        let safety_count = required_strict_consensus_count
            + (required_strict_consensus_count * CONSENSUS_TRIM_PERCENT).div_ceil(100);
        let check_done = {
            Arc::new(
                move |fanout_result: &FanoutResult| match fanout_result.kind {
                    FanoutResultKind::Incomplete => FanoutDoneDisposition::NotDone,
                    FanoutResultKind::Timeout | FanoutResultKind::Exhausted => {
                        FanoutDoneDisposition::DoneEarly
                    }
                    FanoutResultKind::Consensus => {
                        if fanout_result.consensus_nodes.len() >= safety_count {
                            FanoutDoneDisposition::DoneEarly
                        } else {
                            FanoutDoneDisposition::Done
                        }
                    }
                },
            )
        };

        // Bound the post-consensus wait by `POST_CONSENSUS_RTT_FACTOR × max(fastest
        // consensus_count RTTs)` (capped at `rpc.timeout_ms`). Using only the
        // fastest consensus_count keeps a single slow accepter from inflating
        // the deadline up to the rpc-timeout cap.
        let rpc_timeout =
            TimestampDuration::new_ms(config.internal().network.rpc.timeout_ms.into());
        let post_consensus_timeout_callback: FanoutPostConsensusTimeoutCallback = {
            let context = context.clone();
            Arc::new(move |result: &FanoutResult| -> TimestampDuration {
                let rtts_map = context.lock().accepted_rtts.clone();
                let mut consensus_rtts: Vec<TimestampDuration> = result
                    .consensus_nodes
                    .iter()
                    .filter_map(|nr| {
                        nr.node_ids()
                            .get(crypto_kind)
                            .and_then(|nid| rtts_map.get(&nid).copied())
                    })
                    .collect();
                consensus_rtts.sort_unstable();
                let take = required_strict_consensus_count.min(consensus_rtts.len());
                let bound_rtt = consensus_rtts
                    .into_iter()
                    .take(take)
                    .max()
                    .unwrap_or_else(|| TimestampDuration::new_ms(0));
                let bound = bound_rtt.saturating_mul(POST_CONSENSUS_RTT_FACTOR);
                if bound < rpc_timeout {
                    bound
                } else {
                    rpc_timeout
                }
            })
        };

        // Call the fanout
        let routing_table = self.routing_table();
        let hash_coordinate = opaque_record_key.to_hash_coordinate();
        let fanout_call = FanoutCall::new(
            &routing_table,
            FanoutCallParams {
                name: format!(
                    "transact_begin({} @ {})",
                    opaque_record_key,
                    Timestamp::now_increasing()
                ),
                hash_coordinate,
                node_count,
                fanout_tasks,
                consensus_count: required_strict_consensus_count,
                consensus_width,
                timeout,
            },
            capability_fanout_peer_info_filter(vec![VEILID_CAPABILITY_DHT]),
            call_routine,
            check_done,
        )
        .with_post_consensus_timeout_callback(post_consensus_timeout_callback);

        let shutdown_stop_source = self
            .startup_lock
            .stop_token()
            .ok_or_else(VeilidAPIError::not_initialized)?;

        let fanout_result = fanout_call
            .run(
                init_fanout_queue,
                FanoutQueueMode::ThrottleAtConsensus,
                shutdown_stop_source,
            )
            .await?;

        let mut ctx = context.lock();

        veilid_log!(self debug target: "network_result",
            "TransactBegin[{}] Fanout result: {:#} value_nodes=[{}]",
            opaque_record_key,
            fanout_result,
            fanout_result.value_nodes.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "));

        let descriptor = ctx.opt_descriptor.clone().unwrap_or_log();
        let seqs = if fanout_result.value_nodes.is_empty() {
            #[cfg(feature = "verbose-tracing")]
            veilid_log!(self debug "Fanout for {} had no value_nodes, using default seqs (kind={:?})",
                opaque_record_key, fanout_result.kind);
            vec![ValueSeqNum::NONE; descriptor.schema().unwrap_or_log().subkey_count()]
        } else {
            #[cfg(feature = "verbose-tracing")]
            veilid_log!(self debug "Fanout for {} completed: kind={:?}, value_nodes={}, ctx.seqs={}",
                opaque_record_key,
                fanout_result.kind,
                fanout_result.value_nodes.len(),
                ctx.seqs.to_table_string()
            );
            ctx.seqs.clone()
        };

        let result = OutboundTransactBeginResult {
            params,
            fanout_result,
            seqs,
            descriptor,
            route_refs: core::mem::take(&mut ctx.route_refs),
        };

        Ok(result)
    }

    ////////////////////////////////////////////////////////////////////////

    /// Handle a received 'TransactBegin' query
    #[cfg_attr(feature = "instrument", instrument(level = "debug", target = "dht", ret(Display), err, fields(duration, __VEILID_LOG_KEY = self.log_key(), opt_descriptor = opt_descriptor.is_some()), skip(self, opt_descriptor)))]
    pub async fn inbound_transact_begin(
        &self,
        opaque_record_key: OpaqueRecordKey,
        opt_descriptor: Option<SignedValueDescriptor>,
        want_descriptor: bool,
        signing_member_id: MemberId,
    ) -> VeilidAPIResult<NetworkResult<InboundTransactBeginResult>> {
        record_duration_fut(async {
            // Can't provide descriptor and want descriptor
            if opt_descriptor.is_some() && want_descriptor {
                return VeilidAPIResult::Ok(NetworkResult::invalid_message(
                    "can't provide descriptor and want descriptor",
                ));
            }

            let remote_record_store = self.get_remote_record_store()?;

            remote_record_store
                .begin_inbound_transaction(
                    &opaque_record_key,
                    opt_descriptor,
                    want_descriptor,
                    signing_member_id,
                )
                .await
                .map(NetworkResult::value)
        })
        .await
    }
}
