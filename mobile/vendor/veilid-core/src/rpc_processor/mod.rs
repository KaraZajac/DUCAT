use super::*;

mod answer;
mod coders;
#[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
mod debug;
mod descriptor_mode;
mod destination;
mod error;
mod fanout;
mod message;
mod message_header;
mod operation_waiter;
mod rendered_operation;
mod rpc_app_call;
mod rpc_app_message;
mod rpc_find_node;
mod rpc_get_value;
mod rpc_inspect_value;
mod rpc_return_receipt;
mod rpc_route;
mod rpc_set_value;
mod rpc_signal;
mod rpc_status;
mod rpc_transact_begin;
mod rpc_transact_command;
mod rpc_validate_dial_info;
mod rpc_value_changed;
mod rpc_watch_value;
mod rpc_worker;
mod sender_info;
mod sender_peer_info;
mod waitable_reply;

#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod tests_rpc_processor;

pub use coders::encode_nonce as capnp_encode_nonce;
pub use coders::encode_public_key as capnp_encode_public_key;

#[cfg(feature = "unstable-blockstore")]
mod rpc_find_block;
#[cfg(feature = "unstable-blockstore")]
mod rpc_supply_block;

#[cfg(feature = "unstable-tunnels")]
mod rpc_cancel_tunnel;
#[cfg(feature = "unstable-tunnels")]
mod rpc_complete_tunnel;
#[cfg(feature = "unstable-tunnels")]
mod rpc_start_tunnel;

pub(crate) use answer::*;
pub(crate) use coders::{
    canonical_message_builder_to_bytes_writer_packed,
    canonical_message_builder_to_bytes_writer_unpacked, decode_node_info, decode_private_route,
    encode_node_info, encode_private_route, encode_route_hop, RPCDecodeContext, TransactCommand,
};
pub(crate) use descriptor_mode::*;
pub(crate) use destination::*;
pub(crate) use error::*;
pub(crate) use fanout::*;
pub(crate) use rpc_status::StatusResult;
pub(crate) use sender_info::*;
pub(crate) use waitable_reply::WaitableReplyContext;

use coders::*;
use message::*;
use message_header::*;
use operation_waiter::*;
use rendered_operation::*;
use rpc_worker::*;
use sender_peer_info::*;
use waitable_reply::*;

use crypto::*;
use network_manager::*;
use routing_table::*;
use storage_manager::*;

impl_veilid_log_facility!("rpc");

/////////////////////////////////////////////////////////////////////

const RPC_WORKERS_PER_CORE: u32 = 16;

/////////////////////////////////////////////////////////////////////

#[derive(Copy, Clone, Debug)]
#[must_use]
enum RPCKind {
    Question,
    Statement,
    Answer,
}

/////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone)]
#[must_use]
pub struct RPCProcessorStartupContext {
    pub startup_lock: Arc<StartupLock>,
}
impl RPCProcessorStartupContext {
    pub fn new() -> Self {
        Self {
            startup_lock: Arc::new(StartupLock::new()),
        }
    }
}
impl Default for RPCProcessorStartupContext {
    fn default() -> Self {
        Self::new()
    }
}

/////////////////////////////////////////////////////////////////////

#[derive(Debug)]
#[must_use]
struct RPCProcessorInner {
    /// The channel for sending RPCs to the workers
    rpc_send_channel: Option<flume::Sender<RPCWorkerRequest>>,
    /// The stop source for the RPC workers
    rpc_stop_source: Option<StopSource>,
    /// The join handles for the RPC workers
    rpc_worker_join_handles: Vec<MustJoinHandle<()>>,
    /// The dequeue latency stats for the RPC workers
    rpc_worker_dequeue_latency: LatencyStats,
    /// The processing latency stats for the RPC workers
    rpc_worker_process_latency: LatencyStats,
    /// The dequeue latency stats accounting for the RPC workers
    rpc_worker_dequeue_latency_accounting: LatencyStatsAccounting,
    /// The processing latency stats accounting for the RPC workers
    rpc_worker_process_latency_accounting: LatencyStatsAccounting,
    /// The processing latency stats and accounting for the RPC workers by operation kind
    rpc_worker_process_latency_and_accounting_by_operation_kind:
        BTreeMap<&'static str, (LatencyStats, LatencyStatsAccounting)>,
}

#[derive(Debug)]
#[must_use]
pub(crate) struct RPCProcessor {
    /// The component registry
    registry: VeilidComponentRegistry,
    /// The inner state of the RPC processor
    inner: Mutex<RPCProcessorInner>,
    /// The timeout for a direct RPC sent to a node with no extra hops
    timeout: TimestampDuration,
    /// The queue length for the RPC send/recv channels
    queue_size: u32,
    /// The number of RPC workers to spin up
    concurrency: u32,
    /// The RPCs that are waiting for a reply
    waiting_rpc_table: OperationWaiter<Message, Option<Arc<QuestionContext>>>,
    /// The app calls that are waiting for a reply
    waiting_app_call_table: OperationWaiter<Bytes, ()>,
    /// Pool that decouples reply waits from caller cancellation
    reply_pool: FutureRaceCompletionPool,
    /// The context for the startup of the RPC processor
    startup_context: RPCProcessorStartupContext,
}

impl_veilid_component!(RPCProcessor);

impl RPCProcessor {
    fn new_inner() -> RPCProcessorInner {
        RPCProcessorInner {
            rpc_send_channel: None,
            rpc_stop_source: None,
            rpc_worker_join_handles: Vec::new(),
            rpc_worker_dequeue_latency: LatencyStats::default(),
            rpc_worker_process_latency: LatencyStats::default(),
            rpc_worker_dequeue_latency_accounting: LatencyStatsAccounting::new(),
            rpc_worker_process_latency_accounting: LatencyStatsAccounting::new(),
            rpc_worker_process_latency_and_accounting_by_operation_kind: BTreeMap::new(),
        }
    }

    pub fn new(
        registry: VeilidComponentRegistry,
        startup_context: RPCProcessorStartupContext,
    ) -> Self {
        // make local copy of node id for easy access
        let (concurrency, queue_size, timeout_us) = {
            let config = registry.config();

            // set up channel
            let mut concurrency = config.internal().network.rpc.concurrency;
            let queue_size = config.internal().network.rpc.queue_size;
            let timeout_us =
                TimestampDuration::new(ms_to_us(config.internal().network.rpc.timeout_ms));
            if concurrency == 0 {
                concurrency = get_concurrency();
                if concurrency == 0 {
                    concurrency = 1;
                }

                // Default RPC concurrency is the number of CPUs * 16 rpc workers per core, as a single worker takes about 1% CPU when relaying and 16% is reasonable for baseline plus relay
                concurrency *= RPC_WORKERS_PER_CORE;
            }
            (concurrency, queue_size, timeout_us)
        };

        Self {
            waiting_rpc_table: OperationWaiter::new(registry.clone()),
            waiting_app_call_table: OperationWaiter::new(registry.clone()),
            reply_pool: FutureRaceCompletionPool::new("rpc_processor_reply_pool"),
            registry,
            inner: Mutex::new(Self::new_inner()),
            timeout: timeout_us,
            queue_size,
            concurrency,
            startup_context,
        }
    }

    /////////////////////////////////////
    // Initialization

    fn log_facilities_impl(&self) -> VeilidComponentLogFacilities {
        VeilidComponentLogFacilities::new()
            .with_facility(
                VeilidComponentLogFacility::try_new_with_tags("rpc", ["#common"]).unwrap(),
            )
            .with_facility(
                VeilidComponentLogFacility::try_new_with_tags(
                    "rpc_message",
                    ["#dht", "#fanout", "#verbose"],
                )
                .unwrap(),
            )
            .with_facility(VeilidComponentLogFacility::try_new_with_tags("dht", ["#dht"]).unwrap())
            .with_facility(
                VeilidComponentLogFacility::try_new_with_tags(
                    "network_result",
                    ["#dht", "#fanout", "#verbose"],
                )
                .unwrap(),
            )
            .with_facility(
                VeilidComponentLogFacility::try_new_with_tags("fanout", ["#fanout"]).unwrap(),
            )
    }

    #[expect(clippy::unused_async)]
    async fn init_async(&self) -> EyreResult<()> {
        Ok(())
    }

    #[expect(clippy::unused_async)]
    async fn post_init_async(&self) -> EyreResult<()> {
        Ok(())
    }

    #[expect(clippy::unused_async)]
    async fn pre_terminate_async(&self) {
        // Ensure things have shut down
        debug_assert!(
            self.startup_context.startup_lock.is_shut_down(),
            "should have shut down by now"
        );
    }

    #[expect(clippy::unused_async)]
    async fn terminate_async(&self) {}

    //////////////////////////////////////////////////////////////////////

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn startup(&self) -> EyreResult<()> {
        veilid_log!(self debug "starting rpc processor startup");

        let guard = self.startup_context.startup_lock.startup()?;
        {
            let mut inner = self.inner.lock();

            let channel = flume::bounded(self.queue_size as usize);
            inner.rpc_send_channel = Some(channel.0.clone());
            inner.rpc_stop_source = Some(StopSource::new());
        }

        self.startup_rpc_workers()?;

        guard.success();

        veilid_log!(self debug "finished rpc processor startup");

        Ok(())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn shutdown(&self) {
        veilid_log!(self debug "starting rpc processor shutdown");
        let guard = self
            .startup_context
            .startup_lock
            .shutdown()
            .await
            .expect_or_log("should be started up");

        self.shutdown_rpc_workers().await;

        // Wait for any reply-pool tasks (in-flight wait_for_reply futures
        // whose callers have moved on) to complete naturally.
        self.reply_pool.shutdown().await;

        veilid_log!(self debug "resetting rpc processor state");

        // Release the rpc processor
        *self.inner.lock() = Self::new_inner();

        guard.success();
        veilid_log!(self debug "finished rpc processor shutdown");
    }

    //////////////////////////////////////////////////////////////////////

    /// Get waiting app call id for debugging purposes
    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub fn get_app_call_ids(&self) -> Vec<OperationId> {
        self.waiting_app_call_table.get_operation_ids()
    }

    /// Incorporate 'sender peer info' sent along with an RPC message
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn process_sender_peer_info(
        &self,
        sender_node_id: NodeId,
        sender_peer_info: &SenderPeerInfo,
    ) -> RPCNetworkResult<Option<NodeRef>> {
        let Some(peer_info) = sender_peer_info.opt_peer_info() else {
            return Ok(NetworkResult::value(None));
        };

        // Ensure the sender peer info is for the actual sender specified in the envelope
        if !peer_info.node_ids().contains(&sender_node_id) {
            // Attempted to update peer info for the wrong node id
            self.network_manager()
                .address_filter()
                .punish_node_id(sender_node_id, PunishmentReason::WrongSenderPeerInfo);

            return Ok(NetworkResult::invalid_message(
                "attempt to update peer info for non-sender node id",
            ));
        }

        // Register the peer info in its translated routing domain
        // Translation is done at deserialization time
        veilid_log!(self debug target: "network_result", "processing sender peer info: id={} pi={:?}", sender_node_id, sender_peer_info);
        let sender_nr = match self
            .routing_table()
            .register_node_with_peer_info(peer_info, false)
        {
            Ok(v) => v.unfiltered(),
            Err(e) => {
                self.network_manager().address_filter().punish_node_id(
                    sender_node_id,
                    PunishmentReason::FailedToRegisterSenderPeerInfo,
                );
                return Ok(NetworkResult::invalid_message(e));
            }
        };

        Ok(NetworkResult::value(Some(sender_nr)))
    }

    //////////////////////////////////////////////////////////////////////

    /// Search the public internet routing domain for a single node and add
    /// it to the routing table and return the node reference
    /// If no node was found in the timeout, this returns None
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn public_internet_peer_search(
        &self,
        node_id: NodeId,
        safety_selection: SafetySelection,
    ) -> TimeoutOr<Result<Option<NodeRef>, RPCError>> {
        let routing_table = self.routing_table();
        let routing_domain = RoutingDomain::PublicInternet;

        // Ignore own node
        if routing_table.matches_own_node_id(std::slice::from_ref(&node_id)) {
            return TimeoutOr::Value(Err(RPCError::network("can't search for own node id")));
        }

        // If nobody knows where this node is, ask the DHT for it
        let config = self.config();
        let node_count = config.internal().network.dht.max_find_node_count as usize;
        // let consensus_count = config.internal().network.dht.resolve_node_count as usize;
        // let consensus_width = config.internal().network.dht.consensus_width as usize;
        let fanout_tasks = config.internal().network.dht.resolve_node_fanout as usize;
        let timeout = TimestampDuration::from(ms_to_us(
            config.internal().network.dht.resolve_node_timeout_ms,
        ));

        // Routine to call to generate fanout
        let result = Arc::new(Mutex::new(Option::<NodeRef>::None));

        let registry = self.registry();
        let node_id2 = node_id.clone();
        let call_routine = Arc::new(move |next_node: NodeRef| {
            let registry = registry.clone();
            let node_id = node_id2.clone();
            let safety_selection = safety_selection.clone();
            Box::pin(async move {
                let this = registry.rpc_processor();
                match Box::pin(this.rpc_call_find_node(
                    Destination::direct(
                        next_node.routing_domain_filtered(routing_domain),
                        Some(safety_selection.clone()),
                    ),
                    node_id.to_hash_coordinate(),
                    vec![],
                ))
                .await?
                {
                    NetworkResult::Timeout => Ok(FanoutCallOutput {
                        peer_info_list: vec![],
                        disposition: FanoutCallDisposition::Timeout,
                    }),
                    NetworkResult::ServiceUnavailable(_)
                    | NetworkResult::NoConnection(_)
                    | NetworkResult::AlreadyExists(_)
                    | NetworkResult::InvalidMessage(_) => Ok(FanoutCallOutput {
                        peer_info_list: vec![],
                        disposition: FanoutCallDisposition::Rejected,
                    }),
                    NetworkResult::Value(v) => Ok(FanoutCallOutput {
                        peer_info_list: v.answer,
                        disposition: FanoutCallDisposition::Accepted,
                    }),
                }
            }) as PinBoxFuture<FanoutCallResult>
        });

        // Routine to call to check if we're done at each step
        let result2 = result.clone();
        let node_id2 = node_id.clone();
        let check_done = Arc::new(move |_: &FanoutResult| -> FanoutDoneDisposition {
            let Ok(Some(nr)) = routing_table.lookup_node_id(node_id2.clone()) else {
                return FanoutDoneDisposition::NotDone;
            };

            // ensure we have some dial info for the entry already,
            // and that the node is not dead
            // if not, we should keep looking for better info
            if nr.state(Timestamp::now_non_decreasing()).is_live() && nr.has_any_dial_info() {
                *result2.lock() = Some(nr);
                return FanoutDoneDisposition::DoneEarly;
            }

            FanoutDoneDisposition::NotDone
        });

        // Call the fanout
        let routing_table = self.routing_table();
        let fanout_call = FanoutCall::new(
            &routing_table,
            FanoutCallParams {
                name: format!(
                    "public_internet_peer_search({})",
                    Timestamp::now_increasing()
                ),
                hash_coordinate: node_id.to_hash_coordinate(),
                node_count,
                fanout_tasks,
                consensus_count: 0,
                consensus_width: 0,
                timeout,
            },
            empty_fanout_peer_info_filter(),
            call_routine,
            check_done,
        );
        let Some(shutdown_stop_source) = self.startup_context.startup_lock.stop_token() else {
            return TimeoutOr::value(Err(RPCError::ignore("shutting down")));
        };

        match fanout_call
            .run(vec![], FanoutQueueMode::Unthrottled, shutdown_stop_source)
            .await
        {
            Ok(fanout_result) => {
                if matches!(fanout_result.kind, FanoutResultKind::Timeout) {
                    TimeoutOr::timeout()
                } else {
                    TimeoutOr::value(Ok(result.lock().take()))
                }
            }
            Err(e) => TimeoutOr::value(Err(e)),
        }
    }

    /// Search the DHT for a specific node corresponding to a key unless we
    /// have that node in our routing table already, and return the node reference
    /// Note: This routine can possibly be recursive, hence the PinBoxFuture async form
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub fn resolve_node(
        &self,
        node_id: NodeId,
        safety_selection: SafetySelection,
    ) -> PinBoxFuture<'_, Result<Option<NodeRef>, RPCError>> {
        let registry = self.registry();
        Box::pin(
            async move {
                let this = registry.rpc_processor();

                let _guard = this
                    .startup_context
                    .startup_lock
                    .enter()
                    .map_err(RPCError::map_try_again("not started up"))?;

                let routing_table = this.routing_table();

                // First see if we have the node in our routing table already
                let mut existing_nr = None;
                if let Some(nr) = routing_table
                    .lookup_node_id(node_id.clone())
                    .map_err(RPCError::internal)?
                {
                    existing_nr = Some(nr.clone());

                    // ensure we have some dial info for the entry already,
                    // and that the node is still alive
                    // if not, we should do the find_node anyway
                    if nr.state(Timestamp::now_non_decreasing()).is_live() && nr.has_any_dial_info()
                    {
                        return Ok(Some(nr));
                    }
                }

                // Search routing domains for peer
                // xxx: Eventually add other routing domains here
                let nr = match this
                    .public_internet_peer_search(node_id, safety_selection)
                    .await
                {
                    TimeoutOr::Timeout => None,
                    TimeoutOr::Value(Ok(v)) => v,
                    TimeoutOr::Value(Err(e)) => {
                        return Err(e);
                    }
                };

                // Either return the node we just resolved or a dead one we found in the routing table to try again
                Ok(nr.or(existing_nr))
            }
            .in_current_span(),
        )
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(
            __VEILID_LOG_KEY = self.log_key(),
            arg_debug_string = %debug_string,
            arg_destination = %waitable_reply.context.destination,
            arg_node_ref = %waitable_reply.context.node_ref,
            arg_safety_route = tracing::field::Empty,
            arg_remote_private_route = tracing::field::Empty,
            arg_reply_private_route = tracing::field::Empty,
            arg_timeout_ms = (waitable_reply.context.timeout.as_u64() / 1000),
            ret_outcome = tracing::field::Empty,
            ret_latency_ms = tracing::field::Empty,
            ret_answer_body_len = tracing::field::Empty,
        ))
    )]
    async fn wait_for_reply(
        &self,
        waitable_reply: WaitableReply,
        debug_string: String,
    ) -> Result<TimeoutOr<(Message, AnswerContext)>, RPCError> {
        let id = waitable_reply.handle.id();

        #[cfg(feature = "instrument")]
        {
            let span = tracing::Span::current();
            let ctx = &waitable_reply.context;
            span.record(
                "arg_safety_route",
                tracing::field::debug(&ctx.opt_safety_route_ref),
            );
            span.record(
                "arg_remote_private_route",
                tracing::field::debug(&ctx.opt_remote_private_route_ref),
            );
            span.record(
                "arg_reply_private_route",
                tracing::field::debug(&ctx.opt_reply_private_route_ref),
            );
        }

        // Spawn the actual wait into the reply pool so a dropped caller
        // doesn't cancel the answer wait — the question's outbound RPC
        // always completes, even if no one is left listening.
        let waiter = self.waiting_rpc_table.clone();
        let timeout = waitable_reply.context.timeout;
        let handle = waitable_reply.handle;
        let registry = self.registry();
        let rx = self
            .reply_pool
            .channel_spawn(async move {
                let this = registry.rpc_processor();
                let res = waiter.wait_for_op(handle, timeout).await;
                match res {
                    Err(e) => {
                        #[cfg(feature = "verbose-tracing")]
                        let context_debug = {
                            let routing_table = &*this.routing_table();
                            format!("{}\n", waitable_reply.context.debug(routing_table))
                        };
                        #[cfg(not(feature = "verbose-tracing"))]
                        let context_debug = waitable_reply
                            .context
                            .send_data_result
                            .unique_flow()
                            .map(|uf| format!("flow={:#}", uf.flow))
                            .unwrap_or_else(|| "flow=<none>".to_owned());
                        veilid_log!(this debug "RPC Lost (id={:#} {} {}): {}", id, debug_string, e, context_debug);
                        this.record_lost_question(&waitable_reply.context).await;

                        #[cfg(feature = "instrument")]
                        tracing::Span::current().record("ret_outcome", "error");

                        Err(e)
                    }
                    Ok(TimeoutOr::Timeout) => {
                        #[cfg(feature = "verbose-tracing")]
                        let context_debug = {
                            let routing_table = &*this.routing_table();
                            format!("{}\n", waitable_reply.context.debug(routing_table))
                        };
                        #[cfg(not(feature = "verbose-tracing"))]
                        let context_debug = waitable_reply
                            .context
                            .send_data_result
                            .unique_flow()
                            .map(|uf| format!("flow={:#}", uf.flow))
                            .unwrap_or_else(|| "flow=<none>".to_owned());
                        veilid_log!(this debug "RPC Lost (id={:#} {} Timeout): {}", id, debug_string, context_debug);
                        this.record_lost_question(&waitable_reply.context).await;

                        #[cfg(feature = "instrument")]
                        tracing::Span::current().record("ret_outcome", "timeout");

                        Ok(TimeoutOr::Timeout)
                    }
                    Ok(TimeoutOr::Value((rpcreader, latency))) => {
                        // Reply received
                        let recv_ts = Timestamp::now_non_decreasing();

                        // Record answer received
                        this.record_answer_received(
                            recv_ts,
                            rpcreader.header.body_len,
                            &waitable_reply.context,
                        );

                        #[cfg(feature = "instrument")]
                        {
                            let span = tracing::Span::current();
                            span.record("ret_outcome", "value");
                            span.record("ret_latency_ms", latency.as_u64() / 1000);
                            span.record("ret_answer_body_len", rpcreader.header.body_len.as_u64());
                        }

                        // Ensure the reply comes over the private route that was requested
                        if let Some(reply_private_route) = waitable_reply
                            .context
                            .opt_reply_private_route_ref
                            .as_ref()
                            .map(|r| r.route_key().clone())
                        {
                            match &rpcreader.header.detail {
                                RPCMessageHeaderDetail::Direct(_) => {
                                    return Err(RPCError::protocol(
                                        "should have received reply over private route or stub",
                                    ));
                                }
                                RPCMessageHeaderDetail::SafetyRouted(sr) => {
                                    let public_key = this
                                        .routing_table()
                                        .public_key(sr.direct.envelope.get_crypto_kind());
                                    if public_key != reply_private_route {
                                        return Err(RPCError::protocol(
                                            "should have received reply from safety route to a stub",
                                        ));
                                    }
                                }
                                RPCMessageHeaderDetail::PrivateRouted(pr) => {
                                    if pr.private_route != reply_private_route {
                                        return Err(RPCError::protocol(
                                            "received reply over the wrong private route",
                                        ));
                                    }
                                }
                            };
                        }

                        Ok(TimeoutOr::Value((
                            rpcreader,
                            AnswerContext {
                                latency,
                                waitable_reply_context: waitable_reply.context,
                            },
                        )))
                    }
                }
            });
        match rx.recv_async().await {
            Ok(inner) => inner,
            Err(e) => Err(RPCError::ignore(e)),
        }
    }

    /// Wrap an operation with a private route inside a safety route
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(
            __VEILID_LOG_KEY = self.log_key(),
            arg_routing_domain = %routing_domain,
            arg_safety_selection = %safety_selection,
            arg_remote_private_route = %remote_private_route.public_key,
            arg_remote_pr_is_stub = remote_private_route.is_stub(),
            arg_opt_reply_private_route = tracing::field::debug(&opt_reply_private_route),
            arg_signed_message_data_len = signed_message_data.len(),
            ret_safety_route = tracing::field::Empty,
            ret_sr_is_stub = tracing::field::Empty,
            ret_remote_private_route = tracing::field::Empty,
            ret_reply_private_route = tracing::field::Empty,
            ret_outer_op_id = tracing::field::Empty,
            ret_first_hop = tracing::field::Empty,
            ret_message_len = tracing::field::Empty,
        ))
    )]
    async fn wrap_with_route(
        &self,
        routing_domain: RoutingDomain,
        safety_selection: SafetySelection,
        remote_private_route: Arc<PrivateRoute>,
        opt_reply_private_route: Option<PublicKey>,
        signed_message_data: Bytes,
    ) -> RPCNetworkResult<RenderedOperation> {
        let routing_table = self.routing_table();
        let crypto = self.crypto();
        let rss = routing_table.route_spec_store();

        // Get useful private route properties
        let pr_is_stub = remote_private_route.is_stub();
        let pr_pubkey = remote_private_route.public_key.clone();
        let crypto_kind = remote_private_route.crypto_kind();
        let Some(vcrypto) = crypto.get_async(crypto_kind) else {
            return Err(RPCError::internal(
                "crypto not available for selected private route",
            ));
        };

        // Compile the safety route with the private route
        let sequencing = safety_selection.get_sequencing();
        let params = CompileRouteParams {
            safety_selection,
            private_route: remote_private_route,
            opt_reply_pr_pubkey: opt_reply_private_route.clone(),
        };
        let compiled_route: Arc<CompiledRoute> =
            network_result_try!(rss.compile_route(&params).await.to_rpc_network_result()?);
        let sr_is_stub = compiled_route.compiled_safety_route.is_stub();
        let sr_pubkey = compiled_route.compiled_safety_route.public_key.clone();

        // Encrypt routed operation
        // Xmsg + ENC(Xmsg, DH(PKapr, SKbsr))
        let nonce = vcrypto.random_nonce().await;
        let dh_secret = vcrypto
            .cached_dh(&pr_pubkey, &compiled_route.secret)
            .await
            .map_err(RPCError::map_internal("dh failed"))?;
        let enc_msg_data = vcrypto
            .encrypt_aead(signed_message_data, &nonce, &dh_secret, None)
            .await
            .map_err(RPCError::map_internal("encryption failed"))?;

        // Make the routed operation
        let operation =
            RoutedOperation::new(routing_domain, sequencing, vec![], nonce, enc_msg_data)?;

        // Prepare route operation
        let route_operation =
            RPCOperationRoute::new(compiled_route.compiled_safety_route.clone(), operation);
        let ssni_route =
            self.get_sender_peer_info(&Destination::direct(compiled_route.first_hop.clone(), None));
        let operation = RPCOperation::new_statement(
            RPCStatement::new(RPCStatementDetail::Route(Box::new(route_operation))),
            ssni_route,
        );
        let op_id = operation.op_id();

        let signed_operation = RPCSignedOperation::sign(&crypto, &operation, None).await?;

        // Convert message to bytes and return it
        let mut route_msg = ::capnp::message::Builder::new_default();
        let mut route_operation = route_msg.init_root::<veilid_capnp::signed_operation::Builder>();
        signed_operation.encode(&mut route_operation)?;
        let message = canonical_message_builder_to_bytes_writer_packed(route_msg, |size| {
            BytesWriter::with_capacity(size)
        })?
        .into_inner()
        .freeze();

        // Hold refs to all routes in use so they remain locked until the operation is processed.
        // The ref carries both the route identity (route_key) and the lifetime lock.
        let opt_safety_route_ref = if sr_is_stub {
            None
        } else {
            rss.lock_allocated_route_by_key(&sr_pubkey)
        };
        let opt_remote_private_route_ref = if pr_is_stub {
            None
        } else {
            rss.lock_remote_route_by_key(&pr_pubkey)
        };
        let opt_reply_private_route_ref = opt_reply_private_route
            .as_ref()
            .and_then(|reply_private_route| rss.lock_allocated_route_by_key(reply_private_route));

        let out = RenderedOperation {
            outer_op_id: op_id,
            message,
            node_ref: compiled_route.first_hop.clone(),
            opt_dial_info: None,
            opt_safety_route_ref,
            opt_remote_private_route_ref,
            opt_reply_private_route_ref,
        };

        #[cfg(feature = "instrument")]
        {
            let span = tracing::Span::current();
            span.record(
                "ret_safety_route",
                tracing::field::debug(&out.opt_safety_route_ref),
            );
            span.record("ret_sr_is_stub", sr_is_stub);
            span.record(
                "ret_remote_private_route",
                tracing::field::debug(&out.opt_remote_private_route_ref),
            );
            span.record(
                "ret_reply_private_route",
                tracing::field::debug(&out.opt_reply_private_route_ref),
            );
            span.record("ret_outer_op_id", out.outer_op_id.as_u64());
            span.record("ret_first_hop", tracing::field::display(&out.node_ref));
            span.record("ret_message_len", out.message.len());
        }

        Ok(NetworkResult::value(out))
    }

    /// Produce a byte buffer that represents the wire encoding of the entire
    /// unencrypted envelope body for a RPC message. This incorporates
    /// wrapping a private and/or safety route if they are specified.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn render_signed_operation(
        &self,
        dest: Destination,
        operation: RPCOperation,
        signing_params: Option<RPCSigningParams>,
    ) -> RPCNetworkResult<RenderedOperation> {
        let registry = self.registry();

        let rpc_processor = registry.rpc_processor();
        let crypto = registry.crypto();
        let out: NetworkResult<RenderedOperation>;

        // Encode message to a builder and make a message reader for it
        // Then produce the message as an unencrypted byte buffer

        // Sign question if desired
        let op_id = operation.op_id();
        let signed_operation =
            RPCSignedOperation::sign(&crypto, &operation, signing_params).await?;
        let message = {
            let mut msg_builder = ::capnp::message::Builder::new_default();
            let mut op_builder = msg_builder.init_root::<veilid_capnp::signed_operation::Builder>();
            signed_operation.encode(&mut op_builder)?;
            canonical_message_builder_to_bytes_writer_packed(msg_builder, |size| {
                BytesWriter::with_capacity(size)
            })?
            .into_inner()
            .freeze()
        };

        // Get reply private route if we are asking for one to be used in our 'respond to'
        let opt_reply_private_route = match operation.kind() {
            RPCOperationKind::Question(q) => match q.respond_to() {
                RespondTo::Sender => None,
                RespondTo::PrivateRoute(pr) => Some(pr.public_key.clone()),
            },
            RPCOperationKind::Statement(_) | RPCOperationKind::Answer(_) => None,
        };

        // To where are we sending the request
        match dest {
            Destination::DialInfo {
                dial_info,
                node: ref node_ref,
            } => {
                // Send to a node directly via a specific dialinfo

                // Reply private route should be None here, even for questions
                if opt_reply_private_route.is_some() {
                    return Err(RPCError::internal(
                        "reply private route should never be specified for relay destinations",
                    ));
                }

                // If no safety route is being used, and we're not sending to a private
                // route, we can use a direct envelope instead of routing
                out = NetworkResult::value(RenderedOperation {
                    outer_op_id: op_id,
                    message,
                    node_ref: node_ref.sequencing_filtered(
                        dial_info
                            .protocol_type()
                            .sequence_ordering()
                            .strict_sequencing(),
                    ),
                    opt_dial_info: Some(dial_info),
                    opt_safety_route_ref: None,
                    opt_remote_private_route_ref: None,
                    opt_reply_private_route_ref: None,
                });
            }
            Destination::Direct {
                node: ref node_ref,
                safety_selection,
            } => {
                // Send to a node without a private route
                // --------------------------------------

                // Handle the existence of safety route
                match safety_selection {
                    SafetySelection::Unsafe(sequencing) => {
                        // Apply safety selection sequencing requirement if it is more strict than the node_ref's sequencing requirement
                        let mut node_ref = node_ref.clone();
                        if sequencing > node_ref.sequencing() {
                            node_ref.set_sequencing(sequencing)
                        }

                        // Reply private route should be None here, even for questions
                        if opt_reply_private_route.is_some() {
                            return Err(RPCError::internal("reply private route should never be specified for direct destinations"));
                        }

                        // If no safety route is being used, and we're not sending to a private
                        // route, we can use a direct envelope instead of routing
                        out = NetworkResult::value(RenderedOperation {
                            outer_op_id: op_id,
                            message,
                            node_ref,
                            opt_dial_info: None,
                            opt_safety_route_ref: None,
                            opt_remote_private_route_ref: None,
                            opt_reply_private_route_ref: None,
                        });
                    }
                    SafetySelection::Safe(_) => {
                        // For now we only private-route over PublicInternet
                        let routing_domain = RoutingDomain::PublicInternet;

                        // No private route was specified for the request
                        // but we are using a safety route, so we must create an empty private route
                        // Destination relay is ignored for safety routed operations
                        let peer_info = match node_ref.get_peer_info(routing_domain) {
                            None => {
                                return Ok(NetworkResult::no_connection_other(
                                    "No peer info for stub private route",
                                ))
                            }
                            Some(pi) => pi,
                        };
                        let private_route = Arc::new(PrivateRoute::new_stub(
                            match node_ref.best_public_key(routing_domain) {
                                Some(pk) => pk,
                                None => {
                                    return Ok(NetworkResult::no_connection_other(
                                        "No best node id for stub private route",
                                    ));
                                }
                            },
                            RouteNode::PeerInfo(peer_info),
                        ));

                        // Wrap with safety route
                        out = rpc_processor
                            .wrap_with_route(
                                routing_domain,
                                safety_selection.clone(),
                                private_route,
                                opt_reply_private_route,
                                message,
                            )
                            .await?;

                        if enabled!(target: "rpc_message", Level::DEBUG) {
                            let (route_op_id, safety_route) = match &out {
                                NetworkResult::Value(v) => (
                                    Some(v.outer_op_id.as_u64()),
                                    v.opt_safety_route_ref.as_ref().map(tracing::field::debug),
                                ),
                                _ => (None, None),
                            };

                            veilid_log!(rpc_processor debug target: "rpc_message", fields:
                                dir = "send",
                                kind = "route",
                                route_op_id,
                                op_id = op_id.as_u64(),
                                ?node_ref,
                                safety_route,
                                desc = "wrap_with_route"
                            );
                        }
                    }
                };
            }
            Destination::PrivateRoute {
                private_route,
                safety_selection,
            } => {
                // For now we only private-route over PublicInternet
                let routing_domain = RoutingDomain::PublicInternet;

                // Send to private route
                // ---------------------
                // Reply with 'route' operation
                out = rpc_processor
                    .wrap_with_route(
                        routing_domain,
                        safety_selection,
                        private_route.clone(),
                        opt_reply_private_route,
                        message,
                    )
                    .await?;

                if enabled!(target: "rpc_message", Level::DEBUG) {
                    let (route_op_id, safety_route) = match &out {
                        NetworkResult::Value(v) => (
                            Some(v.outer_op_id.as_u64()),
                            v.opt_safety_route_ref.as_ref().map(tracing::field::debug),
                        ),
                        _ => (None, None),
                    };

                    veilid_log!(rpc_processor debug target: "rpc_message", fields:
                        dir = "send",
                        kind = "route",
                        route_op_id,
                        op_id = op_id.as_u64(),
                        safety_route,
                        private_route = tracing::field::display(&private_route.public_key),
                        desc = "wrap_with_route"
                    );
                }
            }
        }

        Ok(out)
    }

    /// Get signed node info to package with RPC messages to improve
    /// routing table caching when it is okay to do so
    /// Also check target's timestamp of our own node info, to see if we should send that
    /// And send our timestamp of the target's node info so they can determine if they should update us on their next rpc
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn get_sender_peer_info(&self, dest: &Destination) -> SenderPeerInfo {
        // veilid_log!(self debug target: "network_result", "get_sender_peer_info: {:?}", dest);

        let routing_table = self.routing_table();
        // Don't do this if the sender is to remain private
        // Otherwise we would be attaching the original sender's identity to the final destination,
        // thus defeating the purpose of the safety route entirely :P
        let Some(UnsafeRoutingInfo {
            opt_node,
            opt_routing_domain,
        }) = dest.get_unsafe_routing_info(&routing_table)
        else {
            return SenderPeerInfo::default();
        };
        let Some(node) = opt_node else {
            // If this is going over a private route, don't bother sending any sender peer info
            // The other side won't accept it because peer info sent over a private route
            // could be used to deanonymize the private route's endpoint
            return SenderPeerInfo::default();
        };
        let Some(routing_domain) = opt_routing_domain else {
            // No routing domain for target, no node info is safe to send here
            // Only a stale connection or no connection exists, or an unexpected
            // relay was used, possibly due to the destination switching relays
            // in a race condition with our send
            return SenderPeerInfo::default();
        };

        // Get the target's node info timestamp
        let target_node_info_ts = node.node_info_ts(routing_domain);
        // veilid_log!(self debug target: "network_result", "target_node_info_ts: rd={:?} ts={}", routing_domain, target_node_info_ts);

        // Return whatever peer info we have even if the network class is not yet valid
        // That away we overwrite any prior existing valid-network-class nodeinfo in the remote routing table
        let routing_table = self.routing_table();

        if let Some(published_peer_info) = routing_table.get_published_peer_info(routing_domain) {
            // If the target has not yet seen our published peer info, send it along if we have it
            if !node.has_seen_our_node_info_ts(routing_domain) {
                return SenderPeerInfo::new(published_peer_info, target_node_info_ts);
            }
        }
        SenderPeerInfo::new_no_peer_info(target_node_info_ts)
    }

    /// Record a send failure on a node where we couldn't determine a routing domain
    /// for the target. This is a configuration mismatch with the target rather than a
    /// network failure, so it does not feed the online-detector.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn record_unreachable(&self, node_ref: NodeRef) {
        // No transport was attempted (no routing domain, no peer info, no contact method).
        // Record at the node level and clear last flows so a fresh attempt rebuilds state.
        node_ref.stats_unreachable();
        node_ref.clear_last_flows();
    }

    /// Node ids attempted by a routed RPC, for offline detection: the route hops plus any
    /// remote private route first hop.
    fn online_detector_failure_node_ids(
        opt_safety_route: Option<&AllocatedRouteRef>,
        opt_reply_private_route: Option<&AllocatedRouteRef>,
        opt_remote_private_route: Option<&RemoteRouteRef>,
    ) -> HashSet<NodeId> {
        let mut node_ids = HashSet::new();
        if let Some(sr) = opt_safety_route {
            node_ids.extend(sr.hop_node_ids());
        }
        if let Some(rpr) = opt_reply_private_route {
            // Skip if the reply route is the same as the safety route.
            if opt_safety_route.map(|sr| sr.route_key()) != Some(rpr.route_key()) {
                node_ids.extend(rpr.hop_node_ids());
            }
        }
        if let Some(prr) = opt_remote_private_route {
            if let Some(first_hop) = prr.first_hop_node_id() {
                node_ids.insert(first_hop);
            }
        }
        node_ids
    }

    /// Record failure to send to node or route
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn record_send_failure(
        &self,
        rpc_kind: RPCKind,
        send_ts: Timestamp,
        node_ref: FilteredNodeRef,
        opt_transport: Option<TransportType>,
        safety_route: Option<AllocatedRouteRef>,
        remote_private_route: Option<RemoteRouteRef>,
    ) {
        veilid_log!(self debug target: "network_result", "send failure: rpc_kind={:?} send_ts={}, node_ref={}, opt_transport={:?}, safety_route={:?}, remote_private_route={:?}",
            rpc_kind, send_ts, node_ref, opt_transport, safety_route, remote_private_route
        );

        let wants_answer = matches!(rpc_kind, RPCKind::Question);
        let is_routed = safety_route.is_some() || remote_private_route.is_some();

        // Determine routing domain for online detection. Routed sends are PublicInternet by construction.
        let opt_rd = if is_routed {
            Some(RoutingDomain::PublicInternet)
        } else {
            node_ref.best_routing_domain()
        };

        let network_manager = self.network_manager();
        let online_detector = network_manager.online_detector();
        // Only counts toward offline detection if a transport was actually attempted.
        if let (Some(rd), Some(transport)) = (opt_rd, opt_transport) {
            let node_ids: Vec<NodeId> = if is_routed {
                Self::online_detector_failure_node_ids(
                    safety_route.as_ref(),
                    None,
                    remote_private_route.as_ref(),
                )
                .into_iter()
                .collect()
            } else {
                vec![node_ref.best_node_id()]
            };
            online_detector.record_failure(rd, node_ids, send_ts, transport);
        }
        let suppress_degradation = opt_rd
            .map(|rd| online_detector.online_state(rd) == OnlineState::Offline)
            .unwrap_or(false);

        // Record for node if this was not sent via a route
        if !is_routed {
            if !suppress_degradation {
                if let Some(transport) = opt_transport {
                    // A specific transport failed to send, record it on the transport
                    node_ref.stats_failed_to_send(send_ts, wants_answer, transport);
                } else {
                    // Pre-send abort with no transport attempted:
                    // * NoRoutingDomain / NoPeerInfo / NoContactMethod / Punished
                    // * Or a ContactMethod::Existing that didn't exist
                    // Record at the node level.
                    node_ref.stats_unreachable();
                }
            }
            // Always clear last_flows: the underlying flow is gone or broken regardless.
            node_ref.clear_last_flows();
            return;
        }

        if suppress_degradation {
            return;
        }

        // Only attribute a route send failure when we know which transport (ordering) was attempted.
        let Some(ordering) = opt_transport.map(|t| t.sequence_ordering()) else {
            return;
        };
        if let Some(sr) = &safety_route {
            // If safety route was in use, record failure to send there
            sr.with_stats_mut(|s| s.record_send_failed(ordering));
        } else if let Some(pr) = &remote_private_route {
            // If no safety route was in use, then it's the private route's fault if we have one
            pr.with_stats_mut(|s| s.record_send_failed(ordering));
        }
    }

    /// Record question lost to node or route
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn record_lost_question(&self, context: &WaitableReplyContext) {
        // Determine routing domain for online detection. Routed sends are PublicInternet by construction.
        let opt_rd = if context.is_routed() {
            Some(RoutingDomain::PublicInternet)
        } else {
            context.node_ref.best_routing_domain()
        };

        let network_manager = self.network_manager();
        let online_detector = network_manager.online_detector();
        if let Some(rd) = opt_rd {
            let node_ids: Vec<NodeId> = if context.is_routed() {
                Self::online_detector_failure_node_ids(
                    context.opt_safety_route_ref.as_ref(),
                    context.opt_reply_private_route_ref.as_ref(),
                    context.opt_remote_private_route_ref.as_ref(),
                )
                .into_iter()
                .collect()
            } else {
                vec![context.node_ref.best_node_id()]
            };
            if let Some(transport) = context.send_data_result.transport_type() {
                online_detector.record_failure(rd, node_ids, context.send_ts, transport);
            }
        }
        let suppress_degradation = opt_rd
            .map(|rd| online_detector.online_state(rd) == OnlineState::Offline)
            .unwrap_or(false);

        // Record for node if this was not sent via a route
        if !context.is_routed() {
            // Skip demoting a node that was reliable before a recent reattach and
            // hasn't been re-tested since: we haven't had a fair chance to ping it,
            // so leave its reliability and ping cadence intact.
            let cur_ts = Timestamp::now();
            let protect_after_reattach = !suppress_degradation
                && opt_rd
                    .and_then(|rd| online_detector.online_since(rd))
                    .map(|online_ts| {
                        context
                            .node_ref
                            .rpc_stats()
                            .last_seen_ts
                            .map(|ls| ls < online_ts)
                            .unwrap_or(false)
                            && context.node_ref.state(cur_ts) == BucketEntryState::Reliable
                    })
                    .unwrap_or(false);
            if !suppress_degradation && !protect_after_reattach {
                if let Some(transport) = context.send_data_result.transport_type() {
                    context.node_ref.stats_lost_question(transport);
                }
            }
            // Always clear last_flows: the underlying flow is gone or broken regardless.
            context.node_ref.clear_last_flows();
            return;
        }

        if suppress_degradation {
            return;
        }

        // Ordering the send went out on; attribute the lost question to that ordering on the route.
        let opt_ordering = context.send_data_result.sequence_ordering();

        // If safety route was used, record question lost there
        if let Some(sr) = &context.opt_safety_route_ref {
            if let Some(ordering) = opt_ordering {
                sr.with_stats_mut(|s| s.record_lost_question(ordering, &context.destination));
            }

            // Trigger an out-of-band route test so silently-degraded routes
            // get released without waiting for the next regular cycle. The safety
            // route ref is always allocated, so use its set id directly.
            let route_id: RouteId = sr.route_set_id().clone().into();
            veilid_log!(self debug "scheduling out-of-band route test for {} after %SR timeout", route_id);
            let cur_ts = Timestamp::now();
            self.routing_table()
                .enqueue_route_tests(cur_ts, vec![route_id], 0)
                .await;
        }
        // If remote private route was used, record question lost there
        if let Some(pr) = &context.opt_remote_private_route_ref {
            if let Some(ordering) = opt_ordering {
                pr.with_stats_mut(|s| s.record_lost_question(ordering, &context.destination));
            }
        }
        // If reply private route was used, record question lost there
        // Skip if reply private route is the same as the safety route to avoid double-counting
        if let Some(rpr) = &context.opt_reply_private_route_ref {
            if context.opt_safety_route_ref.as_ref().map(|r| r.route_key()) != Some(rpr.route_key())
            {
                if let Some(ordering) = opt_ordering {
                    rpr.with_stats_mut(|s| s.record_lost_question(ordering, &context.destination));
                }
            }
        }
    }

    /// Record success sending to node or route
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    #[expect(clippy::too_many_arguments)]
    fn record_send_success(
        &self,
        rpc_kind: RPCKind,
        send_ts: Timestamp,
        bytes: ByteCount,
        node_ref: FilteredNodeRef,
        destination_node_ref: Option<NodeRef>,
        safety_route: Option<AllocatedRouteRef>,
        remote_private_route: Option<RemoteRouteRef>,
        transport: TransportType,
    ) {
        let is_routed = safety_route.is_some() || remote_private_route.is_some();

        // Record for node if this was not sent via a route
        if !is_routed {
            let wants_answer = matches!(rpc_kind, RPCKind::Question);
            let is_answer = matches!(rpc_kind, RPCKind::Answer);

            if is_answer {
                node_ref.stats_answer_sent(bytes, transport);
            } else {
                node_ref.stats_question_sent(send_ts, bytes, wants_answer, transport);
            }
            return;
        }

        // Ordering the send went out on
        let ordering = transport.sequence_ordering();

        // If safety route was used, record send there
        if let Some(sr) = &safety_route {
            sr.with_stats_mut(|s| s.record_sent(ordering, send_ts, bytes));
        }

        // If remote private route was used, record send there
        if let Some(pr) = &remote_private_route {
            pr.with_stats_mut(|s| s.record_sent(ordering, send_ts, bytes));
        }

        // Per-(target, outbound-SR) up-bytes. Covers all RPC kinds since this is the single
        // success choke point for outbound across Q/S/A. Target is the remote PR for routed-PR
        // sends, or the original target node for routed-direct sends (looked up via UnsafeRoutingInfo).
        if let Some(sr) = &safety_route {
            if let Some(pr) = &remote_private_route {
                pr.with_stats_mut(|s| s.record_routed_up(sr.route_key(), bytes));
            } else if let Some(target_node) = &destination_node_ref {
                target_node.stats_routed_up(sr.route_key().clone(), bytes);
            }
        }
    }

    /// Record answer received from node or route
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn record_answer_received(
        &self,
        recv_ts: Timestamp,
        bytes: ByteCount,
        context: &WaitableReplyContext,
    ) {
        // A completed round-trip is proof we're online.
        let opt_rd = if context.is_routed() {
            Some(RoutingDomain::PublicInternet)
        } else {
            context.node_ref.best_routing_domain()
        };
        if let Some(rd) = opt_rd {
            self.network_manager().online_detector().record_success(rd);
        }

        // Record stats for remote node if this was direct
        if context.opt_safety_route_ref.is_none()
            && context.opt_remote_private_route_ref.is_none()
            && context.opt_reply_private_route_ref.is_none()
        {
            if let Some(transport) = context.send_data_result.transport_type() {
                context
                    .node_ref
                    .stats_answer_rcvd(context.send_ts, recv_ts, bytes, transport);
            }
            return;
        }
        let routing_table = self.routing_table();

        // Ordering the send/answer went out/came in on; marks that ordering valid on the route.
        let opt_ordering = context.send_data_result.sequence_ordering();

        // Get latency for all local routes
        let mut total_local_latency = TimestampDuration::new(0u64);
        let total_latency: TimestampDuration = recv_ts.duration_since(context.send_ts);

        // Per-(target, reply-PR) round-trip + down-bytes. Key is the local route the answer
        // arrived on (reply-PR if set, else fall back to outbound SR pubkey for routed sends
        // without a distinct reply route). Target is the remote PR for routed-PR sends, or
        // the original target node for routed-direct sends.
        let inbound_key = context
            .opt_reply_private_route_ref
            .as_ref()
            .or(context.opt_safety_route_ref.as_ref())
            .map(|r| r.route_key().clone());
        if let Some(inbound_key) = inbound_key {
            if let Some(rpr) = &context.opt_remote_private_route_ref {
                rpr.with_stats_mut(|s| {
                    s.record_routed_round_trip(&inbound_key, total_latency, bytes)
                });
            } else {
                let opt_target = context
                    .destination
                    .get_unsafe_routing_info(&routing_table)
                    .and_then(|i| i.opt_node);
                if let Some(target_node) = opt_target {
                    target_node.stats_routed_round_trip(
                        inbound_key,
                        context.send_ts,
                        recv_ts,
                        bytes,
                    );
                }
            }
        }

        // If safety route was used, record route there
        if let Some(sr) = &context.opt_safety_route_ref {
            sr.with_stats_mut(|s| {
                // Record received bytes
                if let Some(ordering) = opt_ordering {
                    s.record_answer_received(ordering, recv_ts, bytes, &context.destination);
                }

                // If we used a safety route to send, use our last tested latency
                total_local_latency = total_local_latency.saturating_add(s.latency_stats().average);
            });
        }

        // If local private route was used, record route there
        // Skip if reply private route is the same as the safety route to avoid double-counting
        if let Some(pr) = &context.opt_reply_private_route_ref {
            if context.opt_safety_route_ref.as_ref().map(|r| r.route_key()) != Some(pr.route_key())
            {
                pr.with_stats_mut(|s| {
                    // Record received bytes
                    if let Some(ordering) = opt_ordering {
                        s.record_answer_received(ordering, recv_ts, bytes, &context.destination);
                    }

                    // If we used a private route to receive, use our last tested latency
                    total_local_latency =
                        total_local_latency.saturating_add(s.latency_stats().average);
                });
            }
        }

        // If remote private route was used, record there
        if let Some(rpr) = &context.opt_remote_private_route_ref {
            rpr.with_stats_mut(|s| {
                // Record received bytes
                if let Some(ordering) = opt_ordering {
                    s.record_answer_received(ordering, recv_ts, bytes, &context.destination);
                }

                // The remote route latency is recorded using the total latency minus the total local latency
                let remote_latency = total_latency.saturating_sub(total_local_latency);
                s.record_latency(remote_latency);
            });

            // If we sent to a private route without a safety route
            // We need to mark our own node info as having been seen so we can optimize sending it
            if let Err(e) = rpr.mark_seen_our_node_info() {
                veilid_log!(self debug "private route missing: {}", e);
            }

            // We can't record local route latency if a remote private route was used because
            // there is no way other than the prior latency estimation to determine how much time was spent
            // in the remote private route
            // Instead, we rely on local route testing to give us latency numbers for our local routes
        } else {
            // If no remote private route was used, then record half the total latency on our local routes
            // This is fine because if we sent with a local safety route,
            // then we must have received with a local private route too, per the design rules
            if let Some(sr) = &context.opt_safety_route_ref {
                sr.with_stats_mut(|s| s.record_latency(total_latency.div(2)));
            }
            // Skip if reply private route is the same as the safety route to avoid double-counting
            if let Some(pr) = &context.opt_reply_private_route_ref {
                if context.opt_safety_route_ref.as_ref().map(|r| r.route_key())
                    != Some(pr.route_key())
                {
                    pr.with_stats_mut(|s| s.record_latency(total_latency.div(2)));
                }
            }
        }
    }

    /// Record question or statement received from node or route
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn record_question_received(&self, msg: &Message) {
        let recv_ts = msg.header.timestamp;
        let bytes = msg.header.body_len;

        // Inbound traffic is proof we're online.
        self.network_manager()
            .online_detector()
            .record_success(msg.header.routing_domain());

        // Ordering this message arrived on (marks that ordering valid on the receiving route).
        let ordering = msg
            .header
            .detail
            .flow()
            .transport_type()
            .sequence_ordering();

        // Per-sender BucketEntry aggregate (if sender identity is known).
        if let Some(sender_nr) = msg.opt_sender_nr.clone() {
            let transport = msg.header.detail.flow().transport_type();
            sender_nr.stats_question_rcvd(recv_ts, bytes, transport);
        }

        let routing_table = self.routing_table();
        let rss = routing_table.route_spec_store();

        // Process messages based on how they were received
        match &msg.header.detail {
            RPCMessageHeaderDetail::Direct(_) => {}
            // Process messages that arrived with no private route (private route stub)
            RPCMessageHeaderDetail::SafetyRouted(d) => {
                // This may record nothing if the remote safety route is not also
                // a remote private route that been imported, but that's okay
                rss.update_allocated_route_stats(recv_ts, &d.remote_safety_route, |s| {
                    s.record_question_received(ordering, recv_ts, bytes);
                    Ok(())
                });
            }
            // Process messages that arrived to our private route
            RPCMessageHeaderDetail::PrivateRouted(d) => {
                // This may record nothing if the remote safety route is not also
                // a remote private route that been imported, but that's okay
                // it could also be a node id if no remote safety route was used
                // in which case this also will do nothing
                rss.update_remote_route_stats(recv_ts, &d.remote_safety_route, |s| {
                    s.record_question_received(ordering, recv_ts, bytes);
                    // Per-pair down-bytes keyed by our local PR.
                    s.record_routed_down(&d.private_route, bytes);
                    Ok(())
                });

                // Record for our local private route we received over
                rss.update_allocated_route_stats(recv_ts, &d.private_route, |s| {
                    s.record_question_received(ordering, recv_ts, bytes);
                    Ok(())
                });
            }
        }
    }

    /// Record a statement received from node or route. Mirror of `record_question_received`.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn record_statement_received(&self, msg: &Message) {
        let recv_ts = msg.header.timestamp;
        let bytes = msg.header.body_len;

        // Inbound traffic is proof we're online.
        self.network_manager()
            .online_detector()
            .record_success(msg.header.routing_domain());

        // Ordering this message arrived on (marks that ordering valid on the receiving route).
        let ordering = msg
            .header
            .detail
            .flow()
            .transport_type()
            .sequence_ordering();

        if let Some(sender_nr) = msg.opt_sender_nr.clone() {
            let transport = msg.header.detail.flow().transport_type();
            sender_nr.stats_question_rcvd(recv_ts, bytes, transport);
        }

        let routing_table = self.routing_table();
        let rss = routing_table.route_spec_store();

        match &msg.header.detail {
            RPCMessageHeaderDetail::Direct(_) => {}
            RPCMessageHeaderDetail::SafetyRouted(d) => {
                rss.update_allocated_route_stats(recv_ts, &d.remote_safety_route, |s| {
                    s.record_question_received(ordering, recv_ts, bytes);
                    Ok(())
                });
            }
            RPCMessageHeaderDetail::PrivateRouted(d) => {
                rss.update_remote_route_stats(recv_ts, &d.remote_safety_route, |s| {
                    s.record_question_received(ordering, recv_ts, bytes);
                    s.record_routed_down(&d.private_route, bytes);
                    Ok(())
                });
                rss.update_allocated_route_stats(recv_ts, &d.private_route, |s| {
                    s.record_question_received(ordering, recv_ts, bytes);
                    Ok(())
                });
            }
        }
    }

    /// Get the default timeout for RPCs sent to a via a specific safety selection
    pub fn get_safety_selection_timeout(
        &self,
        safety_selection: &SafetySelection,
    ) -> TimestampDuration {
        // For both safety routes to direct destinations, and to private routes, the effective hop count is
        // twice that of the safety selection's choice, either because the private route doubles the hop count (probably)
        // or because we manually double it when contacting non-private destinations
        let effective_hops = safety_selection.get_hop_count() * 2;

        // Timeout is the default timeout plus 500ms (max single hop latency) times the excess hop count
        // Excess hops count is what could possibly fix in the 3sec -> 5sec padding range. Better calculations
        // using actual routing table statistics should be able to optimize this with a more accurate estimate.
        let excess_hops = effective_hops.saturating_sub(4); // 500ms * 4 = 2000ms,
        self.timeout
            .saturating_add(TimestampDuration::new_ms(500).saturating_mul(excess_hops as u64))
        // Effectively for all routed communications under 4 hops total, the timeout will be the default timeout for now
    }

    /// Get the default timeout for RPCs sent to a specific destination
    pub fn get_destination_timeout(&self, dest: &Destination) -> TimestampDuration {
        self.get_safety_selection_timeout(&dest.get_safety_selection())
    }

    /// Issue a question over the network. Returns `Sent` on a successful send,
    /// `Failed` carrying the SendDataResult when the send attempted but didn't reach
    /// a Value, or `NotSent` if we never reached send_envelope.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn question(
        &self,
        dest: Destination,
        question: RPCQuestion,
        opt_signer_keypair: Option<KeyPair>,
        context: Option<QuestionContext>,
    ) -> Result<QuestionOutcome, RPCError> {
        // Get sender peer info if we should send that
        let spi = self.get_sender_peer_info(&dest);

        // Wrap question in operation
        let operation = RPCOperation::new_question(question, spi);
        let op_id = operation.op_id();

        // Get signing parameters
        let signing_params = match opt_signer_keypair {
            Some(signer_keypair) => Some(RPCSigningParams {
                signer_keypair,
                destination_key: dest.destination_key()?,
            }),
            None => None,
        };
        let signer = signing_params.as_ref().map(|x| x.signer_keypair.key());

        // Log rpc send
        veilid_log!(self debug target: "rpc_message", fields: dir = "send", kind = "question", op_id = op_id.as_u64(), desc = operation.kind().desc(), ?dest, ?signer );

        // Produce rendered operation
        let rendered = Box::pin(
            self.render_signed_operation(dest.clone(), operation, signing_params)
                .measure_debug(
                    TimestampDuration::new_ms(200),
                    veilid_log_dbg!(self, "RPCProcessor::question render_signed_operation"),
                ),
        )
        .await?;
        let RenderedOperation {
            outer_op_id,
            message,
            node_ref,
            opt_dial_info,
            opt_safety_route_ref,
            opt_remote_private_route_ref,
            opt_reply_private_route_ref,
        } = match rendered {
            NetworkResult::Value(v) => v,
            other => return Ok(QuestionOutcome::NotSent(other.map(|_| ()))),
        };

        // Calculate answer timeout
        // XXX: Move into RenderedOperation
        let timeout = self.get_destination_timeout(&dest);

        // True target node ref for routed-direct stats
        // XXX: Move into RenderedOperation
        let destination_node_ref = if let Some(UnsafeRoutingInfo {
            opt_node,
            opt_routing_domain,
        }) = dest.get_unsafe_routing_info(&self.routing_table())
        {
            // Shortcut if there's no routing domain possible for this destination
            if opt_routing_domain.is_none() {
                if let Some(node) = &opt_node {
                    self.record_unreachable(node.clone());
                }
                return Ok(QuestionOutcome::NotSent(
                    NetworkResult::no_connection_other("no routing domain"),
                ));
            }
            opt_node
        } else {
            None
        };

        // Set up op id eventual
        let handle = self
            .waiting_rpc_table
            .add_op_waiter(op_id, context.map(Arc::new));

        // Send question
        let bytes: ByteCount = (message.len() as u64).into();
        #[allow(unused_variables)]
        let message_len = message.len();
        let send_data_result = self
            .network_manager()
            .send_envelope(node_ref.clone(), opt_dial_info.clone(), message)
            .await
            .map_err(|e| {
                // Envelope-level error: no transport was attempted.
                let send_ts = Timestamp::now();
                self.record_send_failure(
                    RPCKind::Question,
                    send_ts,
                    node_ref.clone(),
                    None,
                    opt_safety_route_ref.clone(),
                    opt_remote_private_route_ref.clone(),
                );
                RPCError::network(e)
            })?;
        // Take send timestamp -after- send is attempted to exclude TCP connection time which
        // may unfairly punish some nodes, randomly, based on their being in the connection table or not
        let send_ts = Timestamp::now();
        if !matches!(send_data_result.network_result(), NetworkResult::Value(_)) {
            veilid_log!(self debug target: "network_result", "{} at {}:{}: outer_op_id = {}, node_ref={}, opt_relay_di={}, message.len={}",
                send_data_result.network_result(), file!(), line!(),
                outer_op_id, node_ref, opt_dial_info.as_ref().map(|x| x.to_string()).unwrap_or_else(String::new), message_len);
            self.record_send_failure(
                RPCKind::Question,
                send_ts,
                node_ref.clone(),
                send_data_result.opt_transport_type(),
                opt_safety_route_ref,
                opt_remote_private_route_ref,
            );
            return Ok(QuestionOutcome::Failed(send_data_result));
        }

        // Successfully sent
        if let Some(transport) = send_data_result.transport_type() {
            self.record_send_success(
                RPCKind::Question,
                send_ts,
                bytes,
                node_ref.clone(),
                destination_node_ref,
                opt_safety_route_ref.clone(),
                opt_remote_private_route_ref.clone(),
                transport,
            );
        }

        // Ref the connection so it doesn't go away until we're done with the waitable reply
        let opt_connection_ref_scope = send_data_result
            .unique_flow()
            .and_then(|uf| uf.connection_id)
            .and_then(|id| {
                self.network_manager()
                    .connection_manager()
                    .try_connection_ref_scope(id)
            });

        // Pass back waitable reply completion
        Ok(QuestionOutcome::Sent(Box::new(WaitableReply::new(
            handle,
            opt_connection_ref_scope,
            WaitableReplyContext {
                timeout,
                node_ref: node_ref.clone(),
                send_ts,
                send_data_result,
                opt_safety_route_ref,
                opt_remote_private_route_ref,
                opt_reply_private_route_ref,
                destination: dest,
            },
        ))))
    }

    /// Issue a statement over the network, possibly using an anonymized route
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn statement(
        &self,
        dest: Destination,
        statement: RPCStatement,
        opt_signer_keypair: Option<KeyPair>,
        route_op_id: Option<OperationId>,
    ) -> RPCNetworkResult<()> {
        // Get sender peer info if we should send that
        let spi = self.get_sender_peer_info(&dest);

        // Wrap statement in operation
        let operation = RPCOperation::new_statement(statement, spi);

        // Get signing parameters
        let signing_params = match opt_signer_keypair {
            Some(signer_keypair) => Some(RPCSigningParams {
                signer_keypair,
                destination_key: dest.destination_key()?,
            }),
            None => None,
        };
        let signer = signing_params.as_ref().map(|x| x.signer_keypair.key());

        // Log rpc send
        veilid_log!(self debug target: "rpc_message", fields: dir = "send", kind = "statement", ?route_op_id, op_id = operation.op_id().as_u64(), desc = operation.kind().desc(), ?dest, ?signer);

        // Produce rendered operation
        let RenderedOperation {
            outer_op_id,
            message,
            node_ref,
            opt_dial_info,
            opt_safety_route_ref: safety_route,
            opt_remote_private_route_ref: remote_private_route,
            // Held until end of scope; statements are one-shot.
            opt_reply_private_route_ref: _reply_private_route,
        } = network_result_try!(
            Box::pin(
                self.render_signed_operation(dest.clone(), operation, signing_params)
                    .measure_debug(
                        TimestampDuration::new_ms(200),
                        veilid_log_dbg!(self, "RPCProcessor::statement render_signed_operation"),
                    )
            )
            .await?
        );

        // True target node ref for routed-direct stats
        // XXX: Move into RenderedOperation
        let destination_node_ref = if let Some(UnsafeRoutingInfo {
            opt_node,
            opt_routing_domain,
        }) = dest.get_unsafe_routing_info(&self.routing_table())
        {
            // Shortcut if there's no routing domain possible for this destination
            if opt_routing_domain.is_none() {
                if let Some(node) = &opt_node {
                    self.record_unreachable(node.clone());
                }
                return Ok(NetworkResult::no_connection_other("no routing domain"));
            }
            opt_node
        } else {
            None
        };

        // Send statement
        let bytes: ByteCount = (message.len() as u64).into();
        #[allow(unused_variables)]
        let message_len = message.len();
        let send_data_result = self
            .network_manager()
            .send_envelope(node_ref.clone(), opt_dial_info.clone(), message)
            .await
            .map_err(|e| {
                // Envelope-level error: no transport was attempted.
                let send_ts = Timestamp::now();
                self.record_send_failure(
                    RPCKind::Statement,
                    send_ts,
                    node_ref.clone(),
                    None,
                    safety_route.clone(),
                    remote_private_route.clone(),
                );
                RPCError::network(e)
            })?;
        // Take send timestamp -after- send is attempted to exclude TCP connection time which
        // may unfairly punish some nodes, randomly, based on their being in the connection table or not
        let send_ts = Timestamp::now();
        if !matches!(send_data_result.network_result(), NetworkResult::Value(_)) {
            veilid_log!(self debug target: "network_result", "{} at {}:{}: outer_op_id = {}, node_ref={}, opt_relay_di={}, message.len={}",
                send_data_result.network_result(), file!(), line!(),
                outer_op_id, node_ref, opt_dial_info.as_ref().map(|x| x.to_string()).unwrap_or_else(String::new), message_len);
            self.record_send_failure(
                RPCKind::Statement,
                send_ts,
                node_ref.clone(),
                send_data_result.opt_transport_type(),
                safety_route.clone(),
                remote_private_route.clone(),
            );
            network_result_raise!(send_data_result.into_network_result());
        }

        // Successfully sent
        if let Some(transport) = send_data_result.transport_type() {
            self.record_send_success(
                RPCKind::Statement,
                send_ts,
                bytes,
                node_ref.clone(),
                destination_node_ref,
                safety_route,
                remote_private_route,
                transport,
            );
        }

        Ok(NetworkResult::value(()))
    }
    /// Issue a reply over the network, possibly using an anonymized route
    /// The request must want a response, or this routine fails
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn answer(
        &self,
        request: Message,
        answer: RPCAnswer,
        opt_signer_keypair: Option<KeyPair>,
    ) -> RPCNetworkResult<()> {
        // Extract destination from respond_to
        let dest = network_result_try!(self.get_respond_to_destination(&request));

        // Get sender signed node info if we should send that
        let spi = self.get_sender_peer_info(&dest);

        // Wrap answer in operation
        let operation = RPCOperation::new_answer(request.operation.op_id(), answer, spi);

        // Get signing parameters
        let signing_params = match opt_signer_keypair {
            Some(signer_keypair) => Some(RPCSigningParams {
                signer_keypair,
                destination_key: dest.destination_key()?,
            }),
            None => None,
        };
        let signer = signing_params.as_ref().map(|x| x.signer_keypair.key());

        // Log rpc send
        veilid_log!(self debug target: "rpc_message", fields: dir = "send", kind = "answer", op_id = operation.op_id().as_u64(), desc = operation.kind().desc(), ?dest, ?signer);

        // Produce rendered operation
        let RenderedOperation {
            outer_op_id,
            message,
            node_ref,
            opt_dial_info,
            opt_safety_route_ref: safety_route,
            opt_remote_private_route_ref: remote_private_route,
            // Held until end of scope; answers are one-shot server-side.
            opt_reply_private_route_ref: _reply_private_route,
        } = network_result_try!(
            Box::pin(
                self.render_signed_operation(dest.clone(), operation, signing_params)
                    .measure_debug(
                        TimestampDuration::new_ms(200),
                        veilid_log_dbg!(self, "RPCProcessor::answer render_signed_operation"),
                    )
            )
            .await?
        );

        // True target node ref for routed-direct stats
        // XXX: Move into RenderedOperation
        let destination_node_ref = if let Some(UnsafeRoutingInfo {
            opt_node,
            opt_routing_domain,
        }) = dest.get_unsafe_routing_info(&self.routing_table())
        {
            // Shortcut if there's no routing domain possible for this destination
            if opt_routing_domain.is_none() {
                if let Some(node) = &opt_node {
                    self.record_unreachable(node.clone());
                }
                return Ok(NetworkResult::no_connection_other("no routing domain"));
            }
            opt_node
        } else {
            None
        };

        // Send the reply
        let bytes: ByteCount = (message.len() as u64).into();
        #[allow(unused_variables)]
        let message_len = message.len();
        let send_data_result = self
            .network_manager()
            .send_envelope(node_ref.clone(), opt_dial_info.clone(), message)
            .await
            .map_err(|e| {
                // Envelope-level error: no transport was attempted.
                let send_ts = Timestamp::now();
                self.record_send_failure(
                    RPCKind::Answer,
                    send_ts,
                    node_ref.clone(),
                    None,
                    safety_route.clone(),
                    remote_private_route.clone(),
                );
                RPCError::network(e)
            })?;
        // Take send timestamp -after- send is attempted to exclude TCP connection time which
        // may unfairly punish some nodes, randomly, based on their being in the connection table or not
        let send_ts = Timestamp::now();
        if !matches!(send_data_result.network_result(), NetworkResult::Value(_)) {
            veilid_log!(self debug target: "network_result", "{} at {}:{}: outer_op_id = {}, node_ref={}, opt_relay_di={}, message.len={}",
                send_data_result.network_result(), file!(), line!(),
                outer_op_id, node_ref, opt_dial_info.as_ref().map(|x| x.to_string()).unwrap_or_else(String::new), message_len);
            self.record_send_failure(
                RPCKind::Answer,
                send_ts,
                node_ref.clone(),
                send_data_result.opt_transport_type(),
                safety_route.clone(),
                remote_private_route.clone(),
            );
            network_result_raise!(send_data_result.into_network_result());
        }

        // Reply successfully sent
        if let Some(transport) = send_data_result.transport_type() {
            self.record_send_success(
                RPCKind::Answer,
                send_ts,
                bytes,
                node_ref.clone(),
                destination_node_ref,
                safety_route,
                remote_private_route,
                transport,
            );
        }

        Ok(NetworkResult::value(()))
    }

    /// Decoding RPC from the wire
    /// This performs a capnp decode on the data, and if it passes the capnp schema
    /// it performs the signature and cryptographic validation required to pass the operation up for processing
    /// Returns the operation and its signing key if provided
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn decode_rpc_operation(
        &self,
        encoded_msg: MessageEncoded,
    ) -> Result<(RPCOperation, Option<PublicKey>, MessageEncoded), RPCError> {
        let decode_context = RPCDecodeContext {
            registry: self.registry(),
            origin_routing_domain: encoded_msg.header.routing_domain(),
        };

        let signed_operation = {
            let reader = encoded_msg.data.get_reader()?;
            let op_reader = reader.get_root::<veilid_capnp::signed_operation::Reader>()?;
            RPCSignedOperation::decode(&decode_context, &op_reader)?
        };

        // Validate the RPC message
        self.validate_rpc_signed_operation(&signed_operation, &encoded_msg)
            .await?;
        let opt_signer = signed_operation
            .signer_signature()
            .map(|x| x.signer.clone());

        // Decode the operation itself
        let operation = signed_operation.decode_operation(&decode_context)?;

        // Validate the operation
        self.validate_rpc_operation(&operation).await?;

        Ok((operation, opt_signer, encoded_msg))
    }

    /// Validation of RPC signature if provided
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn validate_rpc_signed_operation(
        &self,
        signed_operation: &RPCSignedOperation,
        encoded_msg: &MessageEncoded,
    ) -> Result<(), RPCError> {
        // Validate the RPC signed operation
        let destination_key = match &encoded_msg.header.detail {
            RPCMessageHeaderDetail::Direct(d) => {
                let routing_table = self.routing_table();
                routing_table.public_key(d.envelope.get_crypto_kind())
            }
            RPCMessageHeaderDetail::SafetyRouted(s) => {
                let routing_table = self.routing_table();
                routing_table.public_key(s.direct.envelope.get_crypto_kind())
            }
            RPCMessageHeaderDetail::PrivateRouted(p) => p.private_route.clone(),
        };

        let crypto = self.crypto();
        signed_operation.validate(&crypto, destination_key).await?;

        Ok(())
    }

    /// Cryptographic RPC validation and sanitization
    ///
    /// This code may modify the RPC operation to remove elements that are inappropriate for this node
    /// or reject the RPC operation entirely. For example, PeerInfo in fanout peer lists may be
    /// removed if they are deemed inappropriate for this node, without rejecting the entire operation.
    ///
    /// We do this as part of the RPC network layer to ensure that any RPC operations that are
    /// processed have already been validated cryptographically and it is not the job of the
    /// caller or receiver. This does not mean the operation is 'semantically correct'. For
    /// complex operations that require stateful validation and a more robust context than
    /// 'signatures', the caller must still perform whatever validation is necessary
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn validate_rpc_operation(&self, operation: &RPCOperation) -> Result<(), RPCError> {
        // If this is an answer, get the question context for this answer
        // If we received an answer for a question we did not ask, this will return an error
        let question_context = if let RPCOperationKind::Answer(_) = operation.kind() {
            let op_id = operation.op_id();
            self.waiting_rpc_table.get_op_context(op_id)?
        } else {
            None
        };

        // Validate the RPC operation
        let validate_context = RPCValidateContext {
            registry: self.registry(),
            question_context: question_context.as_ref().map(|x| x.as_ref()),
        };
        operation.validate(&validate_context).await
    }

    //////////////////////////////////////////////////////////////////////
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn process_rpc_message(&self, encoded_msg: MessageEncoded) -> RPCNetworkResult<()> {
        let start_ts = Timestamp::now();

        // Decode operation appropriately based on header detail
        let msg = match &encoded_msg.header.detail {
            RPCMessageHeaderDetail::Direct(detail) => {
                // Get sender node id
                let sender_node_id = detail.envelope.get_sender_id();

                // Get the routing domain this message came over
                let origin_routing_domain = detail.routing_domain;

                // Decode and validate the RPC operation
                let decode_res = self
                    .decode_rpc_operation(encoded_msg)
                    .measure_debug(TimestampDuration::new_ms(200), |dur| {
                        veilid_log!(self debug "decode_rpc_operation Direct: {:#}", dur);
                    })
                    .await;
                let (operation, opt_signer, encoded_msg) = match decode_res {
                    Ok(v) => v,
                    Err(e) => {
                        match e {
                            // Invalid messages that should be punished
                            RPCError::Protocol(_) | RPCError::InvalidFormat(_) => {
                                veilid_log!(self debug "Invalid RPC Operation: {}", e);

                                // Punish nodes that send direct undecodable crap
                                self.network_manager().address_filter().punish_node_id(
                                    sender_node_id,
                                    PunishmentReason::FailedToDecodeOperation,
                                );
                            }
                            // Ignored messages that should be dropped
                            RPCError::Ignore(_) | RPCError::Network(_) | RPCError::TryAgain(_) => {
                                veilid_log!(self trace "Dropping RPC Operation: {}", e);
                            }
                            // Internal errors that deserve louder logging
                            RPCError::Unimplemented(_) | RPCError::Internal(_) => {
                                veilid_log!(self error "Error decoding RPC operation: {}", e);
                            }
                        };
                        return Ok(NetworkResult::invalid_message(e));
                    }
                };

                // Get the sender noderef, incorporating sender's peer info
                let sender_peer_info = operation.sender_peer_info();
                let mut opt_sender_nr: Option<NodeRef> = network_result_try!(self
                .process_sender_peer_info(sender_node_id.clone(), &sender_peer_info)? => {
                    veilid_log!(self debug target:"network_result", "Sender PeerInfo: {:?}", sender_peer_info);
                    veilid_log!(self debug target:"network_result", "From Operation: {:?}", operation.kind());
                    veilid_log!(self debug target:"network_result", "With Detail: {:?}", encoded_msg.header.detail);
                });
                // look up sender node, in case it's different than our peer due to relaying
                if opt_sender_nr.is_none() {
                    opt_sender_nr = match self.routing_table().lookup_node_id(sender_node_id) {
                        Ok(v) => v,
                        Err(e) => {
                            // If this fails it's not the other node's fault. We should be able to look up a
                            // node ref for a registered sender node id that just sent a message to us
                            // because it is registered with an existing connection at the very least.
                            return Ok(NetworkResult::no_connection_other(e));
                        }
                    }
                }

                // Update the 'seen our node info' timestamp to determine if this node needs a
                // 'node info update' ping
                if let Some(sender_nr) = &opt_sender_nr {
                    let new_ts = sender_peer_info.target_node_info_ts();
                    if let Some(old_ts) =
                        sender_nr.set_seen_our_node_info_ts(origin_routing_domain, new_ts)
                    {
                        veilid_log!(self debug target: "network_result", "Setting seen_our_node_info_ts on {}: {} -> {}", sender_nr, old_ts, new_ts );
                    }
                }

                // Make the RPC message
                Message {
                    header: encoded_msg.header,
                    operation,
                    opt_signer,
                    opt_sender_nr,
                }
            }
            RPCMessageHeaderDetail::SafetyRouted(_) | RPCMessageHeaderDetail::PrivateRouted(_) => {
                // Decode and validate the RPC operation
                let (operation, opt_signer, encoded_msg) = match self
                    .decode_rpc_operation(encoded_msg)
                    .measure_debug(TimestampDuration::new_ms(200), |dur| {
                        veilid_log!(self debug "decode_rpc_operation Routed: {}", dur);
                    })
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        match e {
                            // Invalid messages that should be punished
                            RPCError::Protocol(_) | RPCError::InvalidFormat(_) => {
                                veilid_log!(self debug "Invalid routed RPC Operation: {}", e);

                                // XXX: Punish routes that send routed undecodable crap
                                // self.network_manager().address_filter().punish_route_id(xxx, PunishmentReason::FailedToDecodeRoutedMessage);
                            }
                            // Ignored messages that should be dropped
                            RPCError::Ignore(_) | RPCError::Network(_) | RPCError::TryAgain(_) => {
                                veilid_log!(self trace "Dropping routed RPC Operation: {}", e);
                            }
                            // Internal errors that deserve louder logging
                            RPCError::Unimplemented(_) | RPCError::Internal(_) => {
                                veilid_log!(self error "Error decoding routed RPC operation: {}", e);
                            }
                        };

                        return Ok(NetworkResult::invalid_message(e));
                    }
                };

                // Make the RPC message
                Message {
                    header: encoded_msg.header,
                    operation,
                    opt_signer,
                    opt_sender_nr: None,
                }
            }
        };

        let desc = msg.operation.kind().desc();
        let signer = msg.opt_signer.as_ref();
        let route_op_id = match &msg.header.detail {
            RPCMessageHeaderDetail::Direct(_) => None,
            RPCMessageHeaderDetail::SafetyRouted(s) => Some(s.route_op_id),
            RPCMessageHeaderDetail::PrivateRouted(p) => Some(p.route_op_id),
        };

        // Process stats for questions/statements received
        match msg.operation.kind() {
            RPCOperationKind::Question(_) => {
                self.record_question_received(&msg);

                // Log rpc receive
                veilid_log!(self debug target: "rpc_message", fields: dir = "recv", kind = "question", ?route_op_id, op_id = msg.operation.op_id().as_u64(), desc, header = ?msg.header, operation = ?msg.operation, signer = ?signer);
            }
            RPCOperationKind::Statement(_) => {
                self.record_statement_received(&msg);

                // Log rpc receive
                veilid_log!(self debug target: "rpc_message", fields: dir = "recv", kind = "statement", ?route_op_id, op_id = msg.operation.op_id().as_u64(), desc, header = ?msg.header, operation = ?msg.operation, signer = ?signer);
            }
            RPCOperationKind::Answer(_) => {
                // Answer stats are processed in wait_for_reply

                // Log rpc receive
                veilid_log!(self debug target: "rpc_message", fields: dir = "recv", kind = "answer", ?route_op_id, op_id = msg.operation.op_id().as_u64(), desc, header = ?msg.header, operation = ?msg.operation, signer = ?signer);
            }
        };

        // Process specific message kind
        let out = match msg.operation.kind() {
            RPCOperationKind::Question(q) => match q.detail() {
                RPCQuestionDetail::StatusQ(_) => {
                    pin_dyn_future_closure!(self.process_status_q(msg)).await
                }
                RPCQuestionDetail::FindNodeQ(_) => {
                    pin_dyn_future_closure!(self.process_find_node_q(msg)).await
                }
                RPCQuestionDetail::AppCallQ(_) => {
                    pin_dyn_future_closure!(self.process_app_call_q(msg)).await
                }
                RPCQuestionDetail::GetValueQ(_) => {
                    pin_dyn_future_closure!(self.process_get_value_q(msg)).await
                }
                RPCQuestionDetail::SetValueQ(_) => {
                    pin_dyn_future_closure!(self.process_set_value_q(msg)).await
                }
                RPCQuestionDetail::WatchValueQ(_) => {
                    pin_dyn_future_closure!(self.process_watch_value_q(msg)).await
                }
                RPCQuestionDetail::InspectValueQ(_) => {
                    pin_dyn_future_closure!(self.process_inspect_value_q(msg)).await
                }
                RPCQuestionDetail::TransactBeginQ(_) => {
                    pin_dyn_future_closure!(self.process_transact_begin_q(msg)).await
                }
                RPCQuestionDetail::TransactCommandQ(_) => {
                    pin_dyn_future_closure!(self.process_transact_command_q(msg)).await
                }
                #[cfg(feature = "unstable-blockstore")]
                RPCQuestionDetail::SupplyBlockQ(_) => {
                    pin_dyn_future_closure!(self.process_supply_block_q(msg)).await
                }
                #[cfg(feature = "unstable-blockstore")]
                RPCQuestionDetail::FindBlockQ(_) => {
                    pin_dyn_future_closure!(self.process_find_block_q(msg)).await
                }
                #[cfg(feature = "unstable-tunnels")]
                RPCQuestionDetail::StartTunnelQ(_) => {
                    pin_dyn_future_closure!(self.process_start_tunnel_q(msg)).await
                }
                #[cfg(feature = "unstable-tunnels")]
                RPCQuestionDetail::CompleteTunnelQ(_) => {
                    pin_dyn_future_closure!(self.process_complete_tunnel_q(msg)).await
                }
                #[cfg(feature = "unstable-tunnels")]
                RPCQuestionDetail::CancelTunnelQ(_) => {
                    pin_dyn_future_closure!(self.process_cancel_tunnel_q(msg)).await
                }
            },
            RPCOperationKind::Statement(s) => match s.detail() {
                RPCStatementDetail::ValidateDialInfo(_) => {
                    pin_dyn_future_closure!(self.process_validate_dial_info(msg)).await
                }
                RPCStatementDetail::Route(_) => {
                    pin_dyn_future_closure!(self.process_route(msg)).await
                }
                RPCStatementDetail::ValueChanged(_) => {
                    pin_dyn_future_closure!(self.process_value_changed(msg)).await
                }
                RPCStatementDetail::Signal(_) => {
                    pin_dyn_future_closure!(self.process_signal(msg)).await
                }
                RPCStatementDetail::ReturnReceipt(_) => {
                    pin_dyn_future_closure!(self.process_return_receipt(msg)).await
                }
                RPCStatementDetail::AppMessage(_) => self.process_app_message(msg),
            },
            RPCOperationKind::Answer(_) => {
                let op_id = msg.operation.op_id();
                if let Err(e) = self.waiting_rpc_table.complete_op_waiter(op_id, msg) {
                    match e {
                        RPCError::Unimplemented(_) | RPCError::Internal(_) => {
                            veilid_log!(self error "Error in RPC operation: id = {}: {}", op_id, e);
                        }
                        RPCError::InvalidFormat(_)
                        | RPCError::Protocol(_)
                        | RPCError::Network(_)
                        | RPCError::TryAgain(_) => {
                            veilid_log!(self debug "Could not complete RPC operation: id = {}: {}", op_id, e);
                        }
                        RPCError::Ignore(e) => {
                            veilid_log!(self debug "RPC operation ignored: id = {}: {}", op_id, e);
                        }
                    };
                    // Don't throw an error here because it's okay if the original operation timed out
                }
                Ok(NetworkResult::value(()))
            }
        };

        // Log per-kind latency
        {
            let latency = TimestampDuration::since(start_ts);
            let mut inner = self.inner.lock();
            let entry = inner
                .rpc_worker_process_latency_and_accounting_by_operation_kind
                .entry(desc)
                .or_default();

            entry.0 = entry.1.record_latency(latency);
        }
        out
    }
}
