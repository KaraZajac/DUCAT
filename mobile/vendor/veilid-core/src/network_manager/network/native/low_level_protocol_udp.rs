use super::*;
use stop_token::future::FutureExt;

const UDP_LISTENER_RESTART_AFTER_CONSECUTIVE_ERRORS: usize = 16;

/// Runtime command to a UDP listener task
pub(super) enum UdpListenerCommand {
    /// Spawn a recv-loop for this handler
    AddHandler(RawUdpProtocolHandler),
}

/// Event drained by a listener task from its `FuturesUnordered`
enum UdpListenerEvent {
    /// A recv-loop returned: `None` = stop, `Some(e)` = persistent error
    HandlerExited(Option<io::Error>),
    /// A new handler arrived on the command channel
    NewHandler(RawUdpProtocolHandler),
    /// All command senders dropped
    ChannelClosed,
}

impl NativeNetwork {
    fn make_udp_handler_future(
        this: NativeNetwork,
        ph: RawUdpProtocolHandler,
    ) -> PinBoxFutureStatic<UdpListenerEvent> {
        let stop_token = ph.stop_token();
        Box::pin(async move {
            let network_manager = this.network_manager();
            let mut data = vec![0u8; 65536];
            let mut consecutive_errors: usize = 0;
            loop {
                match ph
                    .recv_message(&mut data)
                    .timeout_at(stop_token.clone())
                    .in_current_span()
                    .await
                {
                    Ok(Ok((size, flow))) => {
                        consecutive_errors = 0;
                        network_manager.stats_packet_rcvd(
                            flow.remote_address().ip_addr(),
                            ByteCount::new(size as u64),
                        );
                        if let Err(e) = network_manager
                            .on_recv_envelope(Bytes::copy_from_slice(&data[..size]), flow)
                            .measure_debug(TimestampDuration::new_ms(500), |x| {
                                veilid_log!(network_manager debug "on_recv_envelope: {} for {}", x, flow);
                            })
                            .await
                        {
                            veilid_log!(network_manager debug "failed to process received udp envelope: {}", e);
                        }
                    }
                    Ok(Err(e)) => {
                        consecutive_errors += 1;
                        if consecutive_errors >= UDP_LISTENER_RESTART_AFTER_CONSECUTIVE_ERRORS {
                            return UdpListenerEvent::HandlerExited(Some(e));
                        }
                        veilid_log!(this debug
                            "UDP recv_message transient error {}/{} ({})",
                            consecutive_errors,
                            UDP_LISTENER_RESTART_AFTER_CONSECUTIVE_ERRORS,
                            e
                        );
                    }
                    Err(_) => {
                        return UdpListenerEvent::HandlerExited(None);
                    }
                }
            }
        })
    }

    fn make_udp_channel_wait_future(
        rx: flume::Receiver<UdpListenerCommand>,
    ) -> PinBoxFutureStatic<UdpListenerEvent> {
        Box::pin(async move {
            match rx.recv_async().await {
                Ok(UdpListenerCommand::AddHandler(ph)) => UdpListenerEvent::NewHandler(ph),
                Err(_) => UdpListenerEvent::ChannelClosed,
            }
        })
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn create_udp_listener_tasks(&self) -> EyreResult<()> {
        // Spawn socket tasks
        let mut task_count = self
            .config()
            .internal()
            .network
            .protocol
            .udp
            .socket_pool_size;
        if task_count == 0 {
            task_count = get_concurrency() / 2;
            if task_count == 0 {
                task_count = 1;
            }
        }
        veilid_log!(self trace "task_count: {}", task_count);
        for task_n in 0..task_count {
            veilid_log!(self trace "Spawning UDP listener task");

            // Per-task command channel
            let (tx, rx) = flume::unbounded::<UdpListenerCommand>();
            self.inner.lock().udp_listener_command_txs.push(tx);

            ////////////////////////////////////////////////////////////
            // Run thread task to process stream of messages
            let this = self.clone();

            let jh = spawn(&format!("UDP listener {}", task_n), async move {
                veilid_log!(this trace "UDP listener task spawned");

                let protocol_handlers: Vec<RawUdpProtocolHandler> = this
                    .inner
                    .lock()
                    .udp_protocol_handlers
                    .values()
                    .cloned()
                    .collect();

                let mut events = FuturesUnordered::<PinBoxFutureStatic<UdpListenerEvent>>::new();
                if this.inner.lock().stop_source.is_none() {
                    veilid_log!(this debug "exiting UDP listener before it starts because we encountered an error");
                    return;
                }

                for ph in protocol_handlers {
                    events.push(Self::make_udp_handler_future(this.clone(), ph));
                }

                // Wake on AddHandler
                let mut channel_open = true;
                events.push(Self::make_udp_channel_wait_future(rx.clone()));

                while let Some(evt) = events.next().in_current_span().await {
                    match evt {
                        UdpListenerEvent::HandlerExited(None) => {
                            veilid_log!(this trace "UDP recv-loop stopped");
                        }
                        UdpListenerEvent::HandlerExited(Some(e)) => {
                            veilid_log!(this debug
                                "UDP listener task errored persistently ({}), triggering network restart",
                                e
                            );
                            this.inner.lock().network_needs_restart = true;
                        }
                        UdpListenerEvent::NewHandler(ph) => {
                            veilid_log!(this trace "UDP recv-loop added");
                            events.push(Self::make_udp_handler_future(this.clone(), ph));
                            if channel_open {
                                events.push(Self::make_udp_channel_wait_future(rx.clone()));
                            }
                        }
                        UdpListenerEvent::ChannelClosed => {
                            veilid_log!(this trace "UDP listener command channel closed");
                            channel_open = false;
                        }
                    }
                }

                veilid_log!(this trace "UDP listener task stopped");
            }.instrument(trace_span!(parent: None, "UDP Listener", __VEILID_LOG_KEY = self.log_key())));
            ////////////////////////////////////////////////////////////

            // Add to join handle
            self.add_to_join_handles(jh);
        }

        Ok(())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    fn create_udp_protocol_handler(&self, addr: SocketAddr) -> EyreResult<bool> {
        veilid_log!(self debug "create_udp_protocol_handler on {:?}", &addr);

        // Probed bind: an occupied port must fail here so the port search
        // advances instead of reuse-binding alongside another process
        let Some(udp_socket) = bind_async_udp_socket_probed(addr)? else {
            return Ok(false);
        };
        let socket_arc = Arc::new(udp_socket);

        // Create protocol handler
        let protocol_handler =
            RawUdpProtocolHandler::new(self.registry(), socket_arc, addr.is_ipv6());

        // Record protocol handler
        let mut inner = self.inner.lock();
        inner
            .udp_protocol_handlers
            .insert(addr, protocol_handler.clone());

        Ok(true)
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn create_outbound_udp_protocol_handlers(&self) -> EyreResult<bool> {
        let allowed = configured_address_type_set(&self.config());
        let enable_ipv4 = is_ipv4_supported() && allowed.contains(AddressType::IPV4);
        if enable_ipv4 {
            let has_ipv4_handler = self
                .inner
                .lock()
                .udp_protocol_handlers
                .iter()
                .any(|x| x.0.is_ipv4());
            if !has_ipv4_handler {
                // Needs ipv4 handler for outbound but we aren't listening on it, so make a random one
                let bound = self.create_udp_protocol_handler(SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::UNSPECIFIED,
                    0,
                )))?;
                if !bound {
                    bail!("unable to bind ipv4 default udp handler");
                }
            }
        }
        let enable_ipv6 = is_ipv6_supported() && allowed.contains(AddressType::IPV6);
        if enable_ipv6 {
            let has_ipv6_handler = self
                .inner
                .lock()
                .udp_protocol_handlers
                .iter()
                .any(|x| x.0.is_ipv6());
            if !has_ipv6_handler {
                // Needs ipv6 handler for outbound but we aren't listening on it, so make a random one
                let bound = self.create_udp_protocol_handler(SocketAddr::V6(SocketAddrV6::new(
                    Ipv6Addr::UNSPECIFIED,
                    0,
                    0,
                    0,
                )))?;
                if !bound {
                    bail!("unable to bind ipv6 default udp handler");
                }
            }
        }
        Ok(true)
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn create_inbound_udp_protocol_handlers(
        &self,
        bind_set: NetworkBindSet,
    ) -> EyreResult<bool> {
        for ip_addr in bind_set.addrs {
            let mut port = bind_set.port;
            loop {
                let addr = SocketAddr::new(ip_addr, port);

                // see if we've already bound to this already
                // if not, spawn a listener
                if !self.inner.lock().udp_protocol_handlers.contains_key(&addr) {
                    let bound = self.clone().create_udp_protocol_handler(addr)?;

                    // Return interface dial infos we listen on
                    if bound {
                        let mut inner = self.inner.lock();
                        let bapp = inner
                            .bound_address_per_protocol
                            .entry(ProtocolType::UDP)
                            .or_default();
                        bapp.push(addr);

                        veilid_log!(self
                            debug
                            "set_preferred_local_address: {:?} {:?} -> {:?}",
                            ProtocolType::UDP,
                            addr,
                            PeerAddress::new(SocketAddress::from_socket_addr(addr), ProtocolType::UDP)
                        );

                        Self::set_preferred_local_address(
                            &mut inner,
                            PeerAddress::new(
                                SocketAddress::from_socket_addr(addr),
                                ProtocolType::UDP,
                            ),
                        );

                        break;
                    }
                }
                if !bind_set.search {
                    veilid_log!(self debug "unable to bind to udp {}", addr);
                    return Ok(false);
                }

                if port == 65535u16 {
                    port = 1024;
                } else {
                    port += 1;
                }

                if port == bind_set.port {
                    bail!("unable to find a free port for udp {}", ip_addr);
                }
            }
        }
        Ok(true)
    }

    /// Reconcile source-bound UDP handlers with the current interface address set
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub(super) fn bind_outbound_source_udp_handlers(&self) {
        let mut wanted: BTreeSet<SocketAddr> = BTreeSet::new();
        let mut bound_listen_addrs: BTreeSet<SocketAddr> = BTreeSet::new();
        let interface_address_state = self.interfaces.interface_address_state();
        for at in [AddressType::IPV4, AddressType::IPV6] {
            let Some(listen_addr) =
                self.get_preferred_local_address_by_key(TransportType::new(ProtocolType::UDP, at))
            else {
                continue;
            };
            bound_listen_addrs.insert(listen_addr);
            // A pinned listen address already source-binds outbound; no extra handler needed
            if !listen_addr.ip().is_unspecified() {
                continue;
            }
            let listen_port = listen_addr.port();
            for ifaddr in interface_address_state.interface_addresses.iter() {
                let ip = ifaddr.ip();
                let matches_family = matches!(
                    (ip, at),
                    (IpAddr::V4(_), AddressType::IPV4) | (IpAddr::V6(_), AddressType::IPV6)
                );
                if !matches_family {
                    continue;
                }
                if !Address::from_ip_addr(ip).is_global() {
                    continue;
                }
                wanted.insert(SocketAddr::new(ip, listen_port));
            }
        }

        // Source-bound handlers have a specific (non-unspecified) bind address
        let stale: Vec<(SocketAddr, RawUdpProtocolHandler)> = {
            let inner = self.inner.lock();
            inner
                .udp_protocol_handlers
                .iter()
                .filter(|(addr, _ph)| {
                    !addr.ip().is_unspecified()
                        && !wanted.contains(*addr)
                        && !bound_listen_addrs.contains(*addr)
                })
                .map(|(addr, ph)| (*addr, ph.clone()))
                .collect()
        };
        for (addr, ph) in stale {
            ph.stop();
            self.inner.lock().udp_protocol_handlers.remove(&addr);
            veilid_log!(self debug "released UDP outbound source handler at {} (interface address gone)", addr);
        }

        for addr in wanted {
            let already = self.inner.lock().udp_protocol_handlers.contains_key(&addr);
            if already {
                continue;
            }
            match RawUdpProtocolHandler::new_specific_bound_handler(self.registry(), addr) {
                Ok(ph) => {
                    let mut inner = self.inner.lock();
                    inner.udp_protocol_handlers.insert(addr, ph.clone());
                    for tx in inner.udp_listener_command_txs.iter() {
                        if let Err(e) = tx.send(UdpListenerCommand::AddHandler(ph.clone())) {
                            veilid_log!(self error
                                "failed to send UDP outbound source handler to {} to listener task: {}",
                                addr, e
                            );
                        }
                    }
                    drop(inner);
                    veilid_log!(self debug "bound UDP outbound source handler at {}", addr);
                }
                Err(e) => {
                    veilid_log!(self debug "failed to bind UDP outbound source handler at {}: {}", addr, e);
                }
            }
        }
    }

    /////////////////////////////////////////////////////////////////

    pub(super) fn find_best_udp_protocol_handler(
        &self,
        peer_socket_addr: &SocketAddr,
        local_socket_addr: &Option<SocketAddr>,
    ) -> Option<RawUdpProtocolHandler> {
        let inner = self.inner.lock();
        // if our last communication with this peer came from a particular inbound udp protocol handler, use it
        if let Some(sa) = local_socket_addr {
            if let Some(ph) = inner.udp_protocol_handlers.get(sa) {
                return Some(ph.clone());
            }
        }

        // otherwise find the first outbound udp protocol handler that matches the ip protocol version of the peer addr
        match peer_socket_addr {
            SocketAddr::V4(_) => inner.udp_protocol_handlers.iter().find_map(|x| {
                if x.0.is_ipv4() {
                    Some(x.1.clone())
                } else {
                    None
                }
            }),
            SocketAddr::V6(_) => inner.udp_protocol_handlers.iter().find_map(|x| {
                if x.0.is_ipv6() {
                    Some(x.1.clone())
                } else {
                    None
                }
            }),
        }
    }
}
