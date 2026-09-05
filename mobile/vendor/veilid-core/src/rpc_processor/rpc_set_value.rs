use super::*;

impl_veilid_log_facility!("rpc");

#[derive(Clone, Debug)]
pub struct SetValueAnswer {
    pub accepted: bool,
    pub need_descriptor: bool,
    pub value: Option<SignedValueData>,
    pub peers: Vec<Arc<PeerInfo>>,
}

impl RPCProcessor {
    /// Sends a set value request and wait for response
    /// Can be sent via all methods including relays
    /// Safety routes may be used, but never private routes.
    /// Because this leaks information about the identity of the node itself,
    /// replying to this request received over a private route will leak
    /// the identity of the node and defeat the private route.
    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self, value),
        fields(
            %descriptor_mode,
            value.data.len = value.value_data().data().len(),
            value.data.seq = value.value_data().seq().to_option(),
            value.data.writer = value.value_data().writer().to_string(),
            ret.accepted,
            ret.value.data.len,
            ret.value.data.seq,
            ret.value.data.writer,
            ret.peers.len,
            ret.latency
        ), err(level=Level::DEBUG)))]
    pub async fn rpc_call_set_value(
        &self,
        dest: Destination,
        opaque_record_key: OpaqueRecordKey,
        subkey: ValueSubkey,
        value: SignedValueData,
        descriptor_mode: SetDescriptorMode,
    ) -> RPCNetworkResult<Answer<SetValueAnswer>> {
        let _guard = self
            .startup_context
            .startup_lock
            .enter()
            .map_err(RPCError::map_try_again("not started up"))?;

        // Ensure destination never has a private route
        // and get the target noderef so we can validate the response
        let Some(target_node_ids) = dest.get_target_node_ids() else {
            return Err(RPCError::internal(
                "Never send set value requests over private routes",
            ));
        };

        // Get the target node id
        Crypto::validate_crypto_kind(opaque_record_key.kind()).map_err(RPCError::internal)?;
        let Some(target_node_id) = target_node_ids.get(opaque_record_key.kind()) else {
            return Err(RPCError::internal("No node id for crypto kind"));
        };

        let debug_string = format!(
            "OUT ==> SetValueQ({} #{}{} {}) => {:#}",
            opaque_record_key, subkey, descriptor_mode, value, dest
        );

        let (descriptor, send_descriptor) = match descriptor_mode {
            SetDescriptorMode::HaveDescriptor(signed_value_descriptor) => {
                (signed_value_descriptor.as_ref().clone(), false)
            }
            SetDescriptorMode::SendDescriptor(signed_value_descriptor) => {
                (signed_value_descriptor.as_ref().clone(), true)
            }
        };

        // Send the setvalue question
        let set_value_q = RPCOperationSetValueQ::new(
            opaque_record_key.clone(),
            subkey,
            value,
            if send_descriptor {
                Some(descriptor.clone())
            } else {
                None
            },
        );
        let question = RPCQuestion::new(
            network_result_try!(self.get_destination_respond_to(&dest).await?),
            RPCQuestionDetail::SetValueQ(Box::new(set_value_q)),
        );
        let question_context = QuestionContext::SetValue(ValidateSetValueContext {
            opaque_record_key: opaque_record_key.clone(),
            descriptor,
            subkey,
        });

        if debug_target_enabled!("dht") {
            veilid_log!(self debug target: "dht", "{}", debug_string);
        }

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
        let set_value_a = match kind {
            RPCOperationKind::Answer(a) => match a.destructure() {
                RPCAnswerDetail::SetValueA(a) => a,
                _ => return Ok(NetworkResult::invalid_message("not a setvalue answer")),
            },
            _ => return Ok(NetworkResult::invalid_message("not an answer")),
        };

        let (accepted, need_descriptor, value, peers) = set_value_a.destructure();

        if debug_target_enabled!("dht") {
            let debug_string_value = value.as_ref().map(|v| v.to_string()).unwrap_or_default();

            let debug_string_answer = format!(
                "OUT <== SetValueA({} #{}{}{}{} peers={}) <= {}",
                opaque_record_key,
                subkey,
                if accepted { " +accept" } else { "" },
                if need_descriptor { " +needdesc" } else { "" },
                debug_string_value,
                peers.len(),
                dest,
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

        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.latency", answer_context.latency.as_u64());
        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.accepted", accepted);
        #[cfg(feature = "verbose-tracing")]
        if let Some(value) = &value {
            tracing::Span::current().record("ret.value.data.len", value.value_data().data().len());
            tracing::Span::current().record(
                "ret.value.data.seq",
                tracing::field::display(value.value_data().seq()),
            );
            tracing::Span::current().record(
                "ret.value.data.writer",
                tracing::field::display(value.value_data().writer()),
            );
        }
        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.peers.len", peers.len());

        Ok(NetworkResult::value(Answer::new(
            answer_context,
            SetValueAnswer {
                accepted,
                need_descriptor,
                value,
                peers,
            },
        )))
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self, msg), fields(msg.operation.op_id), ret, err))]
    pub(super) async fn process_set_value_q(&self, msg: Message) -> RPCNetworkResult<()> {
        // Ensure this never came over a private route, safety route is okay though
        if msg.header.is_private_routed() {
            return Ok(NetworkResult::invalid_message(
                "not processing get value request over private route",
            ));
        }
        let routing_table = self.routing_table();
        let routing_domain = msg.header.routing_domain();

        // Ignore if disabled
        let has_capability_dht = routing_table
            .get_published_peer_info(msg.header.routing_domain())
            .map(|ppi| ppi.node_info().has_capability(VEILID_CAPABILITY_DHT))
            .unwrap_or(false);
        if !has_capability_dht {
            return Ok(NetworkResult::service_unavailable("dht is not available"));
        }

        // Get the question
        let kind = msg.operation.kind().clone();
        let set_value_q = match kind {
            RPCOperationKind::Question(q) => match q.destructure() {
                (_, RPCQuestionDetail::SetValueQ(q)) => q,
                _ => return Err(RPCError::internal("not a setvalue question")),
            },
            _ => return Err(RPCError::internal("not a question")),
        };

        // Destructure
        let (opaque_record_key, subkey, value, descriptor) = set_value_q.destructure();

        // Get target for ValueChanged notifications
        let dest = network_result_try!(self.get_respond_to_destination(&msg));
        let target = dest.get_target(&routing_table)?;

        // Get the nodes that we know about that are closer to the the key than our own node
        let closer_to_key_peers = network_result_try!(routing_table
            .get_reliable_nodes_closer_to_key_peer_info(
                routing_domain,
                opaque_record_key.to_hash_coordinate(),
                vec![VEILID_CAPABILITY_DHT]
            ));

        let debug_string = format!(
            "IN <=== SetValueQ({} #{} {}{}) <== {}",
            opaque_record_key,
            subkey,
            value,
            if descriptor.is_some() { " +desc" } else { "" },
            msg.header.direct_sender_node_id()
        );

        veilid_log!(self debug target: "dht", "{}", debug_string);

        // If there are less than 'consensus_width' peers that are closer, then store here too
        let consensus_width = self.config().internal().network.dht.consensus_width as usize;

        let (accepted, need_descriptor, return_value) =
            if closer_to_key_peers.len() >= consensus_width {
                // Not close enough
                (false, false, None)
            } else {
                // Close enough, lets set it

                // Save the subkey, creating a new record if necessary
                let storage_manager = self.storage_manager();
                let result = network_result_try!(storage_manager
                    .inbound_set_value(
                        &opaque_record_key,
                        subkey,
                        Arc::new(value),
                        descriptor.map(Arc::new),
                        target
                    )
                    .await
                    .map_err(RPCError::internal)?);

                let (need_descriptor, return_value) = match result {
                    InboundSetValueResult::Success => (false, None),
                    InboundSetValueResult::Ignored(old_value) => (false, Some(old_value)),
                    InboundSetValueResult::NeedsDescriptor => (true, None),
                };

                (true, need_descriptor, return_value)
            };

        if debug_target_enabled!("dht") {
            let debug_string_value = return_value
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_default();

            let debug_string_answer = format!(
                "IN ===> SetValueA({} #{}{}{}{} peers={}) ==> {}",
                opaque_record_key,
                subkey,
                if accepted { " +accept" } else { "" },
                if need_descriptor { " +needdesc" } else { "" },
                debug_string_value,
                closer_to_key_peers.len(),
                msg.header.direct_sender_node_id()
            );

            veilid_log!(self debug target: "dht", "{}", debug_string_answer);
        }

        // Make SetValue answer
        let set_value_a = RPCOperationSetValueA::new(
            accepted,
            need_descriptor,
            return_value.map(|x| (*x).clone()),
            closer_to_key_peers,
        )?;

        // Send SetValue answer
        self.answer(
            msg,
            RPCAnswer::new(RPCAnswerDetail::SetValueA(Box::new(set_value_a))),
            None,
        )
        .await
    }
}
