use super::*;
use crate::storage_manager::SignedValueDescriptor;

impl_veilid_log_facility!("rpc");

#[derive(Clone, Debug)]
pub struct InspectValueAnswer {
    pub accepted: bool,
    pub seqs: Vec<ValueSeqNum>,
    pub peers: Vec<Arc<PeerInfo>>,
    pub descriptor: Option<SignedValueDescriptor>,
}

impl RPCProcessor {
    /// Sends an inspect value request and wait for response
    /// Can be sent via all methods including relays
    /// Safety routes may be used, but never private routes.
    /// Because this leaks information about the identity of the node itself,
    /// replying to this request received over a private route will leak
    /// the identity of the node and defeat the private route.
    /// The number of subkey sequence numbers returned may either be:
    ///  * the amount requested
    ///  * an amount truncated to DHTSchema::MAX_SUBKEY_COUNT subkeys
    ///  * zero if nothing was found
    #[
        instrument(level = "trace", target = "rpc", skip(self),
            fields(
                %descriptor_mode,
                ret.peers.len,
                ret.latency,
                ret.accepted
            ),err(level=Level::DEBUG))
    ]
    pub async fn rpc_call_inspect_value(
        &self,
        dest: Destination,
        opaque_record_key: OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        descriptor_mode: GetDescriptorMode,
    ) -> RPCNetworkResult<Answer<InspectValueAnswer>> {
        let _guard = self
            .startup_context
            .startup_lock
            .enter()
            .map_err(RPCError::map_try_again("not started up"))?;

        // Ensure destination never has a private route
        // and get the target noderef so we can validate the response
        let Some(target_node_ids) = dest.get_target_node_ids() else {
            return Err(RPCError::internal(
                "Never send inspect value requests over private routes",
            ));
        };

        // Get the target node id
        Crypto::validate_crypto_kind(opaque_record_key.kind()).map_err(RPCError::internal)?;
        let Some(target_node_id) = target_node_ids.get(opaque_record_key.kind()) else {
            return Err(RPCError::internal("No node id for crypto kind"));
        };

        let debug_string = format!(
            "OUT ==> InspectValueQ({} #{}{}) => {:#}",
            opaque_record_key, &subkeys, descriptor_mode, dest
        );

        // Send the inspectvalue question
        let inspect_value_q = RPCOperationInspectValueQ::new(
            opaque_record_key.clone(),
            subkeys.clone(),
            matches!(descriptor_mode, GetDescriptorMode::WantDescriptor),
        )?;
        let question = RPCQuestion::new(
            network_result_try!(self.get_destination_respond_to(&dest).await?),
            RPCQuestionDetail::InspectValueQ(Box::new(inspect_value_q)),
        );

        let question_context = QuestionContext::InspectValue(ValidateInspectValueContext {
            opaque_record_key: opaque_record_key.clone(),
            descriptor_mode,
            subkeys,
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
        let inspect_value_a = match kind {
            RPCOperationKind::Answer(a) => match a.destructure() {
                RPCAnswerDetail::InspectValueA(a) => a,
                _ => return Ok(NetworkResult::invalid_message("not an inspectvalue answer")),
            },
            _ => return Ok(NetworkResult::invalid_message("not an answer")),
        };

        let (accepted, seqs, peers, descriptor) = inspect_value_a.destructure();

        if debug_target_enabled!("dht") {
            let debug_string_answer = format!(
                "OUT <== InspectValueA({} {}{} peers={}) <= {} seqs:{}",
                opaque_record_key,
                if accepted { " +accept" } else { "" },
                if descriptor.is_some() { " +desc" } else { "" },
                peers.len(),
                dest,
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

        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.latency", answer_context.latency.as_u64());
        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.accepted", accepted);
        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.peers.len", peers.len());

        Ok(NetworkResult::value(Answer::new(
            answer_context,
            InspectValueAnswer {
                accepted,
                seqs,
                peers,
                descriptor,
            },
        )))
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self, msg), fields(msg.operation.op_id), ret, err))]
    pub(super) async fn process_inspect_value_q(&self, msg: Message) -> RPCNetworkResult<()> {
        // Ensure this never came over a private route, safety route is okay though
        if msg.header.is_private_routed() {
            return Ok(NetworkResult::invalid_message(
                "not processing inspect value request over private route",
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
        let inspect_value_q = match kind {
            RPCOperationKind::Question(q) => match q.destructure() {
                (_, RPCQuestionDetail::InspectValueQ(q)) => q,
                _ => return Err(RPCError::internal("not an inspectvalue question")),
            },
            _ => return Err(RPCError::internal("not a question")),
        };

        // Destructure
        let (opaque_record_key, subkeys, want_descriptor) = inspect_value_q.destructure();

        // Get the nodes that we know about that are closer to the the key than our own node
        let closer_to_key_peers = network_result_try!(routing_table
            .get_reliable_nodes_closer_to_key_peer_info(
                routing_domain,
                opaque_record_key.to_hash_coordinate(),
                vec![VEILID_CAPABILITY_DHT]
            ));

        if debug_target_enabled!("dht") {
            let debug_string = format!(
                "IN <=== InspectValueQ({} {}{}) <== {}",
                opaque_record_key,
                subkeys,
                if want_descriptor { " +wantdesc" } else { "" },
                msg.header.direct_sender_node_id()
            );

            veilid_log!(self debug target: "dht", "{}", debug_string);
        }

        // See if this is within the consensus width
        let consensus_width = self.config().internal().network.dht.consensus_width as usize;

        let (accepted, inspect_result_seqs, inspect_result_descriptor) =
            if closer_to_key_peers.len() >= consensus_width {
                // Not close enough
                (false, vec![], None)
            } else {
                // Close enough, lets get it

                // See if we have this record ourselves
                let storage_manager = self.storage_manager();
                let inbound_inspect_value_result = network_result_try!(storage_manager
                    .inbound_inspect_value(&opaque_record_key, subkeys, want_descriptor)
                    .await
                    .map_err(RPCError::internal)?);

                match inbound_inspect_value_result {
                    InboundInspectValueResult::Success(inspect_result) => (
                        true,
                        inspect_result.seqs().to_vec(),
                        inspect_result.opt_descriptor(),
                    ),
                }
            };

        if debug_target_enabled!("dht") {
            let debug_string_answer = format!(
                "IN ===> InspectValueA({} {}{} peers={}) ==> {} seqs:{}",
                opaque_record_key,
                if accepted { " +accept" } else { "" },
                if inspect_result_descriptor.is_some() {
                    " +desc"
                } else {
                    ""
                },
                closer_to_key_peers.len(),
                msg.header.direct_sender_node_id(),
                inspect_result_seqs.to_table_string(),
            );

            veilid_log!(self debug target: "dht", "{}", debug_string_answer);
        }

        // Make InspectValue answer
        let inspect_value_a = RPCOperationInspectValueA::new(
            accepted,
            inspect_result_seqs,
            closer_to_key_peers,
            inspect_result_descriptor.map(|x| (*x).clone()),
        )?;

        // Send InspectValue answer
        self.answer(
            msg,
            RPCAnswer::new(RPCAnswerDetail::InspectValueA(Box::new(inspect_value_a))),
            None,
        )
        .await
    }
}
