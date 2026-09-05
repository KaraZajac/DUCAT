use futures_util::future::select_all;
use futures_util::stream::{FuturesUnordered, StreamExt};
use stop_token::future::FutureExt as _;

use super::*;
use outbound_transaction_keepalive_deadlines::*;

impl_veilid_log_facility!("stor");

/// Parameters for registering a keepalive
/// Excludes the transaction handle and exiration timestamp as these are expected to change over time
#[derive(Debug)]
pub(in crate::storage_manager) struct OutboundTransactionKeepaliveParams {
    pub lnxid: LocalNodeTransactionId,
    pub opaque_record_key: OpaqueRecordKey,
    pub rnxid: RemoteNodeTransactionId,
    pub dest: Destination,
    pub descriptor: Arc<SignedValueDescriptor>,
}

/// Keepalive processor commands
#[derive(Debug, Clone)]
enum OutboundTransactionKeepaliveCommand {
    AddKeepalive {
        keepalive: OutboundTransactionKeepalive,
        send_ts: Timestamp,
    },
}

/// Result returned from a keepalive processor future
#[derive(Debug, Clone)]
enum ProcessorResult {
    CommandReceived {
        command: OutboundTransactionKeepaliveCommand,
    },
    DeadlineReached,
    KeepaliveResult {
        keepalive: OutboundTransactionKeepalive,
        opt_expiration: Option<Timestamp>,
        opt_rtt: Option<TimestampDuration>,
    },
    Failed {
        error: String,
    },
}

/// Processor object with shared state for synchronous register/unregister
/// and async keepalive processing
pub(in crate::storage_manager) struct OutboundTransactionKeepaliveProcessor {
    registry: VeilidComponentRegistry,
    deadlines: Arc<Mutex<OutboundTransactionKeepaliveDeadlines>>,
    sender: flume::Sender<OutboundTransactionKeepaliveCommand>,
    receiver: flume::Receiver<OutboundTransactionKeepaliveCommand>,
    jh: Mutex<Option<MustJoinHandle<()>>>,
    stop_source: Mutex<Option<StopSource>>,
}

impl VeilidComponentRegistryAccessor for OutboundTransactionKeepaliveProcessor {
    fn registry(&self) -> VeilidComponentRegistry {
        self.registry.clone()
    }
}

impl fmt::Debug for OutboundTransactionKeepaliveProcessor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutboundTransactionKeepaliveProcessor")
            .field("deadlines", &self.deadlines)
            .field("sender", &self.sender)
            .field("receiver", &self.receiver)
            .field("jh", &self.jh)
            .field("stop_source", &self.stop_source)
            .finish()
    }
}

impl OutboundTransactionKeepaliveProcessor {
    /// Create a new processor. Call `init` to start the background loop.
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        let ttl =
            TimestampDuration::new_ms(registry.config().internal().network.rpc.timeout_ms as u64)
                .saturating_mul(TRANSACTION_TIMEOUT_RPC_MULTIPLIER);
        let (sender, receiver) = flume::unbounded();
        Self {
            deadlines: Arc::new(Mutex::new(OutboundTransactionKeepaliveDeadlines::new(
                registry.clone(),
                ttl,
            ))),
            registry,
            sender,
            receiver,
            jh: Mutex::new(None),
            stop_source: Mutex::new(None),
        }
    }

    /// Register a new keepalive train for one node transaction.
    /// The first keepalive fires using the TTL-driven schedule against the
    /// initial server-reported expiration. If the registration succeeds,
    /// this returns a stop source that can be used to cancel the keepalive at any point by dropping it.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub fn register(&self, params: OutboundTransactionKeepaliveParams, expiration_ts: Timestamp) {
        let cur_ts = Timestamp::now();

        if expiration_ts <= cur_ts {
            #[cfg(feature = "verbose-tracing")]
            veilid_log!(self debug target:"dht", "Keepalive register: node already expired: lnxid={} record={} rnxid={}",
                params.lnxid, params.opaque_record_key, params.rnxid);
            return;
        }

        let params = Arc::new(params);
        let state = Arc::new(Mutex::new(OutboundTransactionKeepaliveState {
            opt_last_expiration: Some(expiration_ts),
            ..Default::default()
        }));

        let keepalive = OutboundTransactionKeepalive::new(params, state);

        #[cfg(feature = "verbose-tracing")]
        veilid_log!(self debug target:"dht", "Keepalive node registered: keepalive={:?} expiry={} ",
            keepalive, expiration_ts.duration_since(cur_ts));

        let _ = self
            .sender
            .send(OutboundTransactionKeepaliveCommand::AddKeepalive {
                send_ts: cur_ts,
                keepalive,
            });
    }

    /// Cancel scheduled keepalives for a node transaction. Marks it cancelled so
    /// any in-flight keepalive that completes will not reschedule a successor.
    /// Call this before sending End RPCs.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(self))
    )]
    pub fn unregister(&self, lnxid: LocalNodeTransactionId) {
        self.deadlines.lock().cancel_deadline(lnxid);
    }

    /// Spawn the background processor loop. Must be called before register.
    pub fn init(&self) {
        let rx = self.receiver.clone();
        let stop_source = StopSource::new();
        let stop_token = stop_source.token();
        let jh = spawn(
            "outbound transaction keepalive processor",
            Self::processor_loop(self.registry(), stop_token, rx, self.deadlines.clone()),
        );
        *self.jh.lock() = Some(jh);
        *self.stop_source.lock() = Some(stop_source);
    }

    /// Signal the background loop to stop and wait for it to exit.
    pub async fn terminate(&self) {
        drop(self.stop_source.lock().take());
        let opt_jh = self.jh.lock().take();
        if let Some(jh) = opt_jh {
            jh.await;
        }
        self.receiver.drain();
    }

    /// Long-lived background processor loop
    async fn processor_loop(
        registry: VeilidComponentRegistry,
        stop_token: StopToken,
        rx: flume::Receiver<OutboundTransactionKeepaliveCommand>,
        deadlines: Arc<Mutex<OutboundTransactionKeepaliveDeadlines>>,
    ) {
        let mut unord = FuturesUnordered::<PinBoxFutureStatic<ProcessorResult>>::new();

        loop {
            let mut futs = vec![];

            // Start any due keepalives
            Self::start_due_keepalives(&registry, deadlines.clone(), &mut unord);

            // Wait for the next keepalive result if we have any in-flight
            if !unord.is_empty() {
                futs.push(pin_dyn_future!(async {
                    unord
                        .next()
                        .await
                        .unwrap_or_else(|| ProcessorResult::Failed {
                            error: "No keepalive result".to_string(),
                        })
                }));
            }

            // See if we have a next deadline, if so, add a delay to the unord to wait for it
            let opt_next_deadline = deadlines.lock().next_deadline();
            if let Some(next_deadline) = opt_next_deadline {
                let cur_ts = Timestamp::now();
                let delay_ms = next_deadline
                    .duration_since(cur_ts)
                    .millis_u32()
                    .unwrap_or(0);
                futs.push(pin_dyn_future!(async move {
                    sleep(delay_ms).await;
                    ProcessorResult::DeadlineReached
                }));
            }

            // Always wait on the channel
            futs.push(pin_dyn_future!(async {
                rx.recv_async()
                    .await
                    .map(|command| ProcessorResult::CommandReceived { command })
                    .unwrap_or_else(|e| ProcessorResult::Failed {
                        error: e.to_string(),
                    })
            }));

            // Race these futures and handle the first result
            let Ok((res, _idx, _other_futs)) =
                select_all(futs).timeout_at(stop_token.clone()).await
            else {
                // Stop token fired, shut down immediately
                break;
            };

            match res {
                ProcessorResult::CommandReceived { command } => match command {
                    OutboundTransactionKeepaliveCommand::AddKeepalive { send_ts, keepalive } => {
                        deadlines.lock().schedule_deadline(keepalive, send_ts);
                    }
                },
                ProcessorResult::DeadlineReached => {
                    // Fall through and continue looping to start_due_keepalives
                }
                ProcessorResult::KeepaliveResult {
                    keepalive,
                    opt_expiration,
                    opt_rtt,
                } => {
                    Self::handle_keepalive_result(
                        &registry,
                        deadlines.clone(),
                        keepalive,
                        opt_expiration,
                        opt_rtt,
                    );
                }
                ProcessorResult::Failed { error } => {
                    // Log the error and continue
                    veilid_log!(registry error "Keepalive processor error: error={}", error);
                }
            }

            // Loop to construct the next race
        }

        veilid_log!(registry debug "Transaction keepalive processor stopped. {} deadlines remaining.",
            deadlines.lock().len());
    }

    /// Start keepalive RPCs for all entries at or past their deadline.
    /// For each due entry, the next deadline is enqueued at send time so
    /// successive keepalives can be in-flight before the previous reply lands.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    fn start_due_keepalives(
        registry: &VeilidComponentRegistry,
        deadlines: Arc<Mutex<OutboundTransactionKeepaliveDeadlines>>,
        unord: &mut FuturesUnordered<PinBoxFutureStatic<ProcessorResult>>,
    ) {
        let due_entries = deadlines.lock().take_due_entries();
        if due_entries.is_empty() {
            return;
        }

        for due_entry in due_entries {
            Self::start_keepalive(registry, deadlines.clone(), unord, due_entry);
        }
    }

    /// Send one keepalive RPC and immediately enqueue the next deadline.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "dht", skip(registry, deadlines, unord))
    )]
    fn start_keepalive(
        registry: &VeilidComponentRegistry,
        deadlines: Arc<Mutex<OutboundTransactionKeepaliveDeadlines>>,
        unord: &mut FuturesUnordered<PinBoxFutureStatic<ProcessorResult>>,
        keepalive: OutboundTransactionKeepalive,
    ) {
        if keepalive.state().lock().dead {
            return;
        }

        let send_ts = Timestamp::now();

        // Enqueue the next deadline now, before the RPC is awaited.
        deadlines
            .lock()
            .schedule_deadline(keepalive.clone(), send_ts);

        let registry = registry.clone();

        #[cfg(feature = "verbose-tracing")]
        veilid_log!(registry debug target: "dht", "Keepalive sending: send_ts={:#} keepalive={:?}",
            send_ts, keepalive);

        #[cfg(feature = "instrument")]
        let span = tracing::trace_span!(
            target: "dht",
            "keepalive RPC",
            send_ts = ?send_ts,
        );

        let fut = async move {
            let rpc_processor = registry.rpc_processor();

            let start_ts = Timestamp::now();
            let result = rpc_processor
                .rpc_call_transact_command(
                    keepalive.dest().clone(),
                    keepalive.opaque_record_key().clone(),
                    keepalive.descriptor().clone(),
                    keepalive.rnxid().xid(),
                    TransactCommand::Get,
                    None,
                    None,
                    None,
                )
                .await;
            let elapsed = Timestamp::now().duration_since(start_ts);

            match result {
                Ok(NetworkResult::Value(answer)) => ProcessorResult::KeepaliveResult {
                    keepalive,
                    opt_expiration: answer.answer.opt_expiration,
                    opt_rtt: Some(elapsed),
                },
                #[allow(unused_variables)]
                Ok(v) => {
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(registry debug target: "dht", "Keepalive RPC failed: keepalive={:?}: {:?}", keepalive, v);
                    ProcessorResult::KeepaliveResult {
                        keepalive,
                        opt_expiration: None,
                        opt_rtt: None,
                    }
                }
                Err(e) => ProcessorResult::Failed {
                    error: e.to_string(),
                },
            }
        };
        #[cfg(feature = "instrument")]
        let fut = fut.instrument(span);
        unord.push(Box::pin(fut));
    }

    /// Update the train's shared state from a keepalive result. The next deadline
    /// is enqueued by `start_keepalive` at send time, not here. On a recording error
    /// the whole transaction's deadlines are drained so further fires don't pile up
    /// after the TX is gone client-side.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(registry, deadlines))
    )]
    fn handle_keepalive_result(
        registry: &VeilidComponentRegistry,
        deadlines: Arc<Mutex<OutboundTransactionKeepaliveDeadlines>>,
        keepalive: OutboundTransactionKeepalive,
        opt_expiration: Option<Timestamp>,
        opt_rtt: Option<TimestampDuration>,
    ) {
        match (opt_expiration, opt_rtt) {
            (Some(expiration), _) => {
                // Success — update the shared train state.
                {
                    let mut st = keepalive.state().lock();
                    if let Some(rtt) = opt_rtt {
                        st.opt_last_rtt = Some(rtt);
                    }
                    st.opt_last_expiration = Some(expiration);
                }
                let record_result = {
                    let storage_manager = registry.storage_manager();
                    let mut inner = storage_manager.inner.lock();
                    inner
                        .outbound_transaction_manager
                        .record_transact_keepalive_result(keepalive.lnxid(), expiration, opt_rtt)
                };
                if let Err(e) = record_result {
                    veilid_log!(registry debug "Keepalive result recording failed: keepalive={:?} expiration={:#}: {}",
                        keepalive, expiration, e);
                    deadlines.lock().cancel_deadline(keepalive.lnxid());
                }
            }
            (None, Some(_)) => {
                // Server responded but the inbound transaction is gone.
                // Mark dead so future fires bail.
                deadlines.lock().cancel_deadline(keepalive.lnxid());
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(registry debug target: "dht", "Keepalive ended (server said no such transaction): keepalive={:?}", keepalive);
            }
            (None, None) => {
                // RPC failure — the next fire is already queued; it will retry.
            }
        }
    }
}
