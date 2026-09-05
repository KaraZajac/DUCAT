use super::*;

impl_veilid_log_facility!("rpc");

impl RPCProcessor {
    // Sends a unidirectional signal to a node
    // Can be sent via relays but not routes. For routed 'signal' like capabilities, use AppMessage.
    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self), ret, err(level=Level::DEBUG)))]
    pub async fn rpc_call_signal(
        &self,
        dest: Destination,
        signal_info: SignalInfo,
    ) -> RPCNetworkResult<()> {
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
                "Never send signal requests over private routes",
            ));
        }

        let signal = RPCOperationSignal::new(signal_info);
        let statement = RPCStatement::new(RPCStatementDetail::Signal(Box::new(signal)));

        // Send the signal request
        self.statement(dest, statement, None, None).await
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rpc", skip(self, msg), fields(msg.operation.op_id), ret, err))]
    pub(super) async fn process_signal(&self, msg: Message) -> RPCNetworkResult<()> {
        // Ignore if disabled
        let routing_table = self.routing_table();

        let has_capability_signal = routing_table
            .get_published_peer_info(msg.header.routing_domain())
            .map(|ppi| ppi.node_info().has_capability(VEILID_CAPABILITY_SIGNAL))
            .unwrap_or(false);
        if !has_capability_signal {
            return Ok(NetworkResult::service_unavailable(
                "signal is not available",
            ));
        }

        // Can't allow anything other than direct packets here, as handling reverse connections
        // or anything like via signals over private routes would deanonymize the route
        let flow = match &msg.header.detail {
            RPCMessageHeaderDetail::Direct(d) => d.flow,
            RPCMessageHeaderDetail::SafetyRouted(_) | RPCMessageHeaderDetail::PrivateRouted(_) => {
                return Ok(NetworkResult::invalid_message("signal must be direct"));
            }
        };

        // Get the statement
        let (_, _, kind) = msg.operation.destructure();
        let signal = match kind {
            RPCOperationKind::Statement(s) => match s.destructure() {
                RPCStatementDetail::Signal(s) => s,
                _ => return Ok(NetworkResult::invalid_message("not a signal")),
            },
            _ => return Ok(NetworkResult::invalid_message("not a statement")),
        };

        // Handle it
        let network_manager = self.network_manager();
        let signal_info = signal.destructure();
        network_manager
            .handle_signal(flow, signal_info)
            .await
            .map_err(RPCError::network)
    }
}
