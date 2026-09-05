use super::*;
use futures_util::StreamExt;
use hashlink::LruCache;

impl_veilid_log_facility!("net");

/// Allow 25% of the table size to be occupied by priority flows
/// that will not be subject to LRU termination.
const PRIORITY_FLOW_PERCENTAGE: usize = 25;

///////////////////////////////////////////////////////////////////////////////
#[derive(ThisError, Debug)]
pub enum ConnectionTableAddError {
    #[error("Connection already added to table")]
    AlreadyExists(Box<NetworkConnection>),
    #[error("Connection address was filtered")]
    AddressFilter(Box<NetworkConnection>, AddConnectionError),
    #[error("Connection table is full")]
    TableFull(Box<NetworkConnection>),
}

impl ConnectionTableAddError {
    pub fn already_exists(conn: NetworkConnection) -> Self {
        ConnectionTableAddError::AlreadyExists(Box::new(conn))
    }
    pub fn address_filter(conn: NetworkConnection, err: AddConnectionError) -> Self {
        ConnectionTableAddError::AddressFilter(Box::new(conn), err)
    }
    pub fn table_full(conn: NetworkConnection) -> Self {
        ConnectionTableAddError::TableFull(Box::new(conn))
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ConnectionRefKind {
    AddRef,
    RemoveRef,
}

/// Connection-table remote index key: low-level protocol type + socket address
pub(super) type LowLevelProtocolAddress = (LowLevelProtocolType, SocketAddress);

///////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
struct ConnectionTableInner {
    // Maximum total connections across all protocol types
    max_connections: usize,
    // All connections in one LRU so eviction is global, not per-protocol
    conn_by_id: LruCache<NetworkConnectionId, NetworkConnection>,
    id_by_flow: BTreeMap<Flow, NetworkConnectionId>,
    ids_by_remote: BTreeMap<LowLevelProtocolAddress, Vec<NetworkConnectionId>>,
    // Priority flows, kept from LRU termination; capped at a percentage of max_connections
    priority_flows: LruCache<Flow, ()>,
}

#[derive(Debug)]
pub struct ConnectionTable {
    registry: VeilidComponentRegistry,
    inner: Mutex<ConnectionTableInner>,
}

impl_veilid_component_accessors!(ConnectionTable);

impl ConnectionTable {
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        let config = registry.config();
        let max_connections = config.network.max_connections as usize;

        Self {
            registry,
            inner: Mutex::new(ConnectionTableInner {
                max_connections,
                conn_by_id: LruCache::new_unbounded(),
                id_by_flow: BTreeMap::new(),
                ids_by_remote: BTreeMap::new(),
                priority_flows: LruCache::new(max_connections * PRIORITY_FLOW_PERCENTAGE / 100),
            }),
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn join(&self) {
        let mut unord = {
            let mut inner = self.inner.lock();
            let unord = FuturesUnordered::new();
            for (_, mut v) in inner.conn_by_id.drain() {
                veilid_log!(self trace "connection table join: {:?}", v);
                v.close();
                unord.push(v);
            }
            inner.id_by_flow.clear();
            inner.ids_by_remote.clear();
            unord
        };

        while unord.next().await.is_some() {}
    }

    /// Add a priority flow, which is protected from eviction but without the
    /// punishment expectations of a fully 'protected' connection.
    /// This is an LRU set, so there is no removing the flows by hand, and
    /// they are kept in a 'best effort' fashion.
    /// If connections 'should' stay alive, use this mechanism.
    /// If connections 'must' stay alive, use 'NetworkConnection::protect'.
    pub fn add_priority_flow(&self, flow: Flow) {
        let mut inner = self.inner.lock();
        inner.priority_flows.insert(flow, ());
    }

    /// The mechanism for selecting which connections get evicted from the connection table
    /// when it is getting full while adding a new connection.
    /// Factored out into its own function for clarity.
    fn lru_out_connection_inner(
        &self,
        inner: &mut ConnectionTableInner,
    ) -> Result<Option<NetworkConnection>, ()> {
        // The cap is on the total connection count, so if we're under the
        // maximum nothing needs to be LRUd out.
        if inner.conn_by_id.len() < inner.max_connections {
            return Ok(None);
        }

        // Find the globally least-recently-used free connection to make room.
        let dead_k = {
            let Some(lruk) = inner.conn_by_id.iter().find_map(|(k, v)| {
                // Ensure anything being LRU evicted isn't protected somehow
                // 1. connections that are 'in-use' are kept
                // 2. connections with flows in the priority list are kept
                // 3. connections that are protected are kept
                if !v.is_in_use()
                    && !inner.priority_flows.contains_key(&v.flow())
                    && v.protected_node_ref().is_none()
                {
                    Some(*k)
                } else {
                    None
                }
            }) else {
                // Can't make room, connection table is full
                return Err(());
            };
            lruk
        };

        let dead_conn = self.remove_connection_records_inner(inner, dead_k);
        Ok(Some(dead_conn))
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), ret, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn add_connection(
        &self,
        network_connection: NetworkConnection,
    ) -> Result<Option<NetworkConnection>, ConnectionTableAddError> {
        // Get indices for network connection table
        let id = network_connection.connection_id();
        let flow = network_connection.flow();
        let remote = (
            flow.protocol_type().low_level_protocol_type(),
            *flow.remote_address(),
        );

        let mut inner = self.inner.lock();

        // Two connections to the same flow should be rejected (soft rejection)
        if inner.id_by_flow.contains_key(&flow) {
            return Err(ConnectionTableAddError::already_exists(network_connection));
        }

        // Reject duplicates that would invalidate the table representation
        if inner.conn_by_id.contains_key(&id) {
            veilid_log!(self error "duplicate connection id: {:#?}", network_connection);
            return Err(ConnectionTableAddError::already_exists(network_connection));
        }
        if inner
            .ids_by_remote
            .get(&remote)
            .is_some_and(|ids| ids.contains(&id))
        {
            veilid_log!(self error "duplicate id by remote: {:#?}", network_connection);
            return Err(ConnectionTableAddError::already_exists(network_connection));
        }

        // Filter by ip for connection limits
        let ip_addr = flow.remote_address().ip_addr();
        if let Err(e) = self
            .network_manager()
            .address_filter()
            .add_connection(ip_addr)
        {
            // Return the connection in the error to be disposed of
            return Err(ConnectionTableAddError::address_filter(
                network_connection,
                e,
            ));
        }

        // if we have reached the maximum total number of connections
        // then drop the least recently used connection that is not protected or referenced
        let out_conn = match self.lru_out_connection_inner(&mut inner) {
            Ok(v) => v,
            Err(()) => {
                return Err(ConnectionTableAddError::table_full(network_connection));
            }
        };

        // Add the connection to the table
        let res = inner.conn_by_id.insert(id, network_connection);
        debug_assert!(res.is_none());

        // add connection records
        inner.id_by_flow.insert(flow, id);
        inner.ids_by_remote.entry(remote).or_default().push(id);

        Ok(out_conn)
    }

    //#[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), ret, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn peek_connection_by_flow(&self, flow: Flow) -> Option<ConnectionHandle> {
        if flow.protocol_type() == ProtocolType::UDP {
            return None;
        }

        let inner = self.inner.lock();

        let id = *inner.id_by_flow.get(&flow)?;
        let out = inner.conn_by_id.peek(&id).unwrap_or_log();
        if out.is_dead() {
            return None;
        }
        Some(out.get_handle())
    }

    //#[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), ret, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn touch_connection_by_id(&self, id: NetworkConnectionId) {
        let mut inner = self.inner.lock();
        let _ = inner.conn_by_id.get(&id);
    }

    #[expect(dead_code)]
    pub fn with_connection_by_flow<R, F: FnOnce(&NetworkConnection) -> R>(
        &self,
        flow: Flow,
        closure: F,
    ) -> Option<R> {
        if flow.protocol_type() == ProtocolType::UDP {
            return None;
        }

        let inner = self.inner.lock();

        let id = *inner.id_by_flow.get(&flow)?;
        let out = inner.conn_by_id.peek(&id).unwrap_or_log();
        Some(closure(out))
    }

    #[expect(dead_code)]
    pub fn with_connection_by_flow_mut<R, F: FnOnce(&mut NetworkConnection) -> R>(
        &self,
        flow: Flow,
        closure: F,
    ) -> Option<R> {
        if flow.protocol_type() == ProtocolType::UDP {
            return None;
        }

        let mut inner = self.inner.lock();

        let id = *inner.id_by_flow.get(&flow)?;
        let out = inner.conn_by_id.peek_mut(&id).unwrap_or_log();
        Some(closure(out))
    }

    pub fn with_all_connections_mut<R, F: FnMut(&mut NetworkConnection) -> Option<R>>(
        &self,
        mut closure: F,
    ) -> Option<R> {
        let mut inner = self.inner.lock();
        for (_id, conn) in inner.conn_by_id.iter_mut() {
            if let Some(out) = closure(conn) {
                return Some(out);
            }
        }
        None
    }

    //#[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), ret, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn ref_connection_by_id(
        &self,
        id: NetworkConnectionId,
        ref_type: ConnectionRefKind,
    ) -> bool {
        let mut inner = self.inner.lock();
        let Some(out) = inner.conn_by_id.get_mut(&id) else {
            // Sometimes network connections die before we can ref/unref them
            return false;
        };
        match ref_type {
            ConnectionRefKind::AddRef => out.add_ref(),
            ConnectionRefKind::RemoveRef => out.remove_ref(),
        }
        true
    }

    // #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), ret, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn get_best_connection_by_remote(
        &self,
        best_port: Option<u16>,
        remote: LowLevelProtocolAddress,
    ) -> Option<ConnectionHandle> {
        let inner = &mut *self.inner.lock();

        // Skip dead connections; their processor loop has exited and sends would not be drained
        let live_ids = inner
            .ids_by_remote
            .get(&remote)?
            .iter()
            .copied()
            .filter(|id| !inner.conn_by_id.peek(id).unwrap_or_log().is_dead())
            .collect::<Vec<_>>();

        if live_ids.is_empty() {
            // no connections
            return None;
        }
        if live_ids.len() == 1 {
            // only one connection
            let nc = inner.conn_by_id.get(&live_ids[0]).unwrap_or_log();
            return Some(nc.get_handle());
        }
        // multiple connections, find the one that matches the best port, or the most recent
        if let Some(best_port) = best_port {
            for id in &live_ids {
                let nc = inner.conn_by_id.peek(id).unwrap_or_log();
                if let Some(local_addr) = nc.flow().local() {
                    if local_addr.port() == best_port {
                        let nc = inner.conn_by_id.get(id).unwrap_or_log();
                        return Some(nc.get_handle());
                    }
                }
            }
        }
        // just return most recent network connection if a best port match can not be found
        let best_id = *live_ids.last().unwrap_or_log();
        let nc = inner.conn_by_id.get(&best_id).unwrap_or_log();
        Some(nc.get_handle())
    }

    //#[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), ret, fields(__VEILID_LOG_KEY = self.log_key())))]
    #[expect(dead_code)]
    pub fn get_connection_ids_by_remote(
        &self,
        remote: LowLevelProtocolAddress,
    ) -> Vec<NetworkConnectionId> {
        let inner = self.inner.lock();
        inner
            .ids_by_remote
            .get(&remote)
            .cloned()
            .unwrap_or_default()
    }

    // pub fn drain_filter<F>(&self, mut filter: F) -> Vec<NetworkConnection>
    // where
    //     F: FnMut(Flow) -> bool,
    // {
    //     let mut inner = self.inner.lock();
    //     let mut filtered_ids = Vec::new();
    //     for cbi in &mut inner.conn_by_id {
    //         for (id, conn) in cbi {
    //             if filter(conn.flow()) {
    //                 filtered_ids.push(*id);
    //             }
    //         }
    //     }
    //     let mut filtered_connections = Vec::new();
    //     for id in filtered_ids {
    //         let conn = Self::remove_connection_records(&mut *inner, id);
    //         filtered_connections.push(conn)
    //     }
    //     filtered_connections
    // }

    pub fn connection_count(&self) -> usize {
        let inner = self.inner.lock();
        inner.conn_by_id.len()
    }

    /// Returns ids of connections whose specific (non-wildcard, non-loopback) local address
    /// is no longer in `valid_local_addresses`.
    pub fn get_dead_connection_ids(
        &self,
        valid_local_addresses: &HashSet<Address>,
    ) -> Vec<NetworkConnectionId> {
        let inner = self.inner.lock();
        let mut out = Vec::new();
        for (id, conn) in inner.conn_by_id.iter() {
            let Some(local) = conn.flow().local() else {
                continue;
            };
            let addr = local.address();
            if addr.is_unspecified() || addr.ip_addr().is_loopback() {
                continue;
            }
            if !valid_local_addresses.contains(&addr) {
                out.push(*id);
            }
        }
        out
    }

    /// Returns ids of all connections.
    pub fn get_all_connection_ids(&self) -> Vec<NetworkConnectionId> {
        let inner = self.inner.lock();
        let mut out = Vec::new();
        for (id, _conn) in inner.conn_by_id.iter() {
            out.push(*id);
        }
        out
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(inner), ret, fields(__VEILID_LOG_KEY = self.log_key())))]
    fn remove_connection_records_inner(
        &self,
        inner: &mut ConnectionTableInner,
        id: NetworkConnectionId,
    ) -> NetworkConnection {
        // conn_by_id
        let conn = inner.conn_by_id.remove(&id).unwrap_or_log();
        // id_by_flow
        let flow = conn.flow();
        let _ = inner
            .id_by_flow
            .remove(&flow)
            .expect_or_log("must have removed something here");
        // ids_by_remote
        let remote = (
            flow.protocol_type().low_level_protocol_type(),
            *flow.remote_address(),
        );
        let ids = inner.ids_by_remote.get_mut(&remote).unwrap_or_log();
        for (n, elem) in ids.iter().enumerate() {
            if *elem == id {
                let _ = ids.remove(n);
                if ids.is_empty() {
                    inner.ids_by_remote.remove(&remote).unwrap_or_log();
                }
                break;
            }
        }
        // priority_flows
        inner.priority_flows.remove(&flow);
        // address_filter
        let ip_addr = flow.remote().socket_addr().ip();
        self.network_manager()
            .address_filter()
            .remove_connection(ip_addr)
            .expect_or_log("Inconsistency in connection table");
        conn
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), ret, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn remove_connection_by_id(&self, id: NetworkConnectionId) -> Option<NetworkConnection> {
        let mut inner = self.inner.lock();

        if !inner.conn_by_id.contains_key(&id) {
            return None;
        }
        let conn = self.remove_connection_records_inner(&mut inner, id);
        Some(conn)
    }

    pub fn debug_print_table(&self) -> String {
        let mut out = Vec::new();
        let inner = self.inner.lock();
        let cur_ts = Timestamp::now();

        // Per-protocol breakdown of the single total connection table
        out.push(format!(
            "  Connections: ({}/{})",
            inner.conn_by_id.len(),
            inner.max_connections
        ));
        for pt in ProtocolType::all_connection_set() {
            let count = inner
                .conn_by_id
                .iter()
                .filter(|(_, conn)| conn.flow().protocol_type() == pt)
                .count();
            out.push(format!("    {pt}: {count}"));
        }

        for (_, conn) in inner.conn_by_id.iter() {
            let is_priority_flow = inner.priority_flows.contains_key(&conn.flow());
            out.push(format!(
                "    {}{}",
                conn.debug_print(cur_ts),
                if is_priority_flow { " PRIORITY" } else { "" }
            ));
        }

        out.push(format!(
            "  Priority Flows: ({}/{})",
            inner.priority_flows.len(),
            inner.priority_flows.capacity(),
        ));
        for (flow, _) in inner.priority_flows.iter() {
            out.push(format!("    {flow}"));
        }
        out.join("\n")
    }
}
