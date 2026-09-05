use super::*;
use futures_util::{FutureExt, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{io, sync::Arc};
use stop_token::prelude::*;

impl_veilid_log_facility!("net");

cfg_if::cfg_if! {
    if #[cfg(all(target_arch = "wasm32", target_os = "unknown"))] {
        // No accept support for WASM
    } else {

        ///////////////////////////////////////////////////////////
        // Accept

        pub(crate) trait ProtocolAcceptHandler: ProtocolAcceptHandlerClone + Send + Sync {
            fn on_accept(
                &self,
                stream: AsyncPeekStream,
                peer_addr: SocketAddr,
                local_addr: SocketAddr,
            ) -> PinBoxFutureStatic<io::Result<Option<ProtocolNetworkConnection>>>;
        }

        pub(crate) trait ProtocolAcceptHandlerClone {
            fn clone_box(&self) -> Box<dyn ProtocolAcceptHandler>;
        }

        impl<T> ProtocolAcceptHandlerClone for T
        where
            T: 'static + ProtocolAcceptHandler + Clone,
        {
            fn clone_box(&self) -> Box<dyn ProtocolAcceptHandler> {
                Box::new(self.clone())
            }
        }
        impl Clone for Box<dyn ProtocolAcceptHandler> {
            fn clone(&self) -> Box<dyn ProtocolAcceptHandler> {
                self.clone_box()
            }
        }

        pub(crate) type NewProtocolAcceptHandler =
            dyn Fn(VeilidComponentRegistry, bool) -> Box<dyn ProtocolAcceptHandler> + Send;
    }
}
///////////////////////////////////////////////////////////
// Dummy protocol network connection for testing

// #[derive(Debug)]
// pub struct DummyNetworkConnection {
//     flow: Flow,
// }

// impl DummyNetworkConnection {
//     pub fn flow(&self) -> Flow {
//         self.flow
//     }
//     pub fn close(&self) -> io::Result<NetworkResult<()>> {
//         Ok(NetworkResult::Value(()))
//     }
//     pub fn send(&self, _message: Vec<u8>) -> io::Result<NetworkResult<()>> {
//         Ok(NetworkResult::Value(()))
//     }
//     pub fn recv(&self) -> io::Result<NetworkResult<Vec<u8>>> {
//         Ok(NetworkResult::Value(Vec::new()))
//     }
// }

///////////////////////////////////////////////////////////
// Top-level protocol independent network connection object

#[derive(Clone, Copy, Debug)]
enum RecvLoopAction {
    Send,
    Recv,
    Finish,
    Timeout,
}

#[derive(Debug, Clone)]
pub struct NetworkConnectionStats {
    last_message_sent_time: Option<Timestamp>,
    last_message_recv_time: Option<Timestamp>,
}

/// Represents a connection in the connection table for connection-oriented protocols
pub(crate) struct NetworkConnection {
    /// Registry accessor
    registry: VeilidComponentRegistry,
    /// A unique id for this connection
    connection_id: NetworkConnectionId,
    /// The dial info used to make this connection if it was made with 'connect'
    /// None if the connection was 'accepted'
    opt_dial_info: Option<DialInfo>,
    /// The network flow 5-tuple this connection is over
    flow: Flow,
    /// Each connection has a processor and this is the task we wait for to ensure it exits cleanly
    processor: Option<MustJoinHandle<()>>,
    /// When this connection was connected or accepted
    established_time: Timestamp,
    /// Statistics about network traffic
    stats: Arc<Mutex<NetworkConnectionStats>>,
    /// To send data out this connection, it is placed in this channel
    sender: flume::Sender<(Option<Id>, Bytes)>,
    /// Drop this when we want to drop the connection
    stop_source: Option<StopSource>,
    /// The node we are responsible for protecting the connection for if it is protected
    protected_nr: Option<NodeRef>,
    /// Mirrors `protected_nr.is_some()` so the connection-processor loop can skip
    /// inactivity-timeout-driven closure on protected connections without holding a lock.
    is_protected: Arc<AtomicBool>,
    /// Set when the processor loop exits or close() is called; shared with handles so
    /// sends fail with NotSent instead of queueing to a channel nothing drains
    dead: Arc<AtomicBool>,
    /// The number of references to the network connection that exist (handles)
    ref_count: usize,
}

impl_veilid_component_accessors!(NetworkConnection);

impl fmt::Debug for NetworkConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetworkConnection")
            //.field("registry", &self.registry)
            .field("connection_id", &self.connection_id)
            .field("opt_dial_info", &self.opt_dial_info)
            .field("flow", &self.flow)
            .field("processor", &self.processor)
            .field("established_time", &self.established_time)
            .field("stats", &self.stats)
            .field("sender", &self.sender)
            .field("stop_source", &self.stop_source)
            .field("protected_nr", &self.protected_nr)
            .field("ref_count", &self.ref_count)
            .finish()
    }
}

impl Drop for NetworkConnection {
    fn drop(&mut self) {
        if self.ref_count != 0 && self.stop_source.is_some() {
            veilid_log!(self error "ref_count for network connection should be zero: {:?}", self);
        }
    }
}

impl NetworkConnection {
    #[cfg(any(test, feature = "test-util"))]
    pub(super) fn dummy(
        registry: VeilidComponentRegistry,
        id: NetworkConnectionId,
        flow: Flow,
    ) -> Self {
        // Create handle for sending (dummy is immediately disconnected)
        let (sender, _receiver) = flume::bounded(get_concurrency() as usize);

        Self {
            registry,
            connection_id: id,
            opt_dial_info: None,
            flow,
            processor: None,
            established_time: Timestamp::now_non_decreasing(),
            stats: Arc::new(Mutex::new(NetworkConnectionStats {
                last_message_sent_time: None,
                last_message_recv_time: None,
            })),
            sender,
            stop_source: None,
            protected_nr: None,
            is_protected: Arc::new(AtomicBool::new(false)),
            dead: Arc::new(AtomicBool::new(false)),
            ref_count: 0,
        }
    }

    pub(super) fn from_protocol(
        connection_manager: ConnectionManager,
        manager_stop_token: StopToken,
        protocol_connection: ProtocolNetworkConnection,
        connection_id: NetworkConnectionId,
        opt_dial_info: Option<DialInfo>,
    ) -> Self {
        // Get flow
        let flow = protocol_connection.flow();

        // Create handle for sending
        //let (sender, receiver) = flume::bounded(get_concurrency() as usize);
        let (sender, receiver) = flume::unbounded();

        // Create stats
        let stats = Arc::new(Mutex::new(NetworkConnectionStats {
            last_message_sent_time: None,
            last_message_recv_time: None,
        }));

        let stop_source = StopSource::new();
        let local_stop_token = stop_source.token();

        let is_protected = Arc::new(AtomicBool::new(false));
        let dead = Arc::new(AtomicBool::new(false));

        // Spawn connection processor and pass in protocol connection
        let registry = connection_manager.registry();
        let processor = spawn(
            "connection processor",
            Self::process_connection(
                connection_manager,
                local_stop_token,
                manager_stop_token,
                connection_id,
                flow,
                receiver,
                protocol_connection,
                stats.clone(),
                is_protected.clone(),
                dead.clone(),
            ),
        );

        // Return the connection
        Self {
            registry,
            connection_id,
            opt_dial_info,
            flow,
            processor: Some(processor),
            established_time: Timestamp::now_non_decreasing(),
            stats,
            sender,
            stop_source: Some(stop_source),
            protected_nr: None,
            is_protected,
            dead,
            ref_count: 0,
        }
    }

    pub fn connection_id(&self) -> NetworkConnectionId {
        self.connection_id
    }

    pub fn flow(&self) -> Flow {
        self.flow
    }

    pub fn dial_info(&self) -> Option<DialInfo> {
        self.opt_dial_info.clone()
    }

    #[expect(dead_code)]
    pub fn unique_flow(&self) -> UniqueFlow {
        UniqueFlow {
            flow: self.flow,
            connection_id: Some(self.connection_id),
        }
    }

    pub fn get_handle(&self) -> ConnectionHandle {
        ConnectionHandle::new(
            self.connection_id,
            self.flow,
            self.sender.clone(),
            self.dead.clone(),
        )
    }

    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Relaxed)
    }

    pub fn is_in_use(&self) -> bool {
        self.ref_count > 0
    }

    pub fn protected_node_ref(&self) -> Option<NodeRef> {
        self.protected_nr.clone()
    }

    pub fn protect(&mut self, protect_nr: NodeRef) {
        self.protected_nr = Some(protect_nr);
        self.is_protected.store(true, Ordering::Relaxed);
    }

    pub fn unprotect(&mut self) {
        self.protected_nr = None;
        self.is_protected.store(false, Ordering::Relaxed);
    }

    pub fn add_ref(&mut self) {
        self.ref_count += 1;
    }

    pub fn remove_ref(&mut self) {
        self.ref_count -= 1;
    }

    pub fn close(&mut self) {
        self.dead.store(true, Ordering::Relaxed);
        if let Some(stop_source) = self.stop_source.take() {
            // drop the stopper
            drop(stop_source);
        }
    }

    async fn send_internal(
        protocol_connection: &ProtocolNetworkConnection,
        stats: Arc<Mutex<NetworkConnectionStats>>,
        message: Bytes,
    ) -> io::Result<NetworkResult<()>> {
        let ts = Timestamp::now_non_decreasing();
        network_result_try!(protocol_connection.send(message).await?);

        let mut stats = stats.lock();
        stats.last_message_sent_time.max_assign(Some(ts));

        Ok(NetworkResult::Value(()))
    }

    async fn recv_internal(
        protocol_connection: &ProtocolNetworkConnection,
        stats: Arc<Mutex<NetworkConnectionStats>>,
    ) -> io::Result<NetworkResult<Bytes>> {
        let ts = Timestamp::now_non_decreasing();
        let out = network_result_try!(protocol_connection.recv().await?);

        let mut stats = stats.lock();
        stats.last_message_recv_time.max_assign(Some(ts));

        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.len", out.len());

        Ok(NetworkResult::Value(out))
    }

    pub fn stats(&self) -> NetworkConnectionStats {
        let stats = self.stats.lock();
        stats.clone()
    }

    pub fn established_time(&self) -> Timestamp {
        self.established_time
    }

    // Connection receiver loop
    #[allow(clippy::too_many_arguments)]
    #[cfg_attr(feature = "instrument", instrument(parent = None, level="trace", target="net", skip_all, fields(__VEILID_LOG_KEY = connection_manager.log_key())))]
    fn process_connection(
        connection_manager: ConnectionManager,
        local_stop_token: StopToken,
        manager_stop_token: StopToken,
        connection_id: NetworkConnectionId,
        flow: Flow,
        receiver: flume::Receiver<(Option<Id>, Bytes)>,
        protocol_connection: ProtocolNetworkConnection,
        stats: Arc<Mutex<NetworkConnectionStats>>,
        is_protected: Arc<AtomicBool>,
        dead: Arc<AtomicBool>,
    ) -> PinBoxFutureStatic<()> {
        Box::pin(async move {
            let registry = connection_manager.registry();

            veilid_log!(registry trace
                "Starting process_connection loop for id={}, {:?}", connection_id,
                flow
            );

            let mut unord = FuturesUnordered::new();
            let mut need_receiver = true;
            let mut need_sender = true;

            // Protected connections are owned by the connection_manager's drop
            // accounting and should not be closed for recv-side inactivity.
            let inactivity_duration = TimestampDuration::new_ms(connection_manager.connection_inactivity_timeout_ms() as u64);
            let last_activity_timestamp = Arc::new(AtomicTimestamp::now());
            let last_activity_timestamp_clone = last_activity_timestamp.clone();
            let is_protected_clone = is_protected.clone();
            let inactivity_timer = Box::pin(async move {
                    loop {
                        if is_protected_clone.load(Ordering::Relaxed) {
                            sleep(inactivity_duration.millis_u32().unwrap_or(60_000)).await;
                            continue;
                        }
                        let cur_ts = Timestamp::now_non_decreasing();

                        let last_activity_ts = last_activity_timestamp_clone.get();
                        let duration_since_last_activity = cur_ts.duration_since(last_activity_ts);
                        if duration_since_last_activity >= inactivity_duration {
                            break;
                        }
                        let ms_remaining_until_next_timeout = (inactivity_duration.saturating_sub(duration_since_last_activity)).millis_u32().unwrap_or(0);
                        sleep(ms_remaining_until_next_timeout).await;
                    }

                    veilid_log!(registry trace "Connection inactivity timeout on {:?}", flow);
                    RecvLoopAction::Timeout
                }).in_current_span();

            unord.push(pin_dyn_future!(inactivity_timer));

            // Do unord loop
            let registry = connection_manager.registry();
            loop {
                // Add another message sender future if necessary
                if need_sender {
                    need_sender = false;
                    let registry = registry.clone();
                    let sender_fut = receiver.recv_async().then(|res| async {
                        let registry = registry;
                        match res {
                            Ok((_span_id, message)) => {
                                // Touch the LRU for this connection
                                connection_manager.touch_connection_by_id(connection_id);

                                // send the packet
                                match Self::send_internal(
                                    &protocol_connection,
                                    stats.clone(),
                                    message,
                                )
                                .await
                                {
                                    Err(e) => {
                                        // Sending the packet along can fail, if so, this connection is dead
                                        veilid_log!(connection_manager debug e);
                                        RecvLoopAction::Finish
                                    }
                                    Ok(v) => {
                                        network_result_value_or_log!(registry v => [ format!(": send via protocol_connection={:?}", protocol_connection) ] {
                                            return RecvLoopAction::Finish;
                                        });

                                        RecvLoopAction::Send
                                    }
                                }
                            }
                            Err(e) => {
                                // All senders gone, shouldn't happen since we store one alongside the join handle
                                veilid_log!(connection_manager warn e);
                                RecvLoopAction::Finish
                            }
                        }
                    }.in_current_span());

                    unord.push(pin_dyn_future!(sender_fut.in_current_span()));
                }

                // Add another message receiver future if necessary
                if need_receiver {
                    need_receiver = false;
                    let registry = registry.clone();
                    let receiver_fut = Self::recv_internal(&protocol_connection, stats.clone())
                        .then(|res| async {
                            let registry = registry;
                            let network_manager = registry.network_manager();
                            match res {
                                Ok(v) => {
                                    let peer_address = protocol_connection.flow().remote();

                                    // Check to see if it is punished
                                    if network_manager.address_filter().is_ip_addr_punished(peer_address.socket_addr().ip()) {
                                        return RecvLoopAction::Finish;
                                    }

                                    // Check for connection close
                                    if v.is_no_connection() {
                                        veilid_log!(registry trace "Connection closed from: {} ({})", peer_address.socket_addr(), peer_address.protocol_type());
                                        return RecvLoopAction::Finish;
                                    }

                                    // Invalid framing (tcp framing or websocket framing)
                                    if v.is_invalid_message() {
                                        // Outbound: protocol collision (simultaneous open), not an attack; don't punish the IP
                                        if protocol_connection.direction() == Direction::Out {
                                            veilid_log!(registry debug
                                                "InvalidFraming on outbound {} (protocol collision, not punished): {:?}",
                                                protocol_connection.flow(),
                                                v
                                            );
                                            return RecvLoopAction::Finish;
                                        }
                                        veilid_log!(registry debug
                                            "InvalidFraming on {}: {:?}",
                                            protocol_connection.flow(),
                                            v
                                        );
                                        network_manager.address_filter().punish_ip_addr(peer_address.socket_addr().ip(), PunishmentReason::InvalidFraming);
                                        return RecvLoopAction::Finish;
                                    }

                                    // Log other network results
                                    let message = network_result_value_or_log!(registry v => [ format!(": recv via protocol_connection={:?}", protocol_connection) ] {
                                        return RecvLoopAction::Finish;
                                    });

                                    // Pass received messages up to the network manager for processing
                                    if let Err(e) = network_manager
                                        .on_recv_envelope(message, flow).measure_debug(TimestampDuration::new_ms(500), |x| {
                                            veilid_log!(registry debug "on_recv_envelope: {:#} for {} ", x, protocol_connection.flow());
                                        })
                                        .await
                                    {
                                        veilid_log!(registry debug "failed to process received envelope: {}", e);
                                        RecvLoopAction::Finish
                                    } else {
                                        // Touch the LRU for this connection
                                        connection_manager.touch_connection_by_id(connection_id);

                                        RecvLoopAction::Recv
                                    }
                                }
                                Err(e) => {
                                    // Connection unable to receive, closed
                                    veilid_log!(registry error "connection unable to receive: {}", e);
                                    RecvLoopAction::Finish
                                }
                            }
                        }.in_current_span());

                    unord.push(pin_dyn_future!(receiver_fut.in_current_span()));
                }

                // Process futures
                match unord
                    .next()
                    .timeout_at(local_stop_token.clone())
                    .timeout_at(manager_stop_token.clone())
                    .await
                    .and_then(std::convert::identity)   // flatten stoptoken timeouts
                {
                    Ok(Some(RecvLoopAction::Send)) => {
                        // Don't reset inactivity timer if we're only sending
                        need_sender = true;
                    }
                    Ok(Some(RecvLoopAction::Recv)) => {
                        // Reset inactivity timer since we got something from this connection
                        last_activity_timestamp.set(Timestamp::now());

                        need_receiver = true;
                    }
                    Ok(Some(RecvLoopAction::Finish) | Some(RecvLoopAction::Timeout)) => {
                        break;
                    }
                    Ok(None) => {
                        // FuturesUnordered empty: should not happen, log and exit loop
                        veilid_log!(registry error "connection processor futures unordered unexpectedly empty");
                        break;
                    }
                    Err(_) => {
                        // Either one of the stop tokens
                        break;
                    }
                }
            }


            // Mark dead before removal from the table so handles stop queueing sends
            dead.store(true, Ordering::Relaxed);

            // Let the connection manager know the receive loop exited
            connection_manager
                .report_connection_finished(connection_id);

            // Close the low level socket
            if let Err(e) = protocol_connection.close().await {
                veilid_log!(registry debug "Protocol connection close error: {}", e);
            }

            veilid_log!(registry trace
                "Connection loop exited flow={:?}",
                flow
            );

        }.in_current_span())
    }

    pub fn debug_print(&self, cur_ts: Timestamp) -> String {
        format!(
            "{} | {} | est {} sent {} rcvd {} refcount {}{}",
            self.flow,
            self.connection_id.as_u64(),
            human_duration(
                cur_ts
                    .as_u64()
                    .saturating_sub(self.established_time.as_u64())
            ),
            self.stats()
                .last_message_sent_time
                .map(|ts| human_duration(cur_ts.as_u64().saturating_sub(ts.as_u64())))
                .unwrap_or("---".to_owned()),
            self.stats()
                .last_message_recv_time
                .map(|ts| human_duration(cur_ts.as_u64().saturating_sub(ts.as_u64())))
                .unwrap_or("---".to_owned()),
            self.ref_count,
            if let Some(pnr) = &self.protected_nr {
                format!(" PROTECTED:{}", pnr)
            } else {
                "".to_owned()
            },
        )
    }
}

// Resolves ready when the connection loop has terminated
impl Future for NetworkConnection {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        let mut pending = 0usize;

        // Process all sub-futures, nulling them out when they return ready
        if let Some(mut processor) = self.processor.as_mut() {
            if Pin::new(&mut processor).poll(cx).is_ready() {
                self.processor = None;
            } else {
                pending += 1
            }
        }

        // Any sub-futures pending?
        if pending > 0 {
            task::Poll::Pending
        } else {
            task::Poll::Ready(())
        }
    }
}
