use super::*;

mod address_check;
mod address_filter;
mod bootstrap;
mod connection_handle;
mod connection_manager;
mod connection_table;
mod debug;
mod network;
mod network_connection;
mod node_contact_method_cache;
mod online_detector;
mod receipt_manager;
mod relay_worker;
mod send_data;
mod stats;
mod tasks;
mod types;

#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod tests_network_manager;

////////////////////////////////////////////////////////////////////////////////////////

pub use connection_manager::*;
pub use network::*;
pub use network_connection::*;
pub use receipt_manager::*;
pub use stats::*;

pub(crate) use bootstrap::*;
pub(crate) use node_contact_method_cache::*;
pub(crate) use online_detector::*;
pub(crate) use types::*;

////////////////////////////////////////////////////////////////////////////////////////
use address_check::*;
use address_filter::*;
use connection_handle::*;
use crypto::*;
use relay_worker::*;
use routing_table::*;
use rpc_processor::*;

////////////////////////////////////////////////////////////////////////////////////////

impl_veilid_log_facility!("net");

/// Bootstrap v0 FOURCC
pub const BOOT_MAGIC: &[u8; 4] = b"BOOT";
/// Bootstrap v1 FOURCC
pub const B01T_MAGIC: &[u8; 4] = b"B01T";
/// Cache size for TXT lookups used by bootstrap
pub const TXT_LOOKUP_CACHE_SIZE: usize = 256;
/// Duration that TXT lookups are valid in the cache (5 minutes, <= the DNS record expiration timeout)
pub const TXT_LOOKUP_EXPIRATION: TimestampDuration = TimestampDuration::new_secs(300);
/// Maximum size for a message is the same as the maximum size for an Envelope
pub const MAX_MESSAGE_SIZE: usize = ENV0_MAX_ENVELOPE_SIZE;
/// Statistics table size for tracking performance by IP address
pub const IPADDR_TABLE_SIZE: usize = 1024;
/// Eviction time for ip addresses from statistics tables (5 minutes)
pub const IPADDR_MAX_INACTIVE_DURATION: TimestampDuration = TimestampDuration::new_secs(300);
/// How frequently to process adddress filter background tasks
pub const ADDRESS_FILTER_TASK_INTERVAL_SECS: u32 = 60;
/// Delay between hole punch operations to improve likelihood of seqential state processing
pub const HOLE_PUNCH_DELAY_MS: u32 = 100;
/// Number of rpc relay operations that can be handles simultaneously
pub const RELAY_WORKERS_PER_CORE: u32 = 16;

/// Things we get when we start up and go away when we shut down
/// Routing table is not in here because we want it to survive a network shutdown/startup restart
#[derive(Clone)]
struct NetworkComponents {
    net: Network,
    connection_manager: ConnectionManager,
    receipt_manager: ReceiptManager,
}

#[derive(Debug)]
struct ClientAllowlistEntry {
    last_seen_ts: Timestamp,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StartupDisposition {
    Success,
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
    BindRetry,
}

#[derive(Debug, Clone)]
pub struct NetworkManagerStartupContext {
    pub startup_lock: Arc<StartupLock>,
}
impl NetworkManagerStartupContext {
    pub fn new() -> Self {
        Self {
            startup_lock: Arc::new(StartupLock::new()),
        }
    }
}
impl Default for NetworkManagerStartupContext {
    fn default() -> Self {
        Self::new()
    }
}
// The mutable state of the network manager
#[derive(Debug)]
struct NetworkManagerInner {
    client_allowlist: hashlink::LruCache<NodeId, ClientAllowlistEntry>,
    node_contact_method_cache: NodeContactMethodCache,
    address_check: Option<AddressCheck>,
    tick_subscription: Option<EventBusSubscription>,
    peer_info_change_subscription: Option<EventBusSubscription>,
    socket_address_change_subscription: Option<EventBusSubscription>,

    // TXT lookup cache
    txt_lookup_cache: hashlink::LruCache<String, (Timestamp, Vec<String>)>,

    // Relay workers
    relay_stop_source: Option<StopSource>,
    relay_send_channel: Option<flume::Sender<RelayWorkerRequest>>,
    relay_worker_join_handles: Vec<MustJoinHandle<()>>,
}

pub(crate) struct NetworkManager {
    registry: VeilidComponentRegistry,
    inner: Mutex<NetworkManagerInner>,

    // Aggregate network transfer stats, shared so they can be broadcast cheaply
    stats: Arc<RwLock<NetworkManagerStats>>,

    // Address filter
    address_filter: AddressFilter,

    // Per-routing-domain online detector
    online_detector: OnlineDetector,

    // Accessors
    components: RwLock<Option<NetworkComponents>>,

    // Background processes
    rolling_transfers_task: TickTask<EyreReport>,
    address_filter_task: TickTask<EyreReport>,

    // Network key
    network_key: Option<BareSharedSecret>,

    // Startup context
    startup_context: NetworkManagerStartupContext,

    // Relay workers config
    concurrency: u32,
    queue_size: u32,
}

impl_veilid_component!(NetworkManager);

impl fmt::Debug for NetworkManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetworkManager")
            //.field("registry", &self.registry)
            .field("inner", &self.inner)
            .field("address_filter", &self.address_filter)
            .field("online_detector", &self.online_detector)
            .field("network_key", &self.network_key)
            .field("startup_context", &self.startup_context)
            .field("concurrency", &self.concurrency)
            .field("queue_size", &self.queue_size)
            .finish()
    }
}

impl NetworkManager {
    fn new_inner() -> NetworkManagerInner {
        NetworkManagerInner {
            client_allowlist: hashlink::LruCache::new_unbounded(),
            node_contact_method_cache: NodeContactMethodCache::new(),
            address_check: None,
            tick_subscription: None,
            peer_info_change_subscription: None,
            socket_address_change_subscription: None,
            txt_lookup_cache: hashlink::LruCache::new(TXT_LOOKUP_CACHE_SIZE),
            //
            relay_send_channel: None,
            relay_stop_source: None,
            relay_worker_join_handles: Vec::new(),
        }
    }

    pub fn new(
        registry: VeilidComponentRegistry,
        startup_context: NetworkManagerStartupContext,
    ) -> Self {
        // Make the network key
        let network_key = {
            let config = registry.config();
            let crypto = registry.crypto();

            let network_key_password = config.network.network_key_password.clone();
            let network_key = if let Some(network_key_password) = network_key_password {
                if !network_key_password.is_empty() {
                    veilid_log!(registry info "Using network key");

                    let bcs = crypto.best();
                    // Yes the use of the salt this way is generally bad, but this just needs to be hashed
                    Some(
                        bcs.derive_shared_secret(
                            network_key_password.as_bytes(),
                            bcs.generate_hash(network_key_password.as_bytes())
                                .ref_value(),
                        )
                        .expect_or_log("failed to derive network key")
                        .value(),
                    )
                } else {
                    None
                }
            } else {
                None
            };

            network_key
        };

        // make local copy of node id for easy access
        let (concurrency, queue_size) = {
            let config = registry.config();

            // set up channel
            let mut concurrency = config.internal().network.rpc.concurrency;
            let queue_size = config.internal().network.rpc.queue_size;
            if concurrency == 0 {
                concurrency = get_concurrency();
                if concurrency == 0 {
                    concurrency = 1;
                }

                // Default relay concurrency is the number of CPUs * 16 relay workers per core
                concurrency *= RELAY_WORKERS_PER_CORE;
            }
            (concurrency, queue_size)
        };

        let inner = Self::new_inner();
        let address_filter = AddressFilter::new(registry.clone());
        let online_detector = OnlineDetector::new(registry.clone());

        let this = Self {
            registry,
            inner: Mutex::new(inner),
            stats: Arc::new(RwLock::new(NetworkManagerStats::default())),
            address_filter,
            online_detector,
            components: RwLock::new(None),
            rolling_transfers_task: TickTask::new(
                "rolling_transfers_task",
                ROLLING_TRANSFERS_INTERVAL_SECS,
            ),
            address_filter_task: TickTask::new(
                "address_filter_task",
                ADDRESS_FILTER_TASK_INTERVAL_SECS,
            ),
            network_key,
            startup_context,
            concurrency,
            queue_size,
        };

        this.setup_tasks();

        this
    }

    pub fn address_filter(&self) -> &AddressFilter {
        &self.address_filter
    }

    pub fn online_detector(&self) -> &OnlineDetector {
        &self.online_detector
    }

    fn net(&self) -> Network {
        self.components.read().as_ref().unwrap_or_log().net.clone()
    }

    fn opt_net(&self) -> Option<Network> {
        self.components.read().as_ref().map(|x| x.net.clone())
    }

    fn receipt_manager(&self) -> ReceiptManager {
        self.components
            .read()
            .as_ref()
            .unwrap_or_log()
            .receipt_manager
            .clone()
    }

    pub fn connection_manager(&self) -> ConnectionManager {
        self.components
            .read()
            .as_ref()
            .unwrap_or_log()
            .connection_manager
            .clone()
    }
    pub fn opt_connection_manager(&self) -> Option<ConnectionManager> {
        self.components
            .read()
            .as_ref()
            .map(|x| x.connection_manager.clone())
    }

    fn log_facilities_impl(&self) -> VeilidComponentLogFacilities {
        VeilidComponentLogFacilities::new()
            .with_facility(
                VeilidComponentLogFacility::try_new_with_tags("net", ["#common"]).unwrap(),
            )
            .with_facility(VeilidComponentLogFacility::try_new("protocol").unwrap())
            .with_facility(VeilidComponentLogFacility::try_new("receipt").unwrap())
            .with_facility(
                VeilidComponentLogFacility::try_new_with_tags("network_result", ["#verbose"])
                    .unwrap(),
            )
            .with_facility(
                VeilidComponentLogFacility::try_new_with_tags("contact_method", ["#verbose"])
                    .unwrap(),
            )
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key())))]
    #[allow(clippy::unused_async)]
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
        if !self.startup_context.startup_lock.is_shut_down() {
            veilid_log!(self error "network manager should have shut down by now");
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    #[allow(clippy::unused_async)]
    async fn terminate_async(&self) {}

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn internal_startup(&self) -> EyreResult<StartupDisposition> {
        if self.components.read().is_some() {
            veilid_log!(self debug "NetworkManager::internal_startup already started");
            return Ok(StartupDisposition::Success);
        }

        // Clean address filter for things that should not be persistent
        self.address_filter.restart();

        // Re-arm online detector (offline/online history persists across detach)
        self.online_detector.startup();

        // Create network components
        let net = new_platform_network(self.registry());
        let connection_manager = ConnectionManager::new(net.clone());
        let receipt_manager = ReceiptManager::new(self.registry());

        *self.components.write() = Some(NetworkComponents {
            net: net.clone(),
            connection_manager: connection_manager.clone(),
            receipt_manager: receipt_manager.clone(),
        });

        // Startup relay workers
        self.startup_relay_workers()?;

        // Start network components
        connection_manager.startup()?;
        match net.startup().await? {
            StartupDisposition::Success => {}
            StartupDisposition::BindRetry => {
                return Ok(StartupDisposition::BindRetry);
            }
        }

        // Set up address filter
        {
            let mut inner = self.inner.lock();
            let address_check = AddressCheck::new(net.clone());
            inner.address_check = Some(address_check);
        }

        receipt_manager.startup()?;

        // Register event handlers
        let tick_subscription = impl_subscribe_event_bus_async!(self, Self, tick_event_handler);

        let peer_info_change_subscription =
            impl_subscribe_event_bus!(self, Self, peer_info_change_event_handler);

        let socket_address_change_subscription =
            impl_subscribe_event_bus!(self, Self, socket_address_change_event_handler);

        {
            let mut inner = self.inner.lock();
            inner.tick_subscription = Some(tick_subscription);
            inner.peer_info_change_subscription = Some(peer_info_change_subscription);
            inner.socket_address_change_subscription = Some(socket_address_change_subscription);
        }

        veilid_log!(self trace "NetworkManager::internal_startup end");

        Ok(StartupDisposition::Success)
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn startup(&self) -> EyreResult<StartupDisposition> {
        let guard = self.startup_context.startup_lock.startup()?;

        match self.internal_startup().await {
            Ok(StartupDisposition::Success) => {
                guard.success();
                Ok(StartupDisposition::Success)
            }
            Ok(StartupDisposition::BindRetry) => {
                self.shutdown_internal().await;
                Ok(StartupDisposition::BindRetry)
            }
            Err(e) => {
                self.shutdown_internal().await;
                Err(e)
            }
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    async fn shutdown_internal(&self) {
        // Shutdown event bus subscriptions and address check
        {
            let mut inner = self.inner.lock();
            if let Some(sub) = inner.tick_subscription.take() {
                self.event_bus().unsubscribe(sub);
            }
            if let Some(sub) = inner.socket_address_change_subscription.take() {
                self.event_bus().unsubscribe(sub);
            }
            if let Some(sub) = inner.peer_info_change_subscription.take() {
                self.event_bus().unsubscribe(sub);
            }
        }

        // Cancel all tasks
        veilid_log!(self debug "stopping network manager tasks");
        self.cancel_tasks().await;

        // Shutdown relay workers
        self.shutdown_relay_workers().await;

        // Shutdown network components if they started up
        veilid_log!(self debug "shutting down network components");

        {
            let components = self.components.read().clone();
            if let Some(components) = components {
                components.net.shutdown().await;

                {
                    let mut inner = self.inner.lock();
                    inner.address_check = None;
                }

                components.receipt_manager.shutdown().await;
                components.connection_manager.shutdown().await;
            }
        }
        *self.components.write() = None;

        // Report detached after components are down: no relay worker or network
        // event can flip the detector back online. State survives for reattach.
        self.online_detector.shutdown();

        // reset the state
        veilid_log!(self debug "resetting network manager state");
        {
            *self.inner.lock() = NetworkManager::new_inner();
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn shutdown(&self) {
        // Proceed with shutdown
        veilid_log!(self debug "starting network manager shutdown");
        let guard = self
            .startup_context
            .startup_lock
            .shutdown()
            .await
            .expect_or_log("should be started up");

        self.shutdown_internal().await;

        guard.success();
        veilid_log!(self debug "finished network manager shutdown");
    }

    #[expect(dead_code)]
    pub fn update_client_allowlist(&self, client: NodeId) {
        let mut inner = self.inner.lock();
        match inner.client_allowlist.entry(client) {
            hashlink::lru_cache::Entry::Occupied(mut entry) => {
                entry.get_mut().last_seen_ts = Timestamp::now_non_decreasing()
            }
            hashlink::lru_cache::Entry::Vacant(entry) => {
                entry.insert(ClientAllowlistEntry {
                    last_seen_ts: Timestamp::now_non_decreasing(),
                });
            }
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), ret, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn check_client_allowlist(&self, client: NodeId) -> bool {
        let mut inner = self.inner.lock();

        match inner.client_allowlist.entry(client) {
            hashlink::lru_cache::Entry::Occupied(mut entry) => {
                entry.get_mut().last_seen_ts = Timestamp::now_non_decreasing();
                true
            }
            hashlink::lru_cache::Entry::Vacant(_) => false,
        }
    }

    pub fn purge_client_allowlist(&self) {
        let timeout_ms = self.config().internal().network.client_allowlist_timeout_ms;
        let mut inner = self.inner.lock();
        let cutoff_timestamp =
            Timestamp::now().earlier(TimestampDuration::new_ms(timeout_ms as u64));
        // Remove clients from the allowlist that haven't been since since our allowlist timeout
        while inner
            .client_allowlist
            .peek_lru()
            .map(|v| v.1.last_seen_ts < cutoff_timestamp)
            .unwrap_or_default()
        {
            let (k, v) = inner.client_allowlist.remove_lru().unwrap_or_log();
            trace!(target: "net", key=?k, value=?v, "purge_client_allowlist: remove_lru")
        }
    }

    pub fn network_needs_restart(&self) -> bool {
        self.opt_net()
            .map(|net| net.needs_restart())
            .unwrap_or(false)
    }

    pub fn network_is_started(&self) -> bool {
        self.opt_net().map(|net| net.is_started()).unwrap_or(false)
    }

    pub fn generate_node_status(&self, _routing_domain: RoutingDomain) -> NodeStatus {
        NodeStatus {}
    }

    /// Generates a multi-shot/normal receipt
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", skip(self, extra_data, callback), fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    #[expect(dead_code)]
    pub async fn generate_receipt(
        &self,
        expiration_duration: TimestampDuration,
        expected_returns: u32,
        extra_data: Bytes,
        callback: impl ReceiptCallback,
    ) -> EyreResult<Bytes> {
        let Ok(_guard) = self.startup_context.startup_lock.enter() else {
            bail!("network is not started");
        };
        let receipt_manager = self.receipt_manager();
        let routing_table = self.routing_table();
        let crypto = self.crypto();

        // Generate receipt and serialized form to return
        let vcrypto = crypto.best();

        let nonce = vcrypto.random_nonce();
        let node_id = routing_table.node_id(vcrypto.kind());
        let secret_key = routing_table.secret_key(vcrypto.kind());

        // Encode envelope
        let version = best_receipt_version();
        let receipt = match version {
            RECEIPT_VERSION_RCP0 => {
                Receipt::try_new_rcp0(&crypto, node_id.kind(), nonce, node_id, extra_data)?
            }
            _ => {
                bail!("unsupported receipt version: {:?}", version);
            }
        };

        let out = receipt
            .to_signed_data(&crypto, &secret_key)
            .await
            .wrap_err("failed to generate signed receipt")?;

        // Record the receipt for later
        let exp_ts = Timestamp::now_non_decreasing().later(expiration_duration);
        receipt_manager.record_receipt(receipt, exp_ts, expected_returns, callback);

        Ok(out)
    }

    /// Generates a single-shot/normal receipt
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", skip(self, extra_data), fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn generate_single_shot_receipt(
        &self,
        expiration_duration: TimestampDuration,
        extra_data: Bytes,
    ) -> EyreResult<(Bytes, EventualValueFuture<ReceiptEvent>)> {
        let Ok(_guard) = self.startup_context.startup_lock.enter() else {
            bail!("network is not started");
        };

        let receipt_manager = self.receipt_manager();
        let routing_table = self.routing_table();
        let crypto = self.crypto();

        // Generate receipt and serialized form to return
        let vcrypto = crypto.best();

        let nonce = vcrypto.random_nonce();
        let node_id = routing_table.node_id(vcrypto.kind());
        let secret_key = routing_table.secret_key(vcrypto.kind());

        let version = best_receipt_version();

        let receipt = match version {
            RECEIPT_VERSION_RCP0 => {
                Receipt::try_new_rcp0(&crypto, node_id.kind(), nonce, node_id, extra_data)?
            }
            _ => {
                bail!("unsupported receipt version: {:?}", version);
            }
        };

        let out = receipt
            .to_signed_data(&crypto, &secret_key)
            .await
            .wrap_err("failed to generate signed receipt")?;

        // Record the receipt for later
        let exp_ts = Timestamp::now_non_decreasing().later(expiration_duration);
        let eventual = SingleShotEventual::new(Some(ReceiptEvent::Cancelled));
        let instance = eventual.instance();
        receipt_manager.record_single_shot_receipt(receipt, exp_ts, eventual);

        Ok((out, instance))
    }

    /// Process a received out-of-band receipt
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "receipt", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn handle_out_of_band_receipt(&self, receipt_data: Bytes) -> NetworkResult<()> {
        let Ok(_guard) = self.startup_context.startup_lock.enter() else {
            return NetworkResult::service_unavailable("network is not started");
        };

        let receipt_manager = self.receipt_manager();
        let crypto = self.crypto();

        let receipt = match Receipt::try_from_signed_data(&crypto, receipt_data).await {
            Err(e) => {
                return NetworkResult::invalid_message(e.to_string());
            }
            Ok(v) => v,
        };

        receipt_manager
            .handle_receipt(receipt, ReceiptReturned::OutOfBand)
            .await
    }

    /// Process a received in-band receipt
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "receipt", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn handle_in_band_receipt(
        &self,
        receipt_data: Bytes,
        inbound_noderef: FilteredNodeRef,
    ) -> NetworkResult<()> {
        let Ok(_guard) = self.startup_context.startup_lock.enter() else {
            return NetworkResult::service_unavailable("network is not started");
        };

        let receipt_manager = self.receipt_manager();
        let crypto = self.crypto();

        let receipt = match Receipt::try_from_signed_data(&crypto, receipt_data).await {
            Err(e) => {
                return NetworkResult::invalid_message(e.to_string());
            }
            Ok(v) => v,
        };

        receipt_manager
            .handle_receipt(receipt, ReceiptReturned::InBand { inbound_noderef })
            .await
    }

    /// Process a received safety receipt
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "receipt", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn handle_safety_receipt(&self, receipt_data: Bytes) -> NetworkResult<()> {
        let Ok(_guard) = self.startup_context.startup_lock.enter() else {
            return NetworkResult::service_unavailable("network is not started");
        };

        let receipt_manager = self.receipt_manager();
        let crypto = self.crypto();

        let receipt = match Receipt::try_from_signed_data(&crypto, receipt_data).await {
            Err(e) => {
                return NetworkResult::invalid_message(e.to_string());
            }
            Ok(v) => v,
        };

        receipt_manager
            .handle_receipt(receipt, ReceiptReturned::Safety)
            .await
    }

    /// Process a received private receipt
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "receipt", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn handle_private_receipt(
        &self,
        receipt_data: Bytes,
        private_route: PublicKey,
    ) -> NetworkResult<()> {
        let Ok(_guard) = self.startup_context.startup_lock.enter() else {
            return NetworkResult::service_unavailable("network is not started");
        };

        let receipt_manager = self.receipt_manager();
        let crypto = self.crypto();

        let receipt = match Receipt::try_from_signed_data(&crypto, receipt_data).await {
            Err(e) => {
                return NetworkResult::invalid_message(e.to_string());
            }
            Ok(v) => v,
        };

        receipt_manager
            .handle_receipt(receipt, ReceiptReturned::Private { private_route })
            .await
    }

    // Process a received signal
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn handle_signal(
        &self,
        signal_flow: Flow,
        signal_info: SignalInfo,
    ) -> EyreResult<NetworkResult<()>> {
        let Ok(_guard) = self.startup_context.startup_lock.enter() else {
            return Ok(NetworkResult::service_unavailable("network is not started"));
        };

        match signal_info {
            SignalInfo::ReverseConnect { receipt, peer_info } => {
                let routing_table = self.routing_table();
                let rpc = self.rpc_processor();

                // Add the peer info to our routing table
                let mut peer_nr = match routing_table.register_node_with_peer_info(peer_info, false)
                {
                    Ok(nr) => nr,
                    Err(e) => {
                        return Ok(NetworkResult::invalid_message(format!(
                            "unable to register reverse connect peerinfo: {}",
                            e
                        )));
                    }
                };

                // Restrict reverse connection to same sequencing requirement as inbound signal
                let sequencing = signal_flow
                    .protocol_type()
                    .sequence_ordering()
                    .strict_sequencing();
                peer_nr.set_sequencing(sequencing);

                // Make a reverse connection to the peer and send the receipt to it
                rpc.rpc_call_return_receipt(Destination::direct(peer_nr, None), receipt)
                    .await
                    .wrap_err("rpc failure")
            }
            SignalInfo::HolePunch {
                receipt,
                peer_info,
                opt_dial_info,
            } => {
                let routing_table = self.routing_table();
                let rpc = self.rpc_processor();

                // Add the peer info to our routing table
                let mut peer_nr = match routing_table.register_node_with_peer_info(peer_info, false)
                {
                    Ok(nr) => nr,
                    Err(e) => {
                        return Ok(NetworkResult::invalid_message(format!(
                            "unable to register hole punch connect peerinfo: {}",
                            e
                        )));
                    }
                };

                // Get the udp direct dialinfo for the hole punch
                let hole_punch_di = if let Some(hope_punch_di) = opt_dial_info {
                    // Specific dialinfo provided, restrict to this dial info
                    let filter = routing_table
                        .get_outbound_node_ref_filter(RoutingDomain::PublicInternet)
                        .with_dial_info_filter(hope_punch_di.make_filter());
                    peer_nr.set_filter(filter);
                    hope_punch_di
                } else {
                    // No specific dialinfo provided, default to UDP protocol
                    let outbound_nrf = routing_table
                        .get_outbound_node_ref_filter(RoutingDomain::PublicInternet)
                        .with_protocol_type(ProtocolType::UDP);
                    peer_nr.set_filter(outbound_nrf);
                    let Some(hole_punch_dial_info_detail) = peer_nr.first_dial_info_detail() else {
                        return Ok(NetworkResult::no_connection_other(format!(
                            "No hole punch capable dialinfo found for node: {}",
                            peer_nr
                        )));
                    };

                    // Now that we picked a specific dialinfo, further restrict the noderef to the specific address type
                    let filter = peer_nr.take_filter();
                    let filter = filter
                        .with_address_type(hole_punch_dial_info_detail.dial_info.address_type());
                    peer_nr.set_filter(filter);
                    hole_punch_dial_info_detail.dial_info
                };

                // Do our half of the hole punch by sending an empty packet
                // Both sides will do this and then the receipt will get sent over the punched hole
                let unique_flow =
                    network_result_try!(self.net().send_hole_punch(hole_punch_di).await?);

                // Add small delay to encourage packets to be delivered in order
                sleep(HOLE_PUNCH_DELAY_MS).await;

                // Set the hole punch as our 'last connection' to ensure we return the receipt over the direct hole punch
                self.set_last_flow(peer_nr.unfiltered(), unique_flow.flow, Timestamp::now());

                // Return the receipt using the same dial info send the receipt to it
                rpc.rpc_call_return_receipt(Destination::direct(peer_nr, None), receipt)
                    .await
                    .wrap_err("rpc failure")
            }
        }
    }

    /// Builds an envelope for sending over the network
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn build_envelope(
        &self,
        timestamp: Timestamp,
        dest_node_id: NodeId,
        version: EnvelopeVersion,
        body: Bytes,
    ) -> EyreResult<Bytes> {
        let registry = self.registry();
        let network_key = self.network_key.clone();

        let routing_table = registry.routing_table();
        let crypto = registry.crypto();

        // DH to get encryption key
        let Some(vcrypto) = crypto.get_async(dest_node_id.kind()) else {
            bail!("should not have a destination with incompatible crypto here");
        };

        let node_id = routing_table.node_id(vcrypto.kind());
        let secret_key = routing_table.secret_key(vcrypto.kind());

        // Get nonce
        let nonce = vcrypto.random_nonce().await;

        // Encode envelope
        let envelope = match version {
            ENVELOPE_VERSION_ENV0 => Envelope::try_new_env0(
                &crypto,
                node_id.kind(),
                timestamp,
                nonce,
                node_id,
                dest_node_id,
            )?,
            _ => {
                bail!("unsupported envelope version: {:?}", version);
            }
        };

        envelope
            .to_encrypted_data(&crypto, body, &secret_key, &network_key)
            .measure_limit(
                TimestampDuration::new_ms(500),
                veilid_log_dbg!(
                    registry,
                    "NetworkManager::build_envelope to_encrypted_data slow"
                ),
            )
            .await
            .wrap_err("envelope failed to encode")
    }

    /// Called by the RPC handler when we want to issue an RPC request or response
    /// node_ref is the final destination to which the envelope will be sent
    /// If `opt_dial_info` is specified, then it will be directly sent to this dialinfo
    /// without resolving the contact method
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn send_envelope(
        &self,
        node_ref: FilteredNodeRef,
        opt_dial_info: Option<DialInfo>,
        body: Bytes,
    ) -> EyreResult<SendDataResult> {
        let Ok(_guard) = self.startup_context.startup_lock.enter() else {
            return Ok(SendDataResult::new(
                NodeContactMethodResult::NoContactMethod,
                NetworkResult::no_connection_other("network is not started"),
            ));
        };
        // Capture a stop token so the long network awaits below can be cancelled
        // when shutdown is requested. Without this, the enter guard above is held
        // for the full duration of the send, blocking shutdown until each peer
        // round-trip completes (or its RPC timeout fires).
        let stop_token = match self.startup_context.startup_lock.stop_token() {
            Some(v) => v,
            None => {
                return Ok(SendDataResult::new(
                    NodeContactMethodResult::NoContactMethod,
                    NetworkResult::no_connection_other("network is stopping"),
                ));
            }
        };

        let best_node_id = node_ref.best_node_id();

        // Get node's envelope versions and see if we can send to it
        // and if so, get the max version we can use
        let Some(envelope_version) = node_ref.best_envelope_version() else {
            return Ok(SendDataResult::new(
                NodeContactMethodResult::NoContactMethod,
                NetworkResult::no_connection_other(format!(
                    "can't talk to this node {} because we dont support its envelope versions",
                    node_ref
                )),
            ));
        };

        // Build the envelope to send
        let timestamp = Timestamp::now_increasing();
        let out = match self
            .build_envelope(timestamp, best_node_id, envelope_version, body)
            .measure_limit(
                TimestampDuration::new_ms(1000),
                veilid_log_dbg!(self, "NetworkManager::build_envelope slow"),
            )
            .timeout_at(stop_token.clone())
            .await?
        {
            Ok(v) => v,
            Err(e) => {
                veilid_log!(self debug "couldn't build envelope: {}", e);
                return Ok(SendDataResult::new(
                    NodeContactMethodResult::NoContactMethod,
                    NetworkResult::no_connection_other("network is stopping"),
                ));
            }
        };

        if let Some(dial_info) = &opt_dial_info {
            veilid_log!(self trace
                "sending envelope to {:?} via {:?}, len={}, timestamp={:?}",
                node_ref,
                dial_info,
                out.len(),
                timestamp
            );
        } else {
            veilid_log!(self trace "sending envelope to {:?}, len={}, timestamp={:?}", node_ref, out.len(), timestamp);
        }

        // Send the envelope via whatever means necessary, gated on the shutdown stop token.
        let send_fut = async {
            if let Some(dial_info) = opt_dial_info {
                self.send_data_direct(node_ref.unfiltered(), dial_info, out)
                    .await
            } else {
                self.send_data(node_ref, out).await
            }
        };
        match send_fut.timeout_at(stop_token).await {
            Ok(res) => res,
            Err(_) => Ok(SendDataResult::new(
                NodeContactMethodResult::NoContactMethod,
                NetworkResult::no_connection_other("network is stopping"),
            )),
        }
    }

    /// Called by the RPC handler when we want to issue an direct receipt
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "receipt", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn send_out_of_band_receipt(
        &self,
        dial_info: DialInfo,
        rcpt_data: Bytes,
    ) -> EyreResult<()> {
        let Ok(_guard) = self.startup_context.startup_lock.enter() else {
            veilid_log!(self debug "not sending out-of-band receipt to {} because network is stopped", dial_info);
            return Ok(());
        };

        // Do we need to validate the outgoing receipt? Probably not
        // because it is supposed to be opaque and the
        // recipient/originator does the validation
        // Also, in the case of an old 'version', returning the receipt
        // should not be subject to our ability to decode it

        // Send receipt directly
        network_result_value_or_log!(self self
            .net()
            .send_data_unbound_to_dial_info(dial_info, rcpt_data)
            .await? => [ format!(": dial_info={}, rcpt_data.len={}", dial_info, rcpt_data.len()) ] {
                return Ok(());
            }
        );
        Ok(())
    }

    // Called when a packet potentially containing an RPC envelope is received by a low-level
    // network protocol handler. Processes the envelope, authenticates and decrypts the RPC message
    // and passes it to the RPC handler
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn on_recv_envelope(&self, data: Bytes, flow: Flow) -> EyreResult<bool> {
        let Ok(_guard) = self.startup_context.startup_lock.enter() else {
            return Ok(false);
        };

        veilid_log!(self trace "envelope of {} bytes received from {:?}", data.len(), flow);
        let remote_addr = flow.remote_address().ip_addr();

        // Network accounting
        self.stats_packet_rcvd(remote_addr, ByteCount::new(data.len() as u64));

        // If this is a zero length packet, just drop it, because these are used for hole punching
        // and possibly other low-level network connectivity tasks and will never require
        // more processing or forwarding
        if data.is_empty() {
            return Ok(true);
        }

        // Ensure we can read the magic number
        if data.len() < 4 {
            veilid_log!(self debug "short packet");
            self.address_filter()
                .punish_ip_addr(remote_addr, PunishmentReason::ShortPacket);
            return Ok(false);
        }
        let magic: [u8; 4] = data[0..4].try_into()?;

        // Get the routing domain for this data
        let routing_domain = match self.routing_table().routing_domain_for_flow(flow) {
            Some(rd) => rd,
            None => {
                veilid_log!(self debug "no routing domain for envelope received from {:?}", flow);
                return Ok(false);
            }
        };

        // Is this a direct bootstrap request instead of an envelope?
        if magic == *BOOT_MAGIC {
            network_result_value_or_log!(self pin_future!(self.handle_boot_v0_request(flow)).await? => [ format!(": v0 flow={:?}", flow) ] {});
            return Ok(true);
        }
        if magic == *B01T_MAGIC {
            network_result_value_or_log!(self pin_future!(self.handle_boot_v1_request(flow)).await? => [ format!(": v1 flow={:?}", flow) ] {});
            return Ok(true);
        }

        // Is this an out-of-band receipt instead of an envelope?
        if VALID_RECEIPT_VERSIONS.contains(&ReceiptVersion::from(magic)) {
            network_result_value_or_log!(self pin_future!(self.handle_out_of_band_receipt(data)).await => [ format!(": data.len={}", data.len()) ] {});
            return Ok(true);
        }

        // Decode envelope header (may fail signature validation)
        let crypto = self.crypto();
        let envelope =
            match Envelope::try_from_signed_data(&crypto, data.clone(), &self.network_key).await {
                Ok(v) => v,
                Err(e) => {
                    veilid_log!(self debug "envelope failed to decode: {}", e);
                    // safe to punish here because relays also check here to ensure they arent forwarding things that don't decode
                    self.address_filter()
                        .punish_ip_addr(remote_addr, PunishmentReason::FailedToDecodeEnvelope);
                    return Ok(false);
                }
            };

        // Verify and log timestamp
        let local_timestamp = Timestamp::now_increasing();
        let remote_timestamp = envelope.get_timestamp();
        match self.address_filter().check_envelope_timestamp(
            envelope.get_sender_id(),
            local_timestamp,
            remote_timestamp,
        ) {
            Ok(()) => {
                // Envelope is good, keep it
            }
            Err(e) => match e {
                TimestampError::TooFarBehind {
                    local_timestamp,
                    remote_timestamp: _,
                    adjusted_remote_timestamp,
                    timestamp_offset: _,
                } => {
                    veilid_log!(self debug
                        "Timestamp behind from {}: {} ({}): {:?}",
                        envelope.get_sender_id(),
                        local_timestamp.duration_since(adjusted_remote_timestamp),
                        flow.remote(),
                        e
                    );
                    return Ok(false);
                }
                TimestampError::TooFarAhead {
                    local_timestamp,
                    remote_timestamp: _,
                    adjusted_remote_timestamp,
                    timestamp_offset: _,
                } => {
                    veilid_log!(self debug
                        "Timestamp ahead from {}: {} ({}): {:?}",
                        envelope.get_sender_id(),
                        adjusted_remote_timestamp.duration_since(local_timestamp),
                        flow.remote(),
                        e
                    );
                    return Ok(false);
                }
                TimestampError::Duplicate {
                    local_timestamp: _,
                    last_local_timestamp: _,
                    remote_timestamp: _,
                    adjusted_remote_timestamp: _,
                    timestamp_offset: _,
                } => {
                    veilid_log!(self debug
                        "Duplicate envelope from {} ({}): {:?}",
                        envelope.get_sender_id(),
                        flow.remote(),
                        e
                    );
                    return Ok(false);
                }
            },
        }

        // Get routing table and rpc processor
        let routing_table = self.routing_table();
        let rpc = self.rpc_processor();

        // See if this sender is punished, if so, ignore the packet
        let sender_id = envelope.get_sender_id();
        if self.address_filter().is_node_id_punished(sender_id.clone()) {
            return Ok(false);
        }

        // Peek at header and see if we need to relay this
        // If the recipient id is not our node id, then it needs relaying
        let recipient_id = envelope.get_recipient_id();
        if !routing_table.matches_own_node_id(std::slice::from_ref(&recipient_id)) {
            // See if the source node is allowed to resolve nodes
            // This is a costly operation, so only outbound-relay permitted
            // nodes are allowed to do this, for example PWA users

            // xxx: eventually allow recipient_id to be in allowlist?
            // xxx: to enable cross-routing domain relaying? or rather
            // xxx: that 'localnetwork' routing domain nodes could be allowed to
            // xxx: full relay as well as client_allowlist nodes...

            let some_relay = if self.check_client_allowlist(sender_id.clone()) {
                // Full relay allowed, do a full resolve_node
                match rpc
                    .resolve_node(
                        recipient_id.clone(),
                        SafetySelection::Unsafe(Sequencing::PreferUnordered),
                    )
                    .await
                {
                    Ok(v) => v.map(|nr| (nr, RelayKind::Outbound)),
                    Err(e) => {
                        veilid_log!(self debug "failed to resolve recipient node for relay, dropping relayed envelope: {}" ,e);
                        return Ok(false);
                    }
                }
            } else {
                // If this is not a node in the client allowlist, only allow inbound relay
                // which only performs a lightweight lookup before passing the packet back out

                // If our node has the relay capability disabled, we should not be asked to relay
                if self
                    .config()
                    .capabilities
                    .disable
                    .contains(&VEILID_CAPABILITY_RELAY)
                {
                    veilid_log!(self debug "node has relay capability disabled, dropping relayed envelope from {} to {}", sender_id, recipient_id);
                    return Ok(false);
                }

                // See if we have the node in our routing table
                // We should, because relays are chosen by nodes that have established connectivity and
                // should be mutually in each others routing tables. The node needing the relay will be
                // pinging this node regularly to keep itself in the routing table
                match routing_table.lookup_node_id(recipient_id) {
                    Ok(v) => v.map(|nr| (nr, RelayKind::Inbound)),
                    Err(e) => {
                        veilid_log!(self debug "failed to look up recipient node for relay, dropping relayed envelope: {}" ,e);
                        return Ok(false);
                    }
                }
            };

            if let Some((relay_nr, relay_kind)) = some_relay {
                // Ensure the protocol used to forward is of the same sequencing requirement
                // Address type is allowed to change if connectivity is better
                let sequencing = flow.protocol_type().sequence_ordering().strict_sequencing();

                // Pass relay to RPC system
                if let Err(e) = self.enqueue_relay(relay_nr, sequencing, data, relay_kind) {
                    // Couldn't enqueue, but not the sender's fault
                    veilid_log!(self debug "failed to enqueue relay: {}", e);
                    return Ok(false);
                }
            }
            // Inform caller that we dealt with the envelope, but did not process it locally
            return Ok(false);
        }

        // DH to get decryption key (cached)
        let secret_key = routing_table.secret_key(envelope.get_crypto_kind());

        // Decrypt the envelope body
        let crypto = self.crypto();
        let body = match envelope
            .decrypt_body(&crypto, data, &secret_key, &self.network_key)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                veilid_log!(self debug "failed to decrypt envelope body: {}", e);
                // Can't punish by ip address here because relaying can't decrypt envelope bodies to check
                // But because the envelope was properly signed by the time it gets here, it is safe to
                // punish by node id
                self.address_filter()
                    .punish_node_id(sender_id, PunishmentReason::FailedToDecryptEnvelopeBody);
                return Ok(false);
            }
        };

        // Add the sender's node without its peer info
        // Gets noderef filtered to the routing domain
        let sender_noderef = match routing_table
            .register_node_without_peer_info(routing_domain, sender_id)
        {
            Ok(v) => v,
            Err(e) => {
                // If the node couldn't be registered just skip this envelope,
                veilid_log!(self debug "failed to register node with existing connection: {}", e);
                return Ok(false);
            }
        };

        // Filter the noderef further by its inbound flow
        let sender_noderef = sender_noderef.merge_filter_clone(
            NodeRefFilter::new()
                .with_address_type(flow.address_type())
                .with_protocol_type(flow.protocol_type()),
        );

        // Set the envelope version for the peer
        sender_noderef.add_envelope_version(envelope.get_version());

        // Set the last flow for the peer
        self.set_last_flow(sender_noderef.unfiltered(), flow, local_timestamp);

        // Pass message to RPC system
        if let Err(e) =
            rpc.enqueue_direct_message(envelope, sender_noderef, flow, routing_domain, body)
        {
            // Couldn't enqueue, but not the sender's fault
            veilid_log!(self debug "failed to enqueue direct message: {}", e);
            return Ok(false);
        }

        // Inform caller that we dealt with the envelope locally
        Ok(true)
    }

    /// Record the last flow for a peer in the routing table and the connection table appropriately
    pub(super) fn set_last_flow(&self, node_ref: NodeRef, flow: Flow, timestamp: Timestamp) {
        // Set the last flow on the routing table entry
        node_ref.set_last_flow(flow, timestamp);

        // Get the routing domain for the flow
        let Some(routing_domain) = self.routing_table().routing_domain_for_flow(flow) else {
            // Flow may be dead because of a network change
            veilid_log!(self debug
                "flow found with no routing domain: {} for {}",
                flow, node_ref
            );
            return;
        };

        // Inform the connection table about the flow's priority
        let is_relaying_flow = node_ref.is_relaying(routing_domain);
        if is_relaying_flow
            && matches!(
                flow.protocol_type().sequence_ordering(),
                SequenceOrdering::Ordered
            )
        {
            self.connection_manager().add_relaying_flow(flow);
        }
    }

    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub fn restart_network(&self) {
        self.net().restart_network();
    }

    // Report peer info changes
    fn peer_info_change_event_handler(&self, evt: Arc<PeerInfoChangeEvent>) {
        let mut inner = self.inner.lock();
        if let Some(address_check) = inner.address_check.as_mut() {
            address_check
                .report_peer_info_change(evt.routing_domain, evt.opt_new_peer_info.clone());
        }
    }

    // Determine if our IP address has changed
    // this means we should recreate our public dial info if it is not static and rediscover it
    // Wait until we have received confirmation from N different peers
    fn socket_address_change_event_handler(&self, evt: Arc<SocketAddressChangeEvent>) {
        let mut inner = self.inner.lock();
        if let Some(address_check) = inner.address_check.as_mut() {
            address_check.report_socket_address_change(
                evt.routing_domain,
                evt.socket_address,
                evt.old_socket_address,
                evt.flow,
                evt.reporting_peer.clone(),
            );
        }
    }
}
