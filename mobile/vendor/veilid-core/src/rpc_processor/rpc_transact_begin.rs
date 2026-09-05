use super::*;

impl_veilid_log_facility!("rpc");

#[derive(Clone, Debug)]
pub struct TransactBeginAnswer {
    pub accepted: bool,
    pub descriptor_mode: DescriptorMode,
    pub transaction_id: Option<u64>,
    pub expiration: Timestamp,
    pub seqs: Vec<ValueSeqNum>,
    pub peers: Vec<Arc<PeerInfo>>,
}

impl RPCProcessor {
    /// Sends an transact begin request and wait for response
    /// Can be sent via all methods including relays
    /// Safety routes may be used, but never private routes.
    /// Because this leaks information about the identity of the node itself,
    /// replying to this request received over a private route will leak
    /// the identity of the node and defeat the private route.
    /// The number of subkey sequence numbers returned may either be:
    ///  * an amount truncated to MAX_TRANSACT_BEGIN_A_SEQS_LEN subkeys
    ///  * zero if nothing was found
    #[
        instrument(level = "trace", target = "rpc", skip(self, descriptor_mode),
            fields(
                ret.peers.len,
                ret.latency,
                ret.accepted
            ),err(level=Level::DEBUG))
    ]
    pub async fn rpc_call_transact_begin(
        &self,
        dest: Destination,
        opaque_record_key: OpaqueRecordKey,
        descriptor_mode: DescriptorMode,
        signing_keypair: KeyPair,
    ) -> RPCNetworkResult<Answer<TransactBeginAnswer>> {
        let _guard = self
            .startup_context
            .startup_lock
            .enter()
            .map_err(RPCError::map_try_again("not started up"))?;

        // Ensure destination never has a private route
        // and get the target noderef so we can validate the response
        let Some(target_node_ids) = dest.get_target_node_ids() else {
            return Err(RPCError::internal(
                "Never send transact value requests over private routes",
            ));
        };

        // Get the target node id
        let Some(target_node_id) = target_node_ids.get(opaque_record_key.kind()) else {
            return Err(RPCError::internal("No node id for crypto kind"));
        };

        let debug_string = format!(
            "OUT ==> TransactBeginQ({} {}) => {:#} (signer={}) ",
            opaque_record_key,
            descriptor_mode,
            dest,
            signing_keypair.key(),
        );

        // Process descriptor mode
        let (last_descriptor, send_descriptor, want_descriptor) = match descriptor_mode {
            DescriptorMode::Want => (None, false, true),
            DescriptorMode::Have(ref d) => (Some(d.clone()), false, false),
            DescriptorMode::Send(ref d) => (Some(d.clone()), true, false),
        };

        // Send the transact begin question
        let transact_value_q = RPCOperationTransactBeginQ::new(
            opaque_record_key.clone(),
            if send_descriptor {
                last_descriptor.as_ref().map(|x| x.as_ref().clone())
            } else {
                None
            },
            want_descriptor,
        )?;
        let question = RPCQuestion::new(
            network_result_try!(self.get_destination_respond_to(&dest).await?),
            RPCQuestionDetail::TransactBeginQ(Box::new(transact_value_q)),
        );

        let question_context = QuestionContext::TransactBegin(ValidateTransactBeginContext {
            opaque_record_key: opaque_record_key.clone(),
            descriptor_mode,
        });

        veilid_log!(self debug target: "dht", "{}", debug_string);

        let waitable_reply = network_result_try!(self
            .question(
                dest.clone(),
                question,
                Some(signing_keypair),
                Some(question_context),
            )
            .await?
            .into_network_result());

        // Wait for reply
        let (msg, answer_context) = match self.wait_for_reply(waitable_reply, debug_string).await? {
            TimeoutOr::Timeout => return Ok(NetworkResult::Timeout),
            TimeoutOr::Value(v) => v,
        };

        // Get the right answer type
        let (_, _, kind) = msg.operation.destructure();
        let transact_begin_a = match kind {
            RPCOperationKind::Answer(a) => match a.destructure() {
                RPCAnswerDetail::TransactBeginA(a) => a,
                _ => return Ok(NetworkResult::invalid_message("not a transactbegin answer")),
            },
            _ => return Ok(NetworkResult::invalid_message("not an answer")),
        };

        let (accepted, need_descriptor, opt_descriptor, transaction_id, duration, seqs, peers) =
            transact_begin_a.destructure();

        if debug_target_enabled!("dht") {
            let debug_string_answer = format!(
                "OUT <== TransactBeginA({} {}{}{} @{:#} peers={}) <= {} (latency={:#}) seqs:{}",
                opaque_record_key,
                if let Some(xid) = transaction_id {
                    format!("xid={} ", xid)
                } else {
                    "".to_string()
                },
                if accepted { " +accept" } else { "" },
                if need_descriptor { " +needdesc" } else { "" },
                duration,
                peers.len(),
                dest,
                answer_context.latency,
                seqs.to_table_string()
            );

            veilid_log!(self debug target: "dht", "{}", debug_string_answer);

            let peer_ids: Vec<String> = peers
                .iter()
                .filter_map(|p| {
                    p.node_ids()
                        .get(opaque_record_key.kind())
                        .map(|k| k.to_string())
                })
                .collect();
            veilid_log!(self debug target: "dht", "Peers: {:#?}", peer_ids);
        }

        // Validate peers returned are, in fact, closer to the key than the node we sent this to
        let valid = match self.routing_table().verify_peer_infos_closer(
            target_node_id.to_hash_coordinate(),
            opaque_record_key.to_hash_coordinate(),
            &peers,
        ) {
            Ok(v) => v,
            Err(e) => {
                return Ok(NetworkResult::invalid_message(format!(
                    "missing cryptosystem in peers node ids: {}",
                    e
                )));
            }
        };
        if !valid {
            return Ok(NetworkResult::invalid_message("non-closer peers returned"));
        }

        let descriptor_mode = if need_descriptor || !accepted {
            DescriptorMode::Want
        } else if let Some(descriptor) = opt_descriptor {
            DescriptorMode::Send(Arc::new(descriptor))
        } else if !want_descriptor {
            DescriptorMode::Have(last_descriptor.unwrap_or_log())
        } else {
            return Ok(NetworkResult::invalid_message(
                "wanted descriptor but did not get one",
            ));
        };

        // Get expiration timestamp
        // Estimates the duration as calculated at a time halfway through the RPC by the remote node
        let expiration = if duration.is_zero() {
            Timestamp::new(0)
        } else {
            answer_context
                .waitable_reply_context
                .send_ts
                .later(answer_context.latency.div(2))
                .later(duration)
        };

        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.latency", answer_context.latency.as_u64());
        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.accepted", accepted);
        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.peers.len", peers.len());

        Ok(NetworkResult::value(Answer::new(
            answer_context,
            TransactBeginAnswer {
                accepted,
                descriptor_mode,
                transaction_id,
                expiration,
                seqs,
                peers,
            },
        )))
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self, msg), fields(msg.operation.op_id), ret, err))]
    pub(super) async fn process_transact_begin_q(&self, msg: Message) -> RPCNetworkResult<()> {
        // Ensure this never came over a private route, safety route is okay though
        if msg.header.is_private_routed() {
            return Ok(NetworkResult::invalid_message(
                "not processing transact value request over private route",
            ));
        }
        let routing_table = self.routing_table();
        let routing_domain = msg.header.routing_domain();

        // Ignore if disabled
        let has_cap_dhtv = routing_table
            .get_published_peer_info(msg.header.routing_domain())
            .map(|ppi| ppi.node_info().has_capability(VEILID_CAPABILITY_DHT))
            .unwrap_or(false);
        if !has_cap_dhtv {
            return Ok(NetworkResult::service_unavailable("dht is not available"));
        }

        // Get the question
        let kind = msg.operation.kind().clone();
        let transact_begin_q = match kind {
            RPCOperationKind::Question(q) => match q.destructure() {
                (_, RPCQuestionDetail::TransactBeginQ(q)) => q,
                _ => {
                    return Ok(NetworkResult::invalid_message(
                        "not a transactbegin question",
                    ))
                }
            },
            _ => return Ok(NetworkResult::invalid_message("not a question")),
        };

        // Destructure
        let (opaque_record_key, opt_descriptor, want_descriptor) = transact_begin_q.destructure();
        let Some(signer) = &msg.opt_signer else {
            return Ok(NetworkResult::invalid_message(
                "transact begin requires signer",
            ));
        };

        // Extract member id for signer
        let Ok(signing_member_id) = self.storage_manager().generate_member_id(signer) else {
            return Ok(NetworkResult::invalid_message(
                "could not generate member id for signer public key",
            ));
        };

        // Get the nodes that we know about that are closer to the the key than our own node
        let closer_to_key_peers = network_result_try!(routing_table
            .get_reliable_nodes_closer_to_key_peer_info(
                routing_domain,
                opaque_record_key.to_hash_coordinate(),
                vec![VEILID_CAPABILITY_DHT]
            ));

        if debug_target_enabled!("dht") {
            let debug_string = format!(
                "IN <=== TransactBeginQ({}{}{}) <== {} (signer={})",
                opaque_record_key,
                if opt_descriptor.is_some() {
                    " +desc"
                } else {
                    ""
                },
                if want_descriptor { " +wantdesc" } else { "" },
                msg.header.direct_sender_node_id(),
                signer,
            );

            veilid_log!(self debug target: "dht", "{}", debug_string);
        }

        // See if this is within the consensus width
        let consensus_width = self.config().internal().network.dht.consensus_width as usize;

        let (
            accepted,
            need_descriptor,
            opt_descriptor,
            opt_transaction_id,
            opt_expiration,
            transact_begin_seqs,
        ) = if closer_to_key_peers.len() >= consensus_width {
            // Not close enough
            (
                false,
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
            )
        } else {
            // Close enough, lets get it

            // See if we have this record ourselves
            let storage_manager = self.storage_manager();
            let inbound_transact_begin_result = network_result_try!(storage_manager
                .inbound_transact_begin(
                    opaque_record_key.clone(),
                    opt_descriptor,
                    want_descriptor,
                    signing_member_id,
                )
                .measure_debug(TimestampDuration::new_ms(100), |dur| {
                    veilid_log!(self debug "inbound_transact_begin: {:#}", dur);
                })
                .await
                .map_err(RPCError::internal)?);

            match inbound_transact_begin_result {
                InboundTransactBeginResult::Success(transact_begin_result) => (
                    true,
                    false,
                    transact_begin_result
                        .opt_descriptor
                        .as_ref()
                        .map(|x| x.as_ref().clone()),
                    Some(transact_begin_result.transaction_id),
                    Some(transact_begin_result.expiration),
                    transact_begin_result.seqs,
                ),
                InboundTransactBeginResult::NeedDescriptor => (
                    true,
                    true,
                    Default::default(),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                ),
                InboundTransactBeginResult::TransactionUnavailable => (
                    true,
                    false,
                    Default::default(),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                ),
            }
        };

        let duration = if let Some(expiration) = opt_expiration {
            expiration.duration_since(Timestamp::now())
        } else {
            TimestampDuration::new(0)
        };

        if debug_target_enabled!("dht") {
            let debug_string_answer = format!(
                "IN ===> TransactBeginA({}{}{}{} peers={}) ==> {} seqs:{}",
                opaque_record_key,
                if accepted { " +accept" } else { "" },
                if need_descriptor { " +needdesc" } else { "" },
                if let Some(xid) = opt_transaction_id {
                    format!(" xid={}", xid)
                } else {
                    "".to_string()
                },
                closer_to_key_peers.len(),
                msg.header.direct_sender_node_id(),
                transact_begin_seqs.to_table_string(),
            );

            veilid_log!(self debug target: "dht", "{}", debug_string_answer);
        }

        // Make TransactBegin answer
        let transact_begin_a = RPCOperationTransactBeginA::new(
            accepted,
            need_descriptor,
            opt_descriptor.clone(),
            opt_transaction_id.map(|id| id.into()),
            duration,
            transact_begin_seqs,
            closer_to_key_peers,
        )?;

        // Send TransactBegin answer
        Box::pin(self.answer(
            msg,
            RPCAnswer::new(RPCAnswerDetail::TransactBeginA(Box::new(transact_begin_a))),
            None,
        ))
        .measure_debug(TimestampDuration::new_ms(200), |dur| {
            veilid_log!(self debug "process_transact_begin_q answer ({}xid={:?}{}{}): {:#}",
                if accepted {
                    "+accepted "
                } else {
                    ""
                },
                opt_transaction_id,
                if need_descriptor {
                    " +needdesc"
                } else {
                    ""
                },
                if opt_descriptor.is_some() {
                    " +senddesc"
                } else {
                    ""
                },
                dur
            );
        })
        .await
    }
}
