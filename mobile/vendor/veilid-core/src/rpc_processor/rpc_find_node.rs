use super::*;

impl_veilid_log_facility!("rpc");

impl RPCProcessor {
    /// Send FindNodeQ RPC request, receive FindNodeA answer
    /// Can be sent via all methods including relays
    /// Safety routes may be used, but never private routes.
    /// Because this leaks information about the identity of the node itself,
    /// replying to this request received over a private route will leak
    /// the identity of the node and defeat the private route.
    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self), err(level=Level::DEBUG)))]
    pub async fn rpc_call_find_node(
        &self,
        dest: Destination,
        hash_coordinate: HashCoordinate,
        capabilities: Vec<VeilidCapability>,
    ) -> RPCNetworkResult<Answer<Vec<Arc<PeerInfo>>>> {
        let _guard = self
            .startup_context
            .startup_lock
            .enter()
            .map_err(RPCError::map_try_again("not started up"))?;

        // Ensure destination never has a private route
        if matches!(
            dest,
            Destination::PrivateRoute {
                private_route: _,
                safety_selection: _
            }
        ) {
            return Err(RPCError::internal(
                "Never send find node requests over private routes",
            ));
        }

        let find_node_q_detail = RPCQuestionDetail::FindNodeQ(Box::new(
            RPCOperationFindNodeQ::new(hash_coordinate.clone(), capabilities.clone())?,
        ));
        let find_node_q = RPCQuestion::new(
            network_result_try!(self.get_destination_respond_to(&dest).await?),
            find_node_q_detail,
        );

        let debug_string = format!("FindNode(node_id={}) => {:#}", hash_coordinate, dest);

        // Send the find_node request
        let waitable_reply = network_result_try!(self
            .question(dest, find_node_q, None, None)
            .await?
            .into_network_result());

        // Wait for reply
        let (msg, answer_context) = match self.wait_for_reply(waitable_reply, debug_string).await? {
            TimeoutOr::Timeout => return Ok(NetworkResult::Timeout),
            TimeoutOr::Value(v) => v,
        };

        // Get the right answer type
        let (_, _, kind) = msg.operation.destructure();
        let find_node_a = match kind {
            RPCOperationKind::Answer(a) => match a.destructure() {
                RPCAnswerDetail::FindNodeA(a) => a,
                _ => return Ok(NetworkResult::invalid_message("not a find_node answer")),
            },
            _ => return Ok(NetworkResult::invalid_message("not an answer")),
        };

        // Verify peers are in the correct peer scope
        let peers = find_node_a.destructure();

        for peer_info in &peers {
            // Check the peer info capability set
            if !peer_info.node_info().has_all_capabilities(&capabilities) {
                return Ok(NetworkResult::invalid_message(
                    "find_node response does not meet peer criteria",
                ));
            }
        }

        Ok(NetworkResult::value(Answer::new(answer_context, peers)))
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////

    #[cfg_attr(feature="verbose-tracing", instrument(level = "trace", skip(self, msg), fields(msg.operation.op_id), ret, err))]
    pub(super) async fn process_find_node_q(&self, msg: Message) -> RPCNetworkResult<()> {
        // Ensure this never came over a private route, safety route is okay though
        if msg.header.is_private_routed() {
            return Ok(NetworkResult::invalid_message(
                "not processing find node request over private route",
            ));
        }

        // Get the question
        let kind = msg.operation.kind().clone();
        let find_node_q = match kind {
            RPCOperationKind::Question(q) => match q.destructure() {
                (_, RPCQuestionDetail::FindNodeQ(q)) => q,
                _ => return Err(RPCError::internal("not a findnode question")),
            },
            _ => return Err(RPCError::internal("not a question")),
        };
        let (hash_coordinate, capabilities) = find_node_q.destructure();

        // Get a chunk of the routing table near the requested node id
        let routing_table = self.routing_table();
        let routing_domain = msg.header.routing_domain();

        let closest_nodes = routing_table.get_preferred_closest_nodes_peer_info(
            routing_domain,
            hash_coordinate,
            &capabilities,
        );

        // Make FindNode answer
        let find_node_a = RPCOperationFindNodeA::new(closest_nodes)?;

        // Send FindNode answer
        self.answer(
            msg,
            RPCAnswer::new(RPCAnswerDetail::FindNodeA(Box::new(find_node_a))),
            None,
        )
        .await
    }
}
