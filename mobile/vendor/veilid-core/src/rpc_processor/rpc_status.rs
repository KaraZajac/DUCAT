use super::*;

impl_veilid_log_facility!("rpc");

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Ord, Eq, Hash, Default)]
pub struct StatusSenderInfo {
    pub opt_sender_info: Option<SenderInfo>,
    pub opt_previous_sender_info: Option<SenderInfo>,
}

/// Outcome of `rpc_call_status`. Callers inspect `send_data_result` (when present)
/// for the transport that was actually attempted at the first hop.
#[derive(Debug)]
pub enum StatusResult {
    /// Question was sent and an answer was received (boxed: much larger than the other variants).
    Answer {
        answer: Box<Answer<StatusSenderInfo>>,
        send_data_result: SendDataResult,
    },
    /// Send was attempted but produced no usable answer (send failure, timeout, bad answer).
    Failed(SendDataResult),
    /// Never reached send_envelope (render failure, no routing domain, etc.).
    NotSent(NetworkResult<()>),
}

impl RPCProcessor {
    // Send StatusQ RPC request, receive StatusA answer
    // Can be sent via relays or routes, but will have less information via routes
    // sender:
    // unsafe -> node status
    // safe -> nothing
    // receiver:
    // direct -> node status + sender info
    // safety -> node status
    // private -> nothing
    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self), ret, err(level=Level::DEBUG)))]
    pub async fn rpc_call_status(&self, dest: Destination) -> Result<StatusResult, RPCError> {
        let _guard = self
            .startup_context
            .startup_lock
            .enter()
            .map_err(RPCError::map_try_again("not started up"))?;

        // Determine routing domain and node status to send
        let routing_table = self.routing_table();
        let (opt_target_nr, routing_domain, node_status) = if let Some(UnsafeRoutingInfo {
            opt_node,
            opt_routing_domain,
        }) =
            dest.get_unsafe_routing_info(&routing_table)
        {
            let Some(routing_domain) = opt_routing_domain else {
                if let Some(node) = &opt_node {
                    self.record_unreachable(node.clone());
                }
                return Ok(StatusResult::NotSent(NetworkResult::no_connection_other(
                    "no routing domain for target",
                )));
            };

            let node_status = Some(self.network_manager().generate_node_status(routing_domain));
            (opt_node, routing_domain, node_status)
        } else {
            // Safety route means we don't exchange node status and things are all PublicInternet RoutingDomain
            (None, RoutingDomain::PublicInternet, None)
        };

        // Get destination respond_to
        let respond_to = match self.get_destination_respond_to(&dest).await? {
            NetworkResult::Value(v) => v,
            other => return Ok(StatusResult::NotSent(other.map(|_| ()))),
        };

        // Create status rpc question
        let status_q = RPCOperationStatusQ::new(node_status);
        let question = RPCQuestion::new(respond_to, RPCQuestionDetail::StatusQ(Box::new(status_q)));

        let debug_string = format!("Status => {:#}", dest);

        // Send the info request
        let waitable_reply = match self.question(dest.clone(), question, None, None).await? {
            QuestionOutcome::Sent(wr) => *wr,
            QuestionOutcome::Failed(sdr) => return Ok(StatusResult::Failed(sdr)),
            QuestionOutcome::NotSent(nr) => return Ok(StatusResult::NotSent(nr)),
        };

        // Note what kind of ping this was and to what peer scope
        let send_direct_dial_info = waitable_reply.context.send_data_result.direct_dial_info();
        let Some(send_unique_flow) = waitable_reply.context.send_data_result.unique_flow() else {
            return Err(RPCError::internal(
                "status question sent without a unique flow",
            ));
        };

        // Snapshot send_data_result for the return value (NCM is Clone; success-path NR is
        // Value(UniqueFlow) which is Copy)
        let send_data_result = SendDataResult::new(
            waitable_reply
                .context
                .send_data_result
                .node_contact_method_result()
                .clone(),
            NetworkResult::Value(send_unique_flow),
        );

        // Wait for reply
        let (msg, answer_context) = match self.wait_for_reply(waitable_reply, debug_string).await? {
            TimeoutOr::Timeout => return Ok(StatusResult::Failed(send_data_result)),
            TimeoutOr::Value(v) => v,
        };

        // Get the right answer type
        let (_, _, kind) = msg.operation.destructure();
        let status_a = match kind {
            RPCOperationKind::Answer(a) => match a.destructure() {
                RPCAnswerDetail::StatusA(a) => a,
                _ => return Ok(StatusResult::Failed(send_data_result)),
            },
            _ => return Ok(StatusResult::Failed(send_data_result)),
        };
        let (a_node_status, sender_info) = status_a.destructure();

        // Ensure the returned node status is the kind for the routing domain we asked for
        if let Some(target_nr) = opt_target_nr {
            if let Some(a_node_status) = a_node_status {
                // Update latest node status in routing table
                target_nr.update_node_status(routing_domain, a_node_status.clone());
            }
        }

        // Report sender_info IP addresses to network manager
        // Don't need to validate these addresses for the current routing domain
        // the address itself is irrelevant, and the remote node can lie anyway
        let mut opt_sender_info = None;
        let mut opt_previous_sender_info = None;
        match dest {
            Destination::Direct {
                node: target,
                safety_selection,
            } => {
                if matches!(safety_selection, SafetySelection::Unsafe(_)) {
                    if let Some(sender_info) = sender_info {
                        if send_direct_dial_info.is_some() {
                            // Directly requested status that actually gets sent directly and not over a relay will tell us what our IP address appears as
                            // If this changes, we'd want to know about that to reset the networking stack
                            opt_previous_sender_info = target.report_sender_info(
                                routing_domain,
                                send_unique_flow,
                                sender_info,
                            );
                        };
                        opt_sender_info = Some(sender_info);

                        // Report ping status results to network manager
                        if let Err(e) = self.event_bus().post(SocketAddressChangeEvent {
                            routing_domain,
                            socket_address: sender_info.socket_address,
                            old_socket_address: opt_previous_sender_info.map(|s| s.socket_address),
                            flow: send_unique_flow.flow,
                            reporting_peer: target.unfiltered(),
                        }) {
                            veilid_log!(self debug "Failed to post event: {}", e);
                        }
                    }
                }
            }
            Destination::DialInfo {
                dial_info: _,
                node: _,
            }
            | Destination::PrivateRoute {
                private_route: _,
                safety_selection: _,
            } => {
                // sender info is irrelevant over relays and routes
            }
        };
        Ok(StatusResult::Answer {
            answer: Box::new(Answer::new(
                answer_context,
                StatusSenderInfo {
                    opt_sender_info,
                    opt_previous_sender_info,
                },
            )),
            send_data_result,
        })
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self, msg), fields(msg.operation.op_id), ret, err))]
    pub(super) async fn process_status_q(&self, msg: Message) -> RPCNetworkResult<()> {
        // Get the question
        let kind = msg.operation.kind().clone();
        let status_q = match kind {
            RPCOperationKind::Question(q) => match q.destructure() {
                (_, RPCQuestionDetail::StatusQ(q)) => q,
                _ => return Ok(NetworkResult::invalid_message("not a status question")),
            },
            _ => return Ok(NetworkResult::invalid_message("not a question")),
        };
        let q_node_status = status_q.destructure();

        let (node_status, sender_info) = match &msg.header.detail {
            RPCMessageHeaderDetail::Direct(detail) => {
                let flow = detail.flow;
                let routing_domain = detail.routing_domain;

                // Ensure the node status from the question is the kind for the routing domain we received the request in
                if let Some(q_node_status) = q_node_status {
                    // update node status for the requesting node to our routing table
                    if let Some(sender_nr) = msg.opt_sender_nr.clone() {
                        // Update latest node status in routing table for the statusq sender
                        sender_nr.update_node_status(routing_domain, q_node_status.clone());
                    }
                }

                // Get the peer address in the returned sender info
                let sender_info = SenderInfo {
                    socket_address: *flow.remote_address(),
                };

                // Make status answer
                let node_status = self.network_manager().generate_node_status(routing_domain);
                (Some(node_status), Some(sender_info))
            }
            RPCMessageHeaderDetail::SafetyRouted(_) => {
                // Make status answer
                let node_status = self
                    .network_manager()
                    .generate_node_status(RoutingDomain::PublicInternet);
                (Some(node_status), None)
            }
            RPCMessageHeaderDetail::PrivateRouted(_) => (None, None),
        };

        // Make status answer
        let status_a = RPCOperationStatusA::new(node_status, sender_info);

        // Send status answer
        self.answer(
            msg,
            RPCAnswer::new(RPCAnswerDetail::StatusA(Box::new(status_a))),
            None,
        )
        .await
    }
}
