mod address_family_confirm;
mod global_event_listener_wrapper;
mod global_online_offline_event;
mod protocol;

use super::*;

use global_event_listener_wrapper::GlobalEventListenerWrapper;
use global_online_offline_event::{GLOBAL_OFFLINE_EVENT, GLOBAL_ONLINE_EVENT};

pub use protocol::*;
use std::io;

impl_veilid_log_facility!("net");

/////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProtocolConfig {
    pub outbound: ProtocolTypeSet,
    pub inbound: ProtocolTypeSet,
    pub family_global: AddressTypeSet,
    pub public_internet_capabilities: BTreeSet<VeilidCapability>,
}

struct WasmNetworkInner {
    network_needs_restart: bool,
    protocol_config: ProtocolConfig,
    last_confirm_request_ts: BTreeMap<RoutingDomain, Timestamp>,
    // Address-family confirmation worker lifecycle (cancelled+joined in cancel_tasks)
    address_family_stop_source: Option<StopSource>,
    address_family_worker_jh: Option<MustJoinHandle<()>>,
    online_event_listener_jh: Option<MustJoinHandle<()>>,
    offline_event_listener_jh: Option<MustJoinHandle<()>>,
}

pub(super) struct WasmNetworkUnlockedInner {
    // Startup lock
    startup_lock: StartupLock,
}

#[derive(Clone)]
pub(super) struct WasmNetwork {
    registry: VeilidComponentRegistry,
    inner: Arc<Mutex<WasmNetworkInner>>,
    unlocked_inner: Arc<WasmNetworkUnlockedInner>,
}

impl_veilid_component_accessors!(WasmNetwork);

impl core::ops::Deref for WasmNetwork {
    type Target = WasmNetworkUnlockedInner;

    fn deref(&self) -> &Self::Target {
        &self.unlocked_inner
    }
}

impl PlatformNetwork for WasmNetwork {
    #[cfg_attr(feature = "instrument", instrument(level = "debug", err, skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    fn startup(&self) -> PinBoxFuture<'_, EyreResult<StartupDisposition>> {
        Box::pin(async move {
            let guard = self.startup_lock.startup()?;

            match self.startup_internal() {
                Ok(StartupDisposition::Success) => {
                    veilid_log!(self debug "Network started");
                    guard.success();
                    Ok(StartupDisposition::Success)
                }
                Ok(StartupDisposition::BindRetry) => {
                    debug!("network bind retry");
                    Ok(StartupDisposition::BindRetry)
                }
                Err(e) => {
                    debug!("network failed to start");
                    Err(e)
                }
            }
        })
    }

    fn needs_restart(&self) -> bool {
        self.inner.lock().network_needs_restart
    }

    fn is_started(&self) -> bool {
        self.startup_lock.is_started()
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    fn restart_network(&self) {
        self.inner.lock().network_needs_restart = true;
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    fn shutdown(&self) -> PinBoxFuture<'_, ()> {
        Box::pin(async move {
            veilid_log!(self debug "starting network shutdown");
            let Ok(guard) = self.startup_lock.shutdown().await else {
                // Startup never reached the success state; nothing to tear down
                veilid_log!(self debug "network was not started, nothing to shut down");
                return;
            };

            // // Reset state
            // let routing_table = self.routing_table();
            // routing_table
            //     .edit_public_internet_routing_domain()
            //     .reset()
            //     .await;

            // Cancels all async background tasks by dropping join handles
            *self.inner.lock() = Self::new_inner();

            guard.success();
            veilid_log!(self debug "finished network shutdown");
        })
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", name = "Network::tick", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn tick(&self) -> PinBoxFuture<'_, EyreResult<()>> {
        Box::pin(async move {
            let Ok(_guard) = self.startup_lock.enter() else {
                veilid_log!(self debug "ignoring 'Network::tick' due to not started up");
                return Ok(());
            };

            // No per-tick tasks for WASM; the address-family confirmation worker self-drives

            Ok(())
        })
    }

    fn cancel_tasks(&self) -> PinBoxFuture<'_, ()> {
        Box::pin(async move {
            // Cancel the address-family confirmation worker and wait for it to exit
            let (
                opt_address_family_worker_jh,
                opt_online_event_listener_jh,
                opt_offline_event_listener_jh,
            ) = {
                let mut inner = self.inner.lock();

                drop(inner.address_family_stop_source.take());
                (
                    inner.address_family_worker_jh.take(),
                    inner.online_event_listener_jh.take(),
                    inner.offline_event_listener_jh.take(),
                )
            };
            if let Some(jh) = opt_address_family_worker_jh {
                jh.await;
            }
            if let Some(jh) = opt_online_event_listener_jh {
                jh.await;
            }
            if let Some(jh) = opt_offline_event_listener_jh {
                jh.await;
            }
        })
    }

    fn connect(
        &self,
        _local_address: Option<SocketAddr>,
        dial_info: DialInfo,
        timeout_ms: u32,
    ) -> PinBoxFuture<'_, EyreResult<NetworkResult<ProtocolNetworkConnection>>> {
        Box::pin(async move {
            let network_manager = self.network_manager();
            let address_filter = network_manager.address_filter();
            if address_filter.is_ip_addr_punished(dial_info.address().ip_addr()) {
                return Ok(NetworkResult::no_connection_other("punished"));
            }
            WasmProtocolNetworkConnection::connect(self.registry(), dial_info, timeout_ms)
                .await
                .wrap_err("io error in connect")
        })
    }

    #[cfg_attr(feature = "instrument", instrument(level="trace", target="net", err, skip(self, data), fields(data.len = data.len())))]
    fn send_data_unbound_to_dial_info(
        &self,
        dial_info: DialInfo,
        data: Bytes,
    ) -> PinBoxFuture<'_, EyreResult<NetworkResult<()>>> {
        Box::pin(async move {
            let _guard = self.unlocked_inner.startup_lock.enter()?;

            let data_len = data.len();
            let ip_addr = dial_info.address().ip_addr();
            let timeout_ms = self
                .config()
                .internal()
                .network
                .connection_initial_timeout_ms;

            if self
                .network_manager()
                .address_filter()
                .is_ip_addr_punished(ip_addr)
            {
                return Ok(NetworkResult::no_connection_other("punished"));
            }

            match dial_info.protocol_type() {
                ProtocolType::UDP => {
                    bail!("no support for UDP protocol")
                }
                ProtocolType::TCP => {
                    bail!("no support for TCP protocol")
                }
                ProtocolType::WS => {
                    let pnc = network_result_try!(ws::WebsocketProtocolHandler::connect(
                        self.registry(),
                        dial_info,
                        timeout_ms
                    )
                    .await
                    .wrap_err("connect failure")?);
                    network_result_try!(pnc.send(data).await.wrap_err("send failure")?);
                }
                #[cfg(feature = "enable-protocol-wss")]
                ProtocolType::WSS => {
                    let pnc = network_result_try!(ws::WebsocketProtocolHandler::connect(
                        self.registry(),
                        dial_info,
                        timeout_ms
                    )
                    .await
                    .wrap_err("connect failure")?);
                    network_result_try!(pnc.send(data).await.wrap_err("send failure")?);
                }
            };

            // Network accounting
            self.network_manager()
                .stats_packet_sent(ip_addr, ByteCount::new(data_len as u64));

            Ok(NetworkResult::Value(()))
        })
    }

    fn send_recv_data_unbound_to_dial_info(
        &self,
        dial_info: DialInfo,
        data: Bytes,
        timeout_ms: u32,
    ) -> PinBoxFuture<'_, EyreResult<NetworkResult<Bytes>>> {
        Box::pin(async move {
            let _guard = self.startup_lock.enter()?;

            let data_len = data.len();
            let network_manager = self.network_manager();
            let ip_addr = dial_info.address().ip_addr();
            let connect_timeout_ms = self
                .config()
                .internal()
                .network
                .connection_initial_timeout_ms;

            if network_manager
                .address_filter()
                .is_ip_addr_punished(ip_addr)
            {
                return Ok(NetworkResult::no_connection_other("punished"));
            }

            match dial_info.protocol_type() {
                ProtocolType::UDP => {
                    bail!("no support for UDP protocol")
                }
                ProtocolType::TCP => {
                    bail!("no support for TCP protocol")
                }
                _ => {
                    let pnc = network_result_try!(match dial_info.protocol_type() {
                        ProtocolType::UDP => bail!("no support for UDP protocol"),
                        ProtocolType::TCP => bail!("no support for TCP protocol"),
                        ProtocolType::WS => {
                            ws::WebsocketProtocolHandler::connect(
                                self.registry(),
                                dial_info,
                                connect_timeout_ms,
                            )
                            .await
                            .wrap_err("connect failure")?
                        }
                        #[cfg(feature = "enable-protocol-wss")]
                        ProtocolType::WSS => {
                            ws::WebsocketProtocolHandler::connect(
                                self.registry(),
                                dial_info,
                                connect_timeout_ms,
                            )
                            .await
                            .wrap_err("connect failure")?
                        }
                    });

                    network_result_try!(pnc.send(data).await.wrap_err("send failure")?);
                    network_manager.stats_packet_sent(ip_addr, ByteCount::new(data_len as u64));

                    let out =
                        network_result_try!(network_result_try!(timeout(timeout_ms, pnc.recv())
                            .await
                            .into_network_result())
                        .wrap_err("recv failure")?);

                    network_manager.stats_packet_rcvd(ip_addr, ByteCount::new(out.len() as u64));

                    Ok(NetworkResult::Value(out))
                }
            }
        })
    }

    #[cfg_attr(feature = "instrument", instrument(level="trace", target="net", err, skip(self, data), fields(data.len = data.len())))]
    fn send_data_to_existing_flow(
        &self,
        flow: Flow,
        data: Bytes,
    ) -> PinBoxFuture<'_, EyreResult<SendDataToExistingFlowResult>> {
        Box::pin(async move {
            let _guard = self.startup_lock.enter()?;

            let data_len = data.len();
            let network_manager = self.network_manager();
            let ip_addr = flow.remote().socket_addr().ip();

            match flow.protocol_type() {
                ProtocolType::UDP => {
                    bail!("no support for UDP protocol")
                }
                ProtocolType::TCP => {
                    bail!("no support for TCP protocol")
                }
                _ => {}
            }

            // Handle connection-oriented protocols

            // Try to send to the exact existing connection if one exists
            if let Some(conn) = network_manager.connection_manager().get_connection(flow) {
                // connection exists, send over it
                match conn.send_async(data).await {
                    ConnectionHandleSendResult::Sent => {
                        // Network accounting
                        network_manager.stats_packet_sent(ip_addr, ByteCount::new(data_len as u64));

                        // Data was consumed
                        return Ok(SendDataToExistingFlowResult::Sent(conn.unique_flow()));
                    }
                    ConnectionHandleSendResult::NotSent(data) => {
                        // Couldn't send
                        // Pass the data back out so we don't own it any more
                        return Ok(SendDataToExistingFlowResult::NotSent(data));
                    }
                }
            }
            // Connection didn't exist
            // Pass the data back out so we don't own it any more
            Ok(SendDataToExistingFlowResult::NotSent(data))
        })
    }

    #[cfg_attr(feature = "instrument", instrument(level="trace", target="net", err, skip(self, data), fields(data.len = data.len())))]
    fn send_data_to_dial_info(
        &self,
        dial_info: DialInfo,
        data: Bytes,
    ) -> PinBoxFuture<'_, EyreResult<NetworkResult<UniqueFlow>>> {
        Box::pin(async move {
            let _guard = self.startup_lock.enter()?;

            let network_manager = self.network_manager();
            let ip_addr = dial_info.address().ip_addr();
            let data_len = data.len();

            if dial_info.protocol_type() == ProtocolType::UDP {
                bail!("no support for UDP protocol");
            }
            if dial_info.protocol_type() == ProtocolType::TCP {
                bail!("no support for TCP protocol");
            }

            // Handle connection-oriented protocols
            let conn = network_result_try!(
                network_manager
                    .connection_manager()
                    .get_or_create_connection(dial_info)
                    .await?
            );

            if let ConnectionHandleSendResult::NotSent(_) = conn.send_async(data).await {
                return Ok(NetworkResult::NoConnection(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "failed to send",
                )));
            }
            let unique_flow = conn.unique_flow();

            // Network accounting
            network_manager.stats_packet_sent(ip_addr, ByteCount::new(data_len as u64));

            Ok(NetworkResult::value(unique_flow))
        })
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", err, skip(self), fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn send_hole_punch(
        &self,
        _dial_info: DialInfo,
    ) -> PinBoxFuture<'_, EyreResult<NetworkResult<UniqueFlow>>> {
        Box::pin(async move {
            Ok(NetworkResult::ServiceUnavailable(
                "unimplemented for this platform".to_owned(),
            ))
        })
    }

    fn get_preferred_local_address(&self, _dial_info: &DialInfo) -> Option<SocketAddr> {
        None
    }

    fn get_preferred_local_address_by_key(&self, _tt: TransportType) -> Option<SocketAddr> {
        None
    }

    fn preferred_outbound_source_addr(&self, _dial_info: &DialInfo) -> Option<SocketAddr> {
        None
    }

    fn routing_domains_detecting_address_changes(&self) -> BTreeSet<RoutingDomain> {
        let Ok(_guard) = self.startup_lock.enter() else {
            veilid_log!(self debug "ignoring 'routing_domains_detecting_address_changes' due to not started up");
            return BTreeSet::new();
        };

        // WASM detects supported address families empirically via confirm_address_families,
        // so PublicInternet starts unconfirmed and re-confirms on address change.
        BTreeSet::from([RoutingDomain::PublicInternet])
    }

    fn routing_domain_request_confirm_dial_info(&self, routing_domain: RoutingDomain) -> bool {
        let Ok(_guard) = self.startup_lock.enter() else {
            veilid_log!(self debug "ignoring 'request_confirm_dial_info' due to not started up");
            return false;
        };

        self.reset_address_family_confirm_state(routing_domain)
    }
}

/////////////////////////////////////////////////////////////////

impl WasmNetwork {
    fn new_inner() -> WasmNetworkInner {
        WasmNetworkInner {
            network_needs_restart: false,
            protocol_config: Default::default(),
            last_confirm_request_ts: BTreeMap::new(),
            address_family_stop_source: None,
            address_family_worker_jh: None,
            online_event_listener_jh: None,
            offline_event_listener_jh: None,
        }
    }

    fn new_unlocked_inner() -> WasmNetworkUnlockedInner {
        WasmNetworkUnlockedInner {
            startup_lock: StartupLock::new(),
        }
    }

    pub fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            registry,
            inner: Arc::new(Mutex::new(Self::new_inner())),
            unlocked_inner: Arc::new(Self::new_unlocked_inner()),
        }
    }

    /////////////////////////////////////////////////////////////////

    pub fn startup_internal(&self) -> EyreResult<StartupDisposition> {
        veilid_log!(self debug "starting network");
        // get protocol config
        let protocol_config = {
            let config = self.config();
            let inbound = ProtocolTypeSet::new();
            let mut outbound = ProtocolTypeSet::new();

            if config.network.protocol.ws.connect {
                outbound.insert(ProtocolType::WS);
            }
            #[cfg(feature = "enable-protocol-wss")]
            if config.network.protocol.wss.connect {
                outbound.insert(ProtocolType::WSS);
            }

            // Start optimistic (all config-allowed families) but unconfirmed; the routing
            // domain sits in NeedsDialInfoConfirmation until confirm_address_families()
            // narrows this to the families that answer and sets confirmed=true.
            let family_global = configured_address_type_set(&config);

            let public_internet_capabilities = {
                PUBLIC_INTERNET_CAPABILITIES
                    .iter()
                    .copied()
                    .filter(|cap| !config.capabilities.disable.contains(cap))
                    .collect()
            };

            ProtocolConfig {
                outbound,
                inbound,
                family_global,
                public_internet_capabilities,
            }
        };
        self.inner.lock().protocol_config = protocol_config.clone();

        // Tell offline detection which socket classes this network uses
        self.network_manager()
            .online_detector()
            .set_protocol_config(protocol_config.outbound, protocol_config.inbound);

        // Start editing routing table
        let routing_table = self.routing_table();
        let public_internet_controller = routing_table
            .get_specific_routing_domain_controller::<PublicInternetRoutingDomainController>();
        let mut public_internet_editor = public_internet_controller.edit();

        // set up the routing table's network config
        let network_config_public_internet = RoutingDomainNetworkConfig::new(
            protocol_config.outbound,
            protocol_config.inbound,
            protocol_config.family_global,
            protocol_config.public_internet_capabilities,
        );
        public_internet_editor.set_network_config(network_config_public_internet);
        // Start unconfirmed; confirm_address_families() (driven from tick) probes the
        // address families and sets confirmed=true once it has a verdict.
        public_internet_editor.set_confirmed(false);

        self.inner.lock().last_confirm_request_ts.insert(
            RoutingDomain::PublicInternet,
            Timestamp::now_non_decreasing(),
        );

        // commit routing domain edits
        public_internet_editor.commit();
        public_internet_controller.publish_peer_info();

        // Spawn the address-family confirmation worker (cancelled+joined in cancel_tasks)
        // Turn on detection of online/offline status from window/worker events
        self.start_address_family_confirm_worker();

        Ok(StartupDisposition::Success)
    }
}
