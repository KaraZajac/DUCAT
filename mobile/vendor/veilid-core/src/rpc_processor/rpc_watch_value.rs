use super::*;

impl_veilid_log_facility!("rpc");

#[derive(Clone, Debug)]
pub struct WatchValueAnswer {
    pub accepted: bool,
    pub expiration: Timestamp,
    pub peers: Vec<Arc<PeerInfo>>,
    pub watch_id: u64,
}

impl RPCProcessor {
    /// Sends a watch value request and wait for response
    /// Can be sent via all methods including relays
    /// Safety routes may be used, but never private routes.
    /// Because this leaks information about the identity of the node itself,
    /// replying to this request received over a private route will leak
    /// the identity of the node and defeat the private route.
    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self),
            fields(ret.expiration,
                ret.latency,
                ret.accepted,
                ret.peers.len
            ),err(level=Level::DEBUG)))]
    #[allow(clippy::too_many_arguments)]
    pub async fn rpc_call_watch_value(
        &self,
        dest: Destination,
        opaque_record_key: OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        expiration: Timestamp,
        count: u32,
        watcher: KeyPair,
        watch_id: Option<u64>,
    ) -> RPCNetworkResult<Answer<WatchValueAnswer>> {
        let _guard = self
            .startup_context
            .startup_lock
            .enter()
            .map_err(RPCError::map_try_again("not started up"))?;

        // Ensure destination never has a private route
        // and get the target noderef so we can validate the response
        let Some(target_node_ids) = dest.get_target_node_ids() else {
            return Err(RPCError::internal(
                "Never send watch value requests over private routes",
            ));
        };

        // Get the target node id
        let Some(target_node_id) = target_node_ids.get(opaque_record_key.kind()) else {
            return Err(RPCError::internal("No node id for crypto kind"));
        };

        // Get duration
        let duration = if expiration.is_zero() {
            TimestampDuration::new(0)
        } else {
            expiration.duration_since(Timestamp::now())
        };

        let debug_string = format!(
            "OUT ==> WatchValueQ({} {} {}@{}+{}) => {:#} (watcher={}) ",
            if let Some(watch_id) = watch_id {
                format!("id={} ", watch_id)
            } else {
                "".to_owned()
            },
            opaque_record_key,
            subkeys,
            duration,
            count,
            dest,
            watcher
        );

        // Send the watchvalue question
        let watch_value_q = RPCOperationWatchValueQ::new(
            opaque_record_key.clone(),
            subkeys.clone(),
            duration,
            count,
            watch_id,
        )?;
        let question = RPCQuestion::new(
            network_result_try!(self.get_destination_respond_to(&dest).await?),
            RPCQuestionDetail::WatchValueQ(Box::new(watch_value_q)),
        );

        veilid_log!(self debug target: "dht", "{}", debug_string);

        let waitable_reply = network_result_try!(self
            .question(dest.clone(), question, Some(watcher), None)
            .await?
            .into_network_result());

        // Wait for reply
        let (msg, answer_context) = match self.wait_for_reply(waitable_reply, debug_string).await? {
            TimeoutOr::Timeout => return Ok(NetworkResult::Timeout),
            TimeoutOr::Value(v) => v,
        };

        // Get the right answer type
        let (_, _, kind) = msg.operation.destructure();
        let watch_value_a = match kind {
            RPCOperationKind::Answer(a) => match a.destructure() {
                RPCAnswerDetail::WatchValueA(a) => a,
                _ => return Ok(NetworkResult::invalid_message("not a watchvalue answer")),
            },
            _ => return Ok(NetworkResult::invalid_message("not an answer")),
        };
        let question_watch_id = watch_id;
        let (accepted, duration, peers, watch_id) = watch_value_a.destructure();
        if debug_target_enabled!("dht") {
            let debug_string_answer = format!(
                "OUT <== WatchValueA({}id={} {} #{:?}@{} peers={}) <= {}",
                if accepted { "+accept " } else { "" },
                watch_id,
                opaque_record_key,
                subkeys,
                duration,
                peers.len(),
                dest
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

        // Validate accepted requests
        if accepted {
            // Verify returned answer watch id is the same as the question watch id if it exists
            if let Some(question_watch_id) = question_watch_id {
                if question_watch_id != watch_id {
                    return Ok(NetworkResult::invalid_message(format!(
                        "answer watch id={} doesn't match question watch id={}",
                        watch_id, question_watch_id,
                    )));
                }
            }
            // Validate if a watch is created/updated, that it has a nonzero id
            if !duration.is_zero() && watch_id == 0 {
                return Ok(NetworkResult::invalid_message(
                    "zero watch id returned on accepted or cancelled watch",
                ));
            }
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
        tracing::Span::current().record("ret.expiration", tracing::field::display(expiration));
        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.peers.len", peers.len());

        Ok(NetworkResult::value(Answer::new(
            answer_context,
            WatchValueAnswer {
                accepted,
                expiration,
                peers,
                watch_id,
            },
        )))
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self, msg), fields(msg.operation.op_id), ret, err))]
    pub(super) async fn process_watch_value_q(&self, msg: Message) -> RPCNetworkResult<()> {
        // Ensure this never came over a private route, safety route is okay though
        if msg.header.is_private_routed() {
            return Ok(NetworkResult::invalid_message(
                "not processing watch value request over private route",
            ));
        }

        // Ignore if disabled
        let routing_table = self.routing_table();
        let routing_domain = msg.header.routing_domain();

        let has_cap_dhtv = routing_table
            .get_published_peer_info(msg.header.routing_domain())
            .map(|ppi| ppi.node_info().has_capability(VEILID_CAPABILITY_DHT))
            .unwrap_or(false);
        if !has_cap_dhtv {
            return Ok(NetworkResult::service_unavailable(
                "DHTV capability is not available",
            ));
        }

        // Get the question
        let kind = msg.operation.kind().clone();
        let watch_value_q = match kind {
            RPCOperationKind::Question(q) => match q.destructure() {
                (_, RPCQuestionDetail::WatchValueQ(q)) => q,
                _ => return Ok(NetworkResult::invalid_message("not a watchvalue question")),
            },
            _ => return Ok(NetworkResult::invalid_message("not a question")),
        };

        // Destructure
        let (opaque_record_key, subkeys, duration, count, watch_id) = watch_value_q.destructure();
        let Some(watcher) = &msg.opt_signer else {
            return Ok(NetworkResult::invalid_message("WatchValueQ must be signed"));
        };

        // Extract member id for watcher
        let Ok(watcher_member_id) = self.storage_manager().generate_member_id(watcher) else {
            return Ok(NetworkResult::invalid_message(
                "could not generate member id for watcher public key",
            ));
        };

        // Get target for ValueChanged notifications
        let dest = network_result_try!(self.get_respond_to_destination(&msg));
        let target = dest.get_target(&routing_table)?;

        if debug_target_enabled!("dht") {
            let debug_string = format!(
                "IN <=== WatchValueQ({}{} {}@{}+{}) <== {} (watcher={})",
                if let Some(watch_id) = watch_id {
                    format!("id={} ", watch_id)
                } else {
                    "".to_owned()
                },
                opaque_record_key,
                subkeys,
                duration,
                count,
                msg.header.direct_sender_node_id(),
                watcher_member_id
            );

            veilid_log!(self debug target: "dht", "{}", debug_string);
        }

        // Get the nodes that we know about that are closer to the the key than our own node
        let closer_to_key_peers = network_result_try!(routing_table
            .get_reliable_nodes_closer_to_key_peer_info(
                routing_domain,
                opaque_record_key.to_hash_coordinate(),
                vec![VEILID_CAPABILITY_DHT]
            ));

        // Calculate expiration
        let expiration = if duration.is_zero() {
            Timestamp::new(0)
        } else {
            Timestamp::now().later(duration)
        };

        // See if this is within the consensus width
        let consensus_width = self.config().internal().network.dht.consensus_width as usize;

        let (ret_accepted, ret_expiration, ret_watch_id) =
            if closer_to_key_peers.len() >= consensus_width {
                // Not close enough, not accepted
                veilid_log!(self debug "Not close enough for watch value");

                (false, Timestamp::new(0), watch_id.unwrap_or_default())
            } else {
                // Accepted, lets try to watch or cancel it
                let params = InboundWatchParameters {
                    subkeys: subkeys.clone(),
                    expiration,
                    count,
                    watcher_member_id,
                    target,
                };

                // See if we have this record ourselves, if so, accept the watch
                let storage_manager = self.storage_manager();
                let watch_result = network_result_try!(storage_manager
                    .inbound_watch_value(opaque_record_key.clone(), params, watch_id)
                    .await
                    .map_err(RPCError::internal)?);

                // Encode the watch result
                // Rejections and cancellations are treated the same way by clients
                let (ret_expiration, ret_watch_id) = match watch_result {
                    InboundWatchValueResult::Created { id, expiration } => (expiration, id.into()),
                    InboundWatchValueResult::Changed { expiration } => {
                        (expiration, watch_id.unwrap_or_default())
                    }
                    InboundWatchValueResult::Cancelled => {
                        (Timestamp::new(0), watch_id.unwrap_or_default())
                    }
                    InboundWatchValueResult::Rejected => {
                        (Timestamp::new(0), watch_id.unwrap_or_default())
                    }
                };
                (true, ret_expiration, ret_watch_id)
            };

        // Calculate duration
        let ret_duration = if ret_expiration.is_zero() {
            TimestampDuration::new(0)
        } else {
            ret_expiration.duration_since(Timestamp::now())
        };

        if debug_target_enabled!("dht") {
            let debug_string_answer = format!(
                "IN ===> WatchValueA({}id={} {} #{} @{} peers={}) ==> {}",
                if ret_accepted { "+accept " } else { "" },
                ret_watch_id,
                opaque_record_key,
                subkeys,
                ret_duration,
                closer_to_key_peers.len(),
                msg.header.direct_sender_node_id()
            );

            veilid_log!(self debug target: "dht", "{}", debug_string_answer);
        }

        // Make WatchValue answer
        let watch_value_a = RPCOperationWatchValueA::new(
            ret_accepted,
            ret_duration,
            closer_to_key_peers,
            ret_watch_id,
        )?;

        // Send GetValue answer
        self.answer(
            msg,
            RPCAnswer::new(RPCAnswerDetail::WatchValueA(Box::new(watch_value_a))),
            None,
        )
        .await
    }
}
