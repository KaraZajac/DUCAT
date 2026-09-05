use super::*;

impl_veilid_log_facility!("rpc");

#[derive(Clone, Debug)]
pub struct TransactCommandAnswer {
    pub transaction_valid: bool,
    pub opt_expiration: Option<Timestamp>,
    pub opt_seqs: Option<Vec<ValueSeqNum>>,
    pub opt_subkey: Option<ValueSubkey>,
    pub opt_value: Option<Arc<SignedValueData>>,
}

impl RPCProcessor {
    /// Sends an transact command request and wait for response
    /// Can be sent via all methods including relays
    /// Safety routes may be used, but never private routes.
    /// Because this leaks information about the identity of the node itself,
    /// replying to this request received over a private route will leak
    /// the identity of the node and defeat the private route.
    /// The number of subkey sequence numbers returned may either be:
    ///  * an amount truncated to MAX_TRANSACT_COMMAND_A_SEQS_LEN subkeys
    ///  * zero if nothing was found
    #[
        instrument(level = "trace", target = "rpc", skip(self),
            fields(
                ret.latency
            ),err(level=Level::DEBUG))
    ]
    #[expect(clippy::too_many_arguments)]
    pub async fn rpc_call_transact_command(
        &self,
        dest: Destination,
        opaque_record_key: OpaqueRecordKey,
        descriptor: Arc<SignedValueDescriptor>,
        transaction_id: u64,
        command: TransactCommand,
        opt_seqs: Option<Vec<ValueSeqNum>>,
        opt_subkey: Option<ValueSubkey>,
        opt_value: Option<Arc<SignedValueData>>,
    ) -> RPCNetworkResult<Answer<TransactCommandAnswer>> {
        let _guard = self
            .startup_context
            .startup_lock
            .enter()
            .map_err(RPCError::map_try_again("not started up"))?;

        // Ensure destination never has a private route
        // and get the target noderef so we can validate the response
        let Some(_target_node_ids) = dest.get_target_node_ids() else {
            return Err(RPCError::internal(
                "Never send transact command requests over private routes",
            ));
        };

        let debug_string = format!(
            "OUT ==> TransactCommandQ({} xid={}{}{}) => {:#}{}",
            command,
            transaction_id,
            if let Some(subkey) = opt_subkey {
                format!(" #{}", subkey)
            } else {
                "".to_string()
            },
            if let Some(value) = &opt_value {
                format!(" {}", value)
            } else {
                "".to_string()
            },
            dest,
            if let Some(seqs) = &opt_seqs {
                format!(" seqs:{}", seqs.to_table_string())
            } else {
                "".to_string()
            },
        );

        // Send the TransactCommand question
        let transact_value_q = RPCOperationTransactCommandQ::new(
            opaque_record_key.clone(),
            transaction_id,
            command,
            opt_seqs,
            opt_subkey,
            opt_value.clone(),
        )?;

        let question = RPCQuestion::new(
            network_result_try!(self.get_destination_respond_to(&dest).await?),
            RPCQuestionDetail::TransactCommandQ(Box::new(transact_value_q)),
        );
        let question_context = QuestionContext::TransactCommand(ValidateTransactCommandContext {
            opaque_record_key,
            command,
            descriptor,
            opt_subkey,
            opt_value,
        });

        veilid_log!(self debug target: "dht", "{}", debug_string);

        let waitable_reply = network_result_try!(self
            .question(dest.clone(), question, None, Some(question_context))
            .await?
            .into_network_result());

        // Wait for reply
        let (msg, answer_context) = match self.wait_for_reply(waitable_reply, debug_string).await? {
            TimeoutOr::Timeout => return Ok(NetworkResult::Timeout),
            TimeoutOr::Value(v) => v,
        };

        // Get the right answer type
        let (_, _, kind) = msg.operation.destructure();
        let transact_command_a = match kind {
            RPCOperationKind::Answer(a) => match a.destructure() {
                RPCAnswerDetail::TransactCommandA(a) => a,
                _ => {
                    return Ok(NetworkResult::invalid_message(
                        "not a transactcommand answer",
                    ))
                }
            },
            _ => return Ok(NetworkResult::invalid_message("not an answer")),
        };

        let (transaction_valid, duration, opt_seqs, opt_subkey, opt_value) =
            transact_command_a.destructure();

        if debug_target_enabled!("dht") {
            let debug_string_answer = format!(
                "OUT <== TransactCommandA({} xid={} @{:#}{}{}) <= {} (latency={:#}) {}",
                command,
                transaction_id,
                duration,
                if let Some(subkey) = opt_subkey {
                    format!(" #{}", subkey)
                } else {
                    "".to_string()
                },
                if let Some(value) = &opt_value {
                    format!(" {}", value)
                } else {
                    "".to_string()
                },
                dest,
                answer_context.latency,
                if let Some(seqs) = &opt_seqs {
                    format!(" seqs:{}", seqs.to_table_string())
                } else {
                    "".to_string()
                },
            );

            veilid_log!(self debug target: "dht", "{}", debug_string_answer);
        }

        // Get expiration timestamp
        // Estimates the duration as calculated at a time halfway through the RPC by the remote node
        let opt_expiration = if duration.is_zero() {
            None
        } else {
            Some(
                answer_context
                    .waitable_reply_context
                    .send_ts
                    .later(answer_context.latency.div(2))
                    .later(duration),
            )
        };

        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.latency", answer_context.latency.as_u64());

        Ok(NetworkResult::value(Answer::new(
            answer_context,
            TransactCommandAnswer {
                transaction_valid,
                opt_expiration,
                opt_seqs,
                opt_subkey,
                opt_value,
            },
        )))
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self, msg), fields(msg.operation.op_id), ret, err))]
    pub(super) async fn process_transact_command_q(&self, msg: Message) -> RPCNetworkResult<()> {
        // Ensure this never came over a private route, safety route is okay though
        if msg.header.is_private_routed() {
            return Ok(NetworkResult::invalid_message(
                "not processing transact command request over private route",
            ));
        }
        let routing_table = self.routing_table();
        let routing_domain = msg.header.routing_domain();

        // Ignore if disabled
        let has_cap_dhtv = routing_table
            .get_published_peer_info(routing_domain)
            .map(|ppi| ppi.node_info().has_capability(VEILID_CAPABILITY_DHT))
            .unwrap_or(false);
        if !has_cap_dhtv {
            return Ok(NetworkResult::service_unavailable("dht is not available"));
        }

        // Get the question
        let kind = msg.operation.kind().clone();
        let transact_value_q = match kind {
            RPCOperationKind::Question(q) => match q.destructure() {
                (_, RPCQuestionDetail::TransactCommandQ(q)) => q,
                _ => {
                    return Ok(NetworkResult::invalid_message(
                        "not a transactcommand question",
                    ))
                }
            },
            _ => return Ok(NetworkResult::invalid_message("not a question")),
        };

        // Destructure
        let (opaque_record_key, transaction_id, command, opt_seqs, opt_subkey, opt_value) =
            transact_value_q.destructure();

        if debug_target_enabled!("dht") {
            let debug_string = format!(
                "IN <=== TransactCommandQ({} {} xid={}{}{}) <= {}{}",
                opaque_record_key,
                command,
                transaction_id,
                if let Some(subkey) = opt_subkey {
                    format!(" #{}", subkey)
                } else {
                    "".to_string()
                },
                if let Some(value) = &opt_value {
                    format!(" {}", value)
                } else {
                    "".to_string()
                },
                msg.header.direct_sender_node_id(),
                if let Some(seqs) = &opt_seqs {
                    format!(" seqs:{}", seqs.to_table_string())
                } else {
                    "".to_owned()
                }
            );

            veilid_log!(self debug target: "dht", "{}", debug_string);
        }

        let (transaction_valid, opt_expiration, opt_seqs, opt_subkey, opt_value) = {
            // See if we have this record ourselves
            let storage_manager = self.storage_manager();
            let inbound_transact_value_result = network_result_try!(storage_manager
                .inbound_transact_command(
                    &opaque_record_key,
                    transaction_id,
                    command,
                    opt_seqs,
                    opt_subkey,
                    opt_value,
                )
                .measure_debug(TimestampDuration::new_ms(200), |dur| {
                    veilid_log!(self debug "inbound_transact_command: {:#} command={} xid={} opt_subkey={:?}", dur, command, transaction_id, opt_subkey);
                })
                .await
                .map_err(RPCError::internal)?);

            match inbound_transact_value_result {
                InboundTransactCommandResult::Success(res) => (
                    true,
                    Some(res.expiration),
                    res.opt_seqs,
                    res.opt_subkey,
                    res.opt_value,
                ),
                InboundTransactCommandResult::InvalidTransaction => (
                    false,
                    Default::default(),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                ),
                InboundTransactCommandResult::InvalidArguments => {
                    return Ok(NetworkResult::invalid_message(
                        "not processing transact command request with invalid arguments",
                    ))
                }
            }
        };

        let duration = if let Some(expiration) = opt_expiration {
            expiration.duration_since(Timestamp::now())
        } else {
            TimestampDuration::new(0)
        };

        if debug_target_enabled!("dht") {
            let debug_string_answer = format!(
                "IN ===> TransactCommandA({} xid={} @{}{}{}{}) => {}{}",
                command,
                transaction_id,
                duration,
                if transaction_valid { " +xvalid" } else { "" },
                if let Some(subkey) = opt_subkey {
                    format!(" #{}", subkey)
                } else {
                    "".to_string()
                },
                if let Some(value) = &opt_value {
                    format!(" {}", value)
                } else {
                    "".to_string()
                },
                msg.header.direct_sender_node_id(),
                if let Some(seqs) = &opt_seqs {
                    format!(" seqs:{}", seqs.to_table_string())
                } else {
                    "".to_owned()
                }
            );

            veilid_log!(self debug target: "dht", "{}", debug_string_answer);
        }

        // Make TransactCommand answer
        let transact_command_a = RPCOperationTransactCommandA::new(
            transaction_valid,
            duration,
            opt_seqs,
            opt_subkey,
            opt_value,
        )?;

        // Send TransactCommand answer
        Box::pin(
            self.answer(
                msg,
                RPCAnswer::new(RPCAnswerDetail::TransactCommandA(Box::new(
                    transact_command_a,
                ))),
                None,
            )
            .measure_debug(TimestampDuration::new_ms(200), |dur| {
                veilid_log!(self debug "process_transact_command_q answer ({} xid={}): {:#}", command, transaction_id, dur);
            }),
        )
        .await
    }
}
