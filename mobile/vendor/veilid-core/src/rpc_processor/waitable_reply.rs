use super::*;

#[derive(Debug)]
#[must_use]
pub struct WaitableReplyContext {
    pub timeout: TimestampDuration,
    pub send_ts: Timestamp,
    pub send_data_result: SendDataResult,
    pub node_ref: FilteredNodeRef,
    pub opt_safety_route_ref: Option<AllocatedRouteRef>,
    pub opt_remote_private_route_ref: Option<RemoteRouteRef>,
    pub opt_reply_private_route_ref: Option<AllocatedRouteRef>,
    pub destination: Destination,
}

impl WaitableReplyContext {
    pub fn is_routed(&self) -> bool {
        self.opt_safety_route_ref.is_some() || self.opt_remote_private_route_ref.is_some()
    }

    /// Take the allocated route refs (safety + reply) held by this context.
    pub fn into_allocated_route_refs(self) -> Vec<AllocatedRouteRef> {
        self.opt_safety_route_ref
            .into_iter()
            .chain(self.opt_reply_private_route_ref)
            .collect()
    }

    #[cfg_attr(not(feature = "verbose-tracing"), expect(dead_code))]
    pub fn debug(&self, routing_table: &RoutingTable) -> String {
        let rss = routing_table.route_spec_store();

        let opt_safety_key = self.opt_safety_route_ref.as_ref().map(|r| r.route_key());
        let opt_srstr = opt_safety_key.map(|key| rss.display_route_by_key(key));
        let opt_remprstr = self
            .opt_remote_private_route_ref
            .as_ref()
            .map(|r| rss.display_route_by_key(r.route_key()));
        let opt_reply_key = self
            .opt_reply_private_route_ref
            .as_ref()
            .map(|r| r.route_key());
        let opt_repprstr = if opt_reply_key != opt_safety_key {
            opt_reply_key.map(|key| rss.display_route_by_key(key))
        } else {
            None
        };

        format!(
            "timeout={} send_ts={} send_data_result={} node={} dest={}{}{}{}",
            self.timeout,
            self.send_ts,
            self.send_data_result,
            self.node_ref,
            self.destination,
            if let Some(srstr) = opt_srstr {
                format!("\nsafety_route={}", srstr)
            } else {
                "".to_string()
            },
            if let Some(remprstr) = opt_remprstr {
                format!("\nremote_private_route={}", remprstr)
            } else {
                "".to_string()
            },
            if let Some(repprstr) = opt_repprstr {
                format!("\nreply_private_route={}", repprstr)
            } else {
                "".to_string()
            },
        )
    }
}

#[derive(Debug)]
#[must_use]
pub(super) struct WaitableReply {
    pub handle: OperationWaitHandle<Message, Option<Arc<QuestionContext>>>,
    _opt_connection_ref_scope: Option<ConnectionRefScope>,
    pub context: WaitableReplyContext,
}

impl WaitableReply {
    pub fn new(
        handle: OperationWaitHandle<Message, Option<Arc<QuestionContext>>>,
        opt_connection_ref_scope: Option<ConnectionRefScope>,
        context: WaitableReplyContext,
    ) -> Self {
        Self {
            handle,
            _opt_connection_ref_scope: opt_connection_ref_scope,
            context,
        }
    }
}

/// Outcome of `RPCProcessor::question`
#[derive(Debug)]
#[must_use]
pub(super) enum QuestionOutcome {
    /// Envelope sent; caller awaits the WaitableReply for the answer (boxed: much larger than the other variants)
    Sent(Box<WaitableReply>),
    /// Send was attempted but didn't produce a Value; SendDataResult exposes what was tried
    Failed(SendDataResult),
    /// Never made it to send_envelope (render failure, no routing domain, etc.)
    NotSent(NetworkResult<()>),
}

impl QuestionOutcome {
    /// Collapse into NetworkResult<WaitableReply> for callers that don't need the SendDataResult
    pub(super) fn into_network_result(self) -> NetworkResult<WaitableReply> {
        match self {
            QuestionOutcome::Sent(wr) => NetworkResult::Value(*wr),
            QuestionOutcome::Failed(sdr) => sdr.into_network_result().map(|_| unreachable!()),
            QuestionOutcome::NotSent(nr) => nr.map(|_| unreachable!()),
        }
    }
}
