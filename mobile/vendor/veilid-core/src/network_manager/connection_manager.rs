use super::*;
use connection_table::ConnectionRefKind;
use connection_table::*;
use stop_token::future::FutureExt;

impl_veilid_log_facility!("net");

const PROTECTED_CONNECTION_DROP_SPAN: TimestampDuration = TimestampDuration::new_secs(10);
const PROTECTED_CONNECTION_DROP_COUNT: usize = 3;
const NEW_CONNECTION_RETRY_COUNT: usize = 0;
const NEW_CONNECTION_RETRY_DELAY_MS: u32 = 500;

///////////////////////////////////////////////////////////
// Connection manager

/// Per-remote address lock guard, acquired at accept time and held until the connection is
/// registered, keeping the connection table in sync with the kernel's connection table.
pub(super) type AddressLockGuard = AsyncTagLockGuard<SocketAddr>;

#[derive(Debug)]
enum ConnectionManagerEvent {
    Accepted(ProtocolNetworkConnection, AddressLockGuard),
    Dead(Box<NetworkConnection>),
}

pub struct ConnectionRefScope {
    connection_manager: ConnectionManager,
    id: NetworkConnectionId,
}

impl fmt::Debug for ConnectionRefScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionRefScope")
            .field("id", &self.id)
            .finish()
    }
}

impl ConnectionRefScope {
    pub fn try_new(connection_manager: ConnectionManager, id: NetworkConnectionId) -> Option<Self> {
        if !connection_manager.connection_ref(id, ConnectionRefKind::AddRef) {
            return None;
        }
        Some(Self {
            connection_manager,
            id,
        })
    }
}

impl Drop for ConnectionRefScope {
    fn drop(&mut self) {
        self.connection_manager
            .connection_ref(self.id, ConnectionRefKind::RemoveRef);
    }
}

#[derive(Debug)]
struct ProtectedAddress {
    node_ref: NodeRef,
    span_start_ts: Timestamp,
    drops_in_span: usize,
}

#[derive(Debug)]
struct ConnectionManagerInner {
    next_id: NetworkConnectionId,
    sender: flume::Sender<ConnectionManagerEvent>,
    async_processor_jh: Option<MustJoinHandle<()>>,
    stop_source: Option<StopSource>,
    protected_addresses: HashMap<SocketAddress, ProtectedAddress>,
}

struct ConnectionManagerArc {
    connection_initial_timeout_ms: u32,
    connection_inactivity_timeout_ms: u32,
    connection_table: ConnectionTable,
    address_lock_table: AsyncTagLockTable<SocketAddr>,
    startup_lock: StartupLock,
    /// Flap detector over protected (relay) connection drops, which should be rare
    connection_flap_detector: Mutex<FlapDetector<()>>,
    inner: Mutex<Option<ConnectionManagerInner>>,
    reconnection_processor: DeferredStreamProcessor,
    commit_subscription: Mutex<Option<EventBusSubscription>>,
    route_allocation_subscription: Mutex<Option<EventBusSubscription>>,
}
impl core::fmt::Debug for ConnectionManagerArc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ConnectionManagerArc")
            .field("inner", &self.inner)
            .finish()
    }
}

#[derive(Clone)]
pub struct ConnectionManager {
    network: Network,
    arc: Arc<ConnectionManagerArc>,
}

impl VeilidComponentRegistryAccessor for ConnectionManager {
    fn registry(&self) -> VeilidComponentRegistry {
        self.network.registry()
    }
}

impl ConnectionManager {
    fn new_inner(
        stop_source: StopSource,
        sender: flume::Sender<ConnectionManagerEvent>,
        async_processor_jh: MustJoinHandle<()>,
    ) -> ConnectionManagerInner {
        ConnectionManagerInner {
            next_id: 0.into(),
            stop_source: Some(stop_source),
            sender,
            async_processor_jh: Some(async_processor_jh),
            protected_addresses: HashMap::new(),
        }
    }
    fn new_arc(registry: VeilidComponentRegistry) -> ConnectionManagerArc {
        let config = registry.config();
        let connection_initial_timeout_ms = config.internal().network.connection_initial_timeout_ms;
        let connection_inactivity_timeout_ms =
            config.internal().network.connection_inactivity_timeout_ms;

        ConnectionManagerArc {
            reconnection_processor: DeferredStreamProcessor::new(),
            connection_initial_timeout_ms,
            connection_inactivity_timeout_ms,
            connection_table: ConnectionTable::new(registry),
            address_lock_table: AsyncTagLockTable::new(),
            startup_lock: StartupLock::new(),
            // 4 protected-connection drops within ~60s = flapping
            connection_flap_detector: Mutex::new(FlapDetector::new_secs(4.0, 60)),
            inner: Mutex::new(None),
            commit_subscription: Mutex::new(None),
            route_allocation_subscription: Mutex::new(None),
        }
    }
    pub fn new(network: Network) -> Self {
        Self {
            arc: Arc::new(Self::new_arc(network.registry())),
            network,
        }
    }

    pub fn connection_inactivity_timeout_ms(&self) -> u32 {
        self.arc.connection_inactivity_timeout_ms
    }

    pub fn startup(&self) -> EyreResult<()> {
        let guard = self.arc.startup_lock.startup()?;

        veilid_log!(self debug "startup connection manager");

        // Create channel for async_processor to receive notifications of networking events
        let (sender, receiver) = flume::unbounded();

        // Create the stop source we'll use to stop the processor and the connection table
        let stop_source = StopSource::new();

        // Spawn the async processor
        let async_processor = spawn(
            "connection manager async processor",
            self.clone().async_processor(stop_source.token(), receiver),
        );

        // Store in the inner object
        {
            let mut inner = self.arc.inner.lock();
            if inner.is_some() {
                bail!("shouldn't start connection manager twice without shutting it down first");
            }
            *inner = Some(Self::new_inner(stop_source, sender, async_processor));
        }

        // Spawn the reconnection processor
        self.arc.reconnection_processor.init();

        // Subscribe to events that may invalidate our protected-address set
        let commit_sub = impl_subscribe_event_bus_clone!(self, commit_event_handler);
        *self.arc.commit_subscription.lock() = Some(commit_sub);
        let route_alloc_sub = impl_subscribe_event_bus_clone!(self, route_allocation_event_handler);
        *self.arc.route_allocation_subscription.lock() = Some(route_alloc_sub);

        guard.success();

        Ok(())
    }

    pub fn commit_event_handler(&self, _evt: Arc<RoutingDomainCommitEvent>) {
        self.update_protections();
    }

    pub fn route_allocation_event_handler(&self, _evt: Arc<RouteAllocationEvent>) {
        self.update_protections();
    }

    pub async fn shutdown(&self) {
        veilid_log!(self debug "starting connection manager shutdown");
        let Ok(guard) = self.arc.startup_lock.shutdown().await else {
            veilid_log!(self error "connection manager is already shut down");
            return;
        };

        // Unsubscribe from event bus
        if let Some(sub) = self.arc.commit_subscription.lock().take() {
            self.event_bus().unsubscribe(sub);
        }
        if let Some(sub) = self.arc.route_allocation_subscription.lock().take() {
            self.event_bus().unsubscribe(sub);
        }

        // Stop the reconnection processor
        veilid_log!(self debug "stopping reconnection processor task");
        self.arc.reconnection_processor.terminate().await;

        // Remove the inner from the lock
        let mut inner = {
            let mut inner_lock = self.arc.inner.lock();
            match inner_lock.take() {
                Some(v) => v,
                None => {
                    veilid_log!(self error "connection manager not started");
                    return;
                }
            }
        };
        // Stop all the connections and the async processor
        veilid_log!(self debug "stopping async processor task");
        drop(inner.stop_source.take());
        let async_processor_jh = inner.async_processor_jh.take().unwrap_or_log();
        // wait for the async processor to stop
        veilid_log!(self debug "waiting for async processor to stop");
        async_processor_jh.await;
        // Wait for the connections to complete
        veilid_log!(self debug "waiting for connection handlers to complete");
        self.arc.connection_table.join().await;

        guard.success();
        veilid_log!(self debug "finished connection manager shutdown");
    }

    // Internal routine to see if we should keep this connection
    // from being LRU removed. Used on our initiated relay connections.
    fn should_protect_connection(
        &self,
        inner: &mut ConnectionManagerInner,
        conn: &NetworkConnection,
    ) -> Option<NodeRef> {
        inner
            .protected_addresses
            .get(conn.flow().remote_address())
            .map(|x| x.node_ref.clone())
    }

    // Update connection protections if things change, like a node becomes a relay
    pub fn update_protections(&self) {
        let Ok(_guard) = self.arc.startup_lock.enter() else {
            return;
        };

        let mut lock = self.arc.inner.lock();
        let Some(inner) = lock.as_mut() else {
            return;
        };

        // Protect addresses for relays in all routing domains
        let mut dead_addresses = inner
            .protected_addresses
            .keys()
            .cloned()
            .collect::<HashSet<_>>();
        for routing_domain in RoutingDomainSet::all() {
            for relay in self
                .network_manager()
                .routing_table()
                .relays(routing_domain)
            {
                for did in relay.relay_node.dial_info_details() {
                    // SocketAddress are distinct per routing domain, so they should not collide
                    // and two nodes should never have the same SocketAddress
                    let protected_address = did.dial_info.socket_address();

                    // Update the protection, note the protected address is not dead
                    dead_addresses.remove(&protected_address);
                    inner
                        .protected_addresses
                        .entry(protected_address)
                        .and_modify(|pa| pa.node_ref = relay.relay_node.unfiltered())
                        .or_insert_with(|| ProtectedAddress {
                            node_ref: relay.relay_node.unfiltered(),
                            span_start_ts: Timestamp::now_non_decreasing(),
                            drops_in_span: 0usize,
                        });
                }
            }
        }

        // Safety-route first hops are intentionally NOT protected: with no far-side priority
        // flow they idle out, and reconnecting them only churns. The bidirectional route lets
        // the first hop reopen a connection to us for return-path answers when ours is down.

        // Remove protected addresses that were not still associated with a protected noderef
        for dead_address in dead_addresses {
            inner.protected_addresses.remove(&dead_address);
        }

        // For all connections, register the protection
        self.arc
            .connection_table
            .with_all_connections_mut(|conn| {
                if let Some(protect_nr) = conn.protected_node_ref() {
                    if self.should_protect_connection(inner, conn).is_none() {
                        veilid_log!(self debug "== Unprotecting connection: {} -> {} for node {}", conn.connection_id(), conn.debug_print(Timestamp::now()), protect_nr);
                        conn.unprotect();
                    }
                } else if let Some(protect_nr) = self.should_protect_connection(inner, conn) {
                    veilid_log!(self debug "== Protecting existing connection: {} -> {} for node {}", conn.connection_id(), conn.debug_print(Timestamp::now()), protect_nr);
                    conn.protect(protect_nr);
                }
                Option::<()>::None
        });
    }

    // Internal routine to register new connection atomically.
    // Registers connection in the connection table for later access
    // and spawns a message processing loop for the connection
    //#[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self, inner), ret, err, fields(__VEILID_LOG_KEY = self.log_key())))]
    fn on_new_protocol_network_connection(
        &self,
        inner: &mut ConnectionManagerInner,
        prot_conn: ProtocolNetworkConnection,
        opt_dial_info: Option<DialInfo>,
    ) -> EyreResult<NetworkResult<ConnectionHandle>> {
        // Get next connection id to use
        let id = inner.next_id;
        inner.next_id += 1u64;
        veilid_log!(self trace
            "on_new_protocol_network_connection: id={} prot_conn={:?}",
            id,
            prot_conn
        );

        // Wrap with NetworkConnection object to start the connection processing loop
        let stop_token = match &inner.stop_source {
            Some(ss) => ss.token(),
            None => {
                bail!("not creating connection because we are stopping");
            }
        };

        let mut conn = NetworkConnection::from_protocol(
            self.clone(),
            stop_token,
            prot_conn,
            id,
            opt_dial_info,
        );
        let handle = conn.get_handle();

        // See if this should be a protected connection
        if let Some(protect_nr) = self.should_protect_connection(inner, &conn) {
            veilid_log!(self debug "== Protecting new connection: {} -> {} for node {}", id, conn.debug_print(Timestamp::now()), protect_nr);
            conn.protect(protect_nr);
        }

        // Add to the connection table
        match self.arc.connection_table.add_connection(conn) {
            Ok(None) => {
                // Connection added
            }
            Ok(Some(conn)) => {
                // Connection added and a different one LRU'd out
                // Send it to be terminated
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(self debug "== LRU kill connection due to limit: {:?}", conn.debug_print(Timestamp::now()));
                let _ = inner
                    .sender
                    .send(ConnectionManagerEvent::Dead(Box::new(conn)));
            }
            Err(ConnectionTableAddError::AddressFilter(conn, e)) => {
                // Connection filtered
                let desc = conn.flow();
                let _ = inner.sender.send(ConnectionManagerEvent::Dead(conn));
                return Ok(NetworkResult::no_connection_other(format!(
                    "connection filtered: {:?} ({})",
                    desc, e
                )));
            }
            Err(ConnectionTableAddError::AlreadyExists(conn)) => {
                // Connection already exists
                let desc = conn.flow();
                veilid_log!(self debug "== Connection already exists: {:?}", conn.debug_print(Timestamp::now()));
                let _ = inner.sender.send(ConnectionManagerEvent::Dead(conn));
                return Ok(NetworkResult::no_connection_other(format!(
                    "connection already exists: {:?}",
                    desc
                )));
            }
            Err(ConnectionTableAddError::TableFull(conn)) => {
                // Connection table is full
                let desc = conn.flow();
                veilid_log!(self debug "== Connection table full: {:?}", conn.debug_print(Timestamp::now()));
                let _ = inner.sender.send(ConnectionManagerEvent::Dead(conn));
                return Ok(NetworkResult::no_connection_other(format!(
                    "connection table is full: {:?}",
                    desc
                )));
            }
        };
        Ok(NetworkResult::Value(handle))
    }

    // Returns a network connection if one already is established
    pub fn get_connection(&self, flow: Flow) -> Option<ConnectionHandle> {
        let Ok(_guard) = self.arc.startup_lock.enter() else {
            return None;
        };
        self.arc.connection_table.peek_connection_by_flow(flow)
    }

    // Returns a network connection if one already is established
    pub(super) fn touch_connection_by_id(&self, id: NetworkConnectionId) {
        self.arc.connection_table.touch_connection_by_id(id)
    }

    /// Keep track of the number of things using a network connection if one already is established
    /// to keep it from being removed from the table during use
    fn connection_ref(&self, id: NetworkConnectionId, kind: ConnectionRefKind) -> bool {
        self.arc.connection_table.ref_connection_by_id(id, kind)
    }

    /// Scope guard for connection ref to keep connection alive when we're using it
    pub fn try_connection_ref_scope(&self, id: NetworkConnectionId) -> Option<ConnectionRefScope> {
        let Ok(_guard) = self.arc.startup_lock.enter() else {
            return None;
        };
        ConnectionRefScope::try_new(self.clone(), id)
    }

    /// Called when we want to create a new connection or get the current one that already exists
    /// This will kill off any connections that are in conflict with the new connection to be made
    /// in order to make room for the new connection in the system's connection table
    /// This routine needs to be atomic, or connections may exist in the table that are not established
    //#[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), ret, err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn get_or_create_connection(
        &self,
        dial_info: DialInfo,
    ) -> EyreResult<NetworkResult<ConnectionHandle>> {
        let Ok(_guard) = self.arc.startup_lock.enter() else {
            return Ok(NetworkResult::service_unavailable(
                "connection manager is not started",
            ));
        };

        // Never connect to one of our own addresses; it loops back to our own listener
        if self
            .network_manager()
            .routing_table()
            .is_own_direct_dial_info_address(&dial_info.socket_address())
        {
            return Ok(NetworkResult::no_connection_other(
                "refusing to connect to our own address",
            ));
        }

        let peer_address = dial_info.peer_address();
        let remote_addr = peer_address.socket_addr();
        let remote = (
            dial_info.protocol_type().low_level_protocol_type(),
            *peer_address.socket_address(),
        );
        // Prefer a publicly-routable source (PublicInternet only); else use configured listener
        let net = self.network_manager().net();
        let preferred_local_address = net
            .preferred_outbound_source_addr(&dial_info)
            .or_else(|| net.get_preferred_local_address(&dial_info));
        let best_port = preferred_local_address.map(|pla| pla.port());

        // Fast path: an existing connection makes the lock unnecessary. Parallel sends to the same
        // remote would otherwise all queue on the address lock and time out before the first TCP completes
        if let Some(best_existing_conn) = self
            .arc
            .connection_table
            .get_best_connection_by_remote(best_port, remote)
        {
            return Ok(NetworkResult::Value(best_existing_conn));
        }

        // Slow path: serialize the actual TCP connect via per-remote address lock
        let Ok(_lock_guard) = timeout(
            self.arc.connection_initial_timeout_ms,
            self.arc.address_lock_table.lock_tag(remote_addr),
        )
        .await
        else {
            veilid_log!(self debug "== get_or_create_connection: connection timeout trying to connect to dial_info={:?}", dial_info);
            return Ok(NetworkResult::no_connection_other("connection timeout"));
        };

        veilid_log!(self trace "== get_or_create_connection dial_info={:?}", dial_info);

        // Re-check inside the lock in case another thread connected while we waited
        if let Some(best_existing_conn) = self
            .arc
            .connection_table
            .get_best_connection_by_remote(best_port, remote)
        {
            veilid_log!(self trace
                "== Returning best existing connection {:?}",
                best_existing_conn
            );

            return Ok(NetworkResult::Value(best_existing_conn));
        }

        // Attempt new connection
        let mut retry_count = NEW_CONNECTION_RETRY_COUNT;

        let nres = loop {
            veilid_log!(self trace "== get_or_create_connection connect({}) {:?} -> {}", retry_count, preferred_local_address, dial_info);
            let result_net_res = self
                .network
                .connect(
                    preferred_local_address,
                    dial_info.clone(),
                    self.arc.connection_initial_timeout_ms,
                )
                .await;
            match result_net_res {
                Ok(net_res) => {
                    if net_res.is_value() || retry_count == 0 {
                        // Successful new connection, return it
                        break net_res;
                    }
                }
                Err(e) => {
                    if retry_count == 0 {
                        bail!(
                            "error creating connection: {:?} -> {:?}: {:#?}",
                            preferred_local_address,
                            dial_info,
                            e
                        );
                    }
                }
            };
            retry_count -= 1;

            // // XXX: This should not be necessary
            // // Release the preferred local address if things can't connect due to a low-level collision we dont have a record of
            // preferred_local_address = None;
            sleep(NEW_CONNECTION_RETRY_DELAY_MS).await;
        };

        let prot_conn = network_result_value_or_log!(self target:"network_result", nres => [ format!("== get_or_create_connection failed {:?} -> {}", preferred_local_address, dial_info) ] {
            network_result_raise!(nres);
        });

        // Add to the connection table
        let mut inner = self.arc.inner.lock();
        let inner = match &mut *inner {
            Some(v) => v,
            None => {
                bail!("shutting down");
            }
        };

        self.on_new_protocol_network_connection(inner, prot_conn, Some(dial_info))
    }

    /// Register a flow as relaying through our node
    pub fn add_relaying_flow(&self, flow: Flow) {
        let Ok(_guard) = self.arc.startup_lock.enter() else {
            return;
        };

        self.arc.connection_table.add_priority_flow(flow);
    }

    /// Closes connections whose specific local address is no longer in
    /// `valid_local_addresses`.
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
    pub fn close_dead_connections(&self, valid_local_addresses: &HashSet<Address>) {
        let Ok(_guard) = self.arc.startup_lock.enter() else {
            return;
        };
        let dead_ids = self
            .arc
            .connection_table
            .get_dead_connection_ids(valid_local_addresses);
        if dead_ids.is_empty() {
            return;
        }
        veilid_log!(self debug "closing {} dead connections after interface change", dead_ids.len());
        for id in dead_ids {
            self.report_connection_finished(id);
        }
    }

    /// Closes connections whose specific local address is no longer in
    /// `valid_local_addresses`.
    #[cfg_attr(
        not(all(target_arch = "wasm32", target_os = "unknown")),
        expect(dead_code)
    )]
    pub fn close_all_connections(&self) {
        let Ok(_guard) = self.arc.startup_lock.enter() else {
            return;
        };
        let dead_ids = self.arc.connection_table.get_all_connection_ids();
        if dead_ids.is_empty() {
            return;
        }
        veilid_log!(self debug "closing {} connections after interface change", dead_ids.len());
        for id in dead_ids {
            self.report_connection_finished(id);
        }
    }

    ///////////////////////////////////////////////////////////////////////////////////////////////////////
    // Asynchronous Event Processor

    async fn process_connection_manager_event(
        &self,
        event: ConnectionManagerEvent,
        allow_accept: bool,
    ) {
        match event {
            ConnectionManagerEvent::Accepted(prot_conn, _address_lock_guard) => {
                if !allow_accept {
                    return;
                }
                let Ok(_guard) = self.arc.startup_lock.enter() else {
                    return;
                };

                let mut inner = self.arc.inner.lock();
                match &mut *inner {
                    Some(inner) => {
                        // Register the connection
                        // We don't care if this fails, since nobody here asked for the inbound connection.
                        // If it does, we just drop the connection

                        let _ = self.on_new_protocol_network_connection(inner, prot_conn, None);
                    }
                    None => {
                        // If this somehow happens, we're shutting down
                    }
                };
            }
            ConnectionManagerEvent::Dead(mut conn) => {
                // No address lock: the connection is already out of the table, and waiting
                // here can deadlock behind a queued Accepted event holding the same tag
                conn.close();
                conn.await;
            }
        }
    }

    async fn async_processor(
        self,
        stop_token: StopToken,
        receiver: flume::Receiver<ConnectionManagerEvent>,
    ) {
        // Process async commands
        while let Ok(Ok(event)) = receiver.recv_async().timeout_at(stop_token.clone()).await {
            self.process_connection_manager_event(event, true).await;
        }
        // Ensure receiver is drained completely
        for event in receiver.drain() {
            self.process_connection_manager_event(event, false).await;
        }
    }

    // Acquire the per-remote address lock for an inbound low-level connection before its protocol
    // is classified, so a concurrent outbound to the same remote waits and reuses. Threaded into
    // the Accepted event and held until the connection is registered. None on timeout.
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
    pub(super) async fn acquire_inbound_address_lock(
        &self,
        remote: SocketAddr,
    ) -> Option<AddressLockGuard> {
        timeout(
            self.arc.connection_initial_timeout_ms,
            self.arc.address_lock_table.lock_tag(remote),
        )
        .await
        .ok()
    }

    // Called by low-level network when any connection-oriented protocol connection appears
    // either from incoming connections.
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
    pub(super) async fn on_accepted_protocol_network_connection(
        &self,
        protocol_connection: ProtocolNetworkConnection,
        address_lock_guard: AddressLockGuard,
    ) -> EyreResult<()> {
        // Get channel sender
        let sender = {
            let mut inner = self.arc.inner.lock();
            let inner = match &mut *inner {
                Some(v) => v,
                None => {
                    // If we are shutting down, just drop this and return
                    return Ok(());
                }
            };
            inner.sender.clone()
        };

        // Inform the processor of the event
        let _ = sender
            .send_async(ConnectionManagerEvent::Accepted(
                protocol_connection,
                address_lock_guard,
            ))
            .await;
        Ok(())
    }

    // Callback from network connection receive loop when it exits
    // cleans up the entry in the connection table
    pub(super) fn report_connection_finished(&self, connection_id: NetworkConnectionId) {
        // Get channel sender
        let sender = {
            let mut inner = self.arc.inner.lock();
            let inner = match &mut *inner {
                Some(v) => v,
                None => {
                    // If we are shutting down, just drop this and return
                    return;
                }
            };
            inner.sender.clone()
        };

        // Remove the connection
        let conn = self
            .arc
            .connection_table
            .remove_connection_by_id(connection_id);

        // Inform the processor of the event
        if let Some(conn) = conn {
            // If the connection closed while it was protected, report it on the node the connection was established on
            // In-use connections will already get reported because they will cause a 'lost_question' stat on the remote node
            if let Some(protect_nr) = conn.protected_node_ref() {
                // Lifetime: only counts remote-closed protected connections.
                // Skip recording when we're offline ourselves so peer-side
                // stats don't get poisoned by our own connectivity loss.
                let cur_ts = Timestamp::now_non_decreasing();
                let lifetime = cur_ts.duration_since(conn.established_time());
                let we_are_online = self
                    .network_manager()
                    .online_detector()
                    .online_state(RoutingDomain::PublicInternet)
                    == OnlineState::Online;
                if we_are_online {
                    protect_nr.record_protected_connection_drop(lifetime);
                    if let Some(penalty) = self
                        .arc
                        .connection_flap_detector
                        .lock()
                        .record_event(cur_ts.as_u64())
                    {
                        veilid_log!(self debugwarn "protected connection FLAPPING (penalty={:.1}): node {}", penalty, protect_nr);
                    }
                }

                if let Some(inner) = self.arc.inner.lock().as_mut() {
                    for pa in inner.protected_addresses.values_mut() {
                        if pa.node_ref.same_entry(&protect_nr) {
                            let duration = cur_ts.duration_since(pa.span_start_ts);

                            let mut reconnect = true;

                            if duration < PROTECTED_CONNECTION_DROP_SPAN {
                                pa.drops_in_span += 1;
                                veilid_log!(self debug "== Protected connection dropped (count={}, lifetime={}): {} -> {} for node {}", pa.drops_in_span, lifetime, conn.connection_id(), conn.debug_print(Timestamp::now()), protect_nr);

                                if pa.drops_in_span >= PROTECTED_CONNECTION_DROP_COUNT {
                                    protect_nr.record_protected_connection_dead(
                                        conn.flow().transport_type(),
                                    );
                                    reconnect = false;

                                    pa.drops_in_span = 0;
                                    pa.span_start_ts = cur_ts;
                                }
                            } else {
                                pa.drops_in_span = 1;
                                pa.span_start_ts = cur_ts;

                                veilid_log!(self debug "== Protected connection dropped (count={}, lifetime={}): {} -> {} for node {}", pa.drops_in_span, lifetime, conn.connection_id(), conn.debug_print(Timestamp::now()), protect_nr);
                            }

                            // Reconnect the protected connection immediately
                            if reconnect {
                                if let Some(dial_info) = conn.dial_info() {
                                    self.spawn_reconnector(dial_info);
                                } else {
                                    veilid_log!(self debug "Can't reconnect to accepted protected connection: {} -> {} for node {}", conn.connection_id(), conn.debug_print(Timestamp::now()), protect_nr);
                                }
                            }

                            break;
                        }
                    }
                }
            }
            let _ = sender.send(ConnectionManagerEvent::Dead(Box::new(conn)));
        }

        // If there are no connections left in the table, we may have switched networks and killed off all the connections
        if self.arc.connection_table.connection_count() == 0 {
            veilid_log!(self debug "Connection table is empty, triggering dial info confirmation");
            self.network_manager()
                .net()
                .routing_domain_request_confirm_dial_info(RoutingDomain::PublicInternet);
        }
    }

    fn spawn_reconnector(&self, dial_info: DialInfo) {
        let this = self.clone();
        self.arc.reconnection_processor.add_future(
                Box::pin(async move {
                    match this.get_or_create_connection(dial_info.clone()).await {
                        Ok(NetworkResult::Value(conn)) => {
                            veilid_log!(this debug "Reconnection successful to {}: {:?}", dial_info,conn);
                        }
                        Ok(res) => {
                            veilid_log!(this debug "Reconnection unsuccessful to {}: {:?}", dial_info, res);
                        }
                        Err(e) => {
                            veilid_log!(this debug "Reconnection error to {}: {}", dial_info, e);
                        }
                    };
                }));
    }

    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub fn debug_print(&self) -> String {
        format!(
            "Connection Table:\n{}",
            self.arc.connection_table.debug_print_table()
        )
    }
}
