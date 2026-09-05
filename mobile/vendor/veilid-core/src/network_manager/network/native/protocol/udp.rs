use super::*;
use stop_token::{StopSource, StopToken};

impl_veilid_log_facility!("net");

#[derive(Clone)]
pub struct RawUdpProtocolHandler {
    /// Component registry for logging and access to other subsystems
    registry: VeilidComponentRegistry,
    /// Async UDP socket this handler reads from and writes to
    socket: Arc<UdpSocket>,
    /// Reassembly buffer for fragmented UDP messages
    assembly_buffer: AssemblyBuffer,
    /// True if `socket` is IPv6, false for IPv4
    is_ipv6: bool,
    /// TTL the socket was created with; restored after holepunch sends
    default_ttl: u32,
    /// Current TTL on the socket; mutated transiently by `send_hole_punch`
    current_ttl: Arc<AsyncMutex<u32>>,
    /// Drop to signal cancellation; taken out by `stop()`
    stop_source: Arc<Mutex<Option<StopSource>>>,
    /// Cloned by recv-loops that follow this handler's stop
    stop_token: StopToken,
}

impl_veilid_component_accessors!(RawUdpProtocolHandler);

impl RawUdpProtocolHandler {
    pub fn new(registry: VeilidComponentRegistry, socket: Arc<UdpSocket>, is_ipv6: bool) -> Self {
        // Get original TTL
        let default_ttl = if is_ipv6 {
            socket2_operation(socket.as_ref(), |s| s.unicast_hops_v6())
                .expect_or_log("getting IPV6_UNICAST_HOPS should not fail")
        } else {
            socket2_operation(socket.as_ref(), |s| s.ttl_v4())
                .expect_or_log("getting IP_TTL should not fail")
        };

        let stop_source = StopSource::new();
        let stop_token = stop_source.token();

        Self {
            registry,
            socket,
            assembly_buffer: AssemblyBuffer::new(),
            is_ipv6,
            default_ttl,
            current_ttl: Arc::new(AsyncMutex::new(default_ttl)),
            stop_source: Arc::new(Mutex::new(Some(stop_source))),
            stop_token,
        }
    }

    /// Stop token for this handler's recv-loop
    pub fn stop_token(&self) -> StopToken {
        self.stop_token.clone()
    }

    /// Signal cancellation for this handler. Idempotent.
    pub fn stop(&self) {
        *self.stop_source.lock() = None;
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "protocol", err, skip(self, data), fields(self.socket = ?self.socket, data.len = data.len(), ret.len, ret.flow)))]
    pub async fn recv_message(&self, data: &mut [u8]) -> io::Result<(usize, Flow)> {
        let (message_len, flow) = loop {
            // Get a packet
            let (size, remote_addr) = network_result_value_or_log!(self self.socket.recv_from(data).await.into_network_result()? => continue);

            // Check to see if it is punished
            if self
                .network_manager()
                .address_filter()
                .is_ip_addr_punished(remote_addr.ip())
            {
                continue;
            }

            // Insert into assembly buffer
            let message = match self
                .assembly_buffer
                .insert_frame(&data[0..size], remote_addr)
            {
                NetworkResult::Value(Some(v)) => v,
                NetworkResult::Value(None) => {
                    continue;
                }
                nres => {
                    veilid_log!(self debug target:"network_result",
                        "UDP::recv_message insert_frame failed: {:?} <= size={} remote_addr={}",
                        nres,
                        size,
                        remote_addr
                    );
                    continue;
                }
            };

            // Check length of reassembled message (same for all protocols)
            if message.len() > MAX_MESSAGE_SIZE {
                veilid_log!(self debug "{}({}) at {}@{}:{}", "Invalid message", "received too large UDP message", file!(), line!(), column!());
                continue;
            }

            // Copy assemble message out if we got one
            data[0..message.len()].copy_from_slice(&message);

            // Return a flow and the amount of data in the message
            let peer_addr = PeerAddress::new(
                SocketAddress::from_socket_addr(remote_addr),
                ProtocolType::UDP,
            );
            let local_socket_addr = self.socket.local_addr()?;
            let flow = Flow::new(
                peer_addr,
                SocketAddress::from_socket_addr(local_socket_addr),
            );

            break (message.len(), flow);
        };

        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.len", message_len);
        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.flow", format!("{:?}", flow).as_str());
        Ok((message_len, flow))
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "protocol", err, skip(self, data), fields(self.socket = ?self.socket, data.len = data.len(), ret.flow)))]
    pub async fn send_message(
        &self,
        data: Bytes,
        remote_addr: SocketAddr,
    ) -> io::Result<NetworkResult<Flow>> {
        if data.len() > MAX_MESSAGE_SIZE {
            bail_io_error_other!("sending too large UDP message");
        }

        // Check to see if it is punished
        if self
            .network_manager()
            .address_filter()
            .is_ip_addr_punished(remote_addr.ip())
        {
            return Ok(NetworkResult::no_connection_other("punished"));
        }

        // Ensure the TTL for sent packets is the default,
        // then fragment and send the packets
        {
            let current_ttl = self.current_ttl.lock().await;
            if *current_ttl != self.default_ttl {
                veilid_log!(self error "Incorrect TTL on sent UDP packet ({} != {}): len={}, remote_addr={:?}", *current_ttl, self.default_ttl, data.len(), remote_addr);
            }

            // Fragment and send
            let sender = |framed_chunk: Bytes, remote_addr: SocketAddr| async move {
                let len = network_result_try!(self
                    .socket
                    .send_to(&framed_chunk, remote_addr)
                    .await
                    .into_network_result()?);
                if len != framed_chunk.len() {
                    bail_io_error_other!("UDP partial send")
                }

                veilid_log!(self trace "udp::send_message:chunk(len={}) {:?}", len, remote_addr);
                Ok(NetworkResult::value(()))
            };

            network_result_try!(
                self.assembly_buffer
                    .split_message(data, remote_addr, sender)
                    .await?
            );
        }

        // Return a flow for the sent message
        let peer_addr = PeerAddress::new(
            SocketAddress::from_socket_addr(remote_addr),
            ProtocolType::UDP,
        );
        let local_socket_addr = self.socket.local_addr()?;

        let flow = Flow::new(
            peer_addr,
            SocketAddress::from_socket_addr(local_socket_addr),
        );

        veilid_log!(self trace "udp::send_message: {:?}", flow);

        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.flow", format!("{:?}", flow).as_str());
        Ok(NetworkResult::value(flow))
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "protocol", err, skip(self), fields(self.socket = ?self.socket, ret.flow)))]
    pub async fn send_hole_punch(
        &self,
        remote_addr: SocketAddr,
        ttl: u32,
    ) -> io::Result<NetworkResult<Flow>> {
        // Check to see if it is punished
        if self
            .network_manager()
            .address_filter()
            .is_ip_addr_punished(remote_addr.ip())
        {
            return Ok(NetworkResult::no_connection_other("punished"));
        }

        // Ensure the TTL for sent packets is the default,
        // then fragment and send the packets
        let res = {
            let mut current_ttl = self.current_ttl.lock().await;
            if *current_ttl != self.default_ttl {
                veilid_log!(self error "Incorrect TTL before sending holepunch UDP packet ({} != {}): remote_addr={:?}", *current_ttl, self.default_ttl, remote_addr);
            }

            // Get synchronous socket
            socket2_operation(self.socket.as_ref(), |s| {
                // Set TTL
                let ttl_res = if self.is_ipv6 {
                    s.set_unicast_hops_v6(ttl)
                } else {
                    s.set_ttl_v4(ttl)
                };
                ttl_res.inspect_err(|e| {
                    veilid_log!(self error "Failed to set TTL on holepunch UDP socket: {} remote_addr={:?}", e, remote_addr);
                })?;
                *current_ttl = ttl;

                // Send zero length packet
                let res = s.send_to(&[], &remote_addr.into());

                // Restore TTL immediately
                let ttl_res = if self.is_ipv6 {
                    s.set_unicast_hops_v6(self.default_ttl)
                } else {
                    s.set_ttl_v4(self.default_ttl)
                };
                ttl_res.inspect_err(|e| {
                    veilid_log!(self error "Failed to reset TTL on holepunch UDP socket: {} remote_addr={:?}", e, remote_addr);
                })?;
                *current_ttl = self.default_ttl;

                res
            })
        };

        // Check for errors
        let len = network_result_try!(res.into_network_result()?);
        if len != 0 {
            bail_io_error_other!("wrong size send");
        }

        // Return a flow for the sent message
        let peer_addr = PeerAddress::new(
            SocketAddress::from_socket_addr(remote_addr),
            ProtocolType::UDP,
        );
        let local_socket_addr = self.socket.local_addr()?;

        let flow = Flow::new(
            peer_addr,
            SocketAddress::from_socket_addr(local_socket_addr),
        );

        veilid_log!(self trace "udp::send_hole_punch: {:?}", flow);

        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("ret.flow", format!("{:?}", flow).as_str());
        Ok(NetworkResult::value(flow))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "protocol", skip(registry), err, fields(__VEILID_LOG_KEY = registry.log_key()))
    )]
    pub fn new_unspecified_bound_handler(
        registry: VeilidComponentRegistry,
        socket_addr: &SocketAddr,
    ) -> io::Result<RawUdpProtocolHandler> {
        // get local wildcard address for bind
        let local_socket_addr = compatible_unspecified_socket_addr(socket_addr);
        let socket = bind_async_udp_socket(local_socket_addr)?
            .ok_or(io::Error::from(io::ErrorKind::AddrInUse))?;
        Ok(RawUdpProtocolHandler::new(
            registry,
            Arc::new(socket),
            local_socket_addr.is_ipv6(),
        ))
    }

    /// Bind a UDP socket to a specific local source address
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "protocol", skip(registry), err, fields(__VEILID_LOG_KEY = registry.log_key()))
    )]
    pub fn new_specific_bound_handler(
        registry: VeilidComponentRegistry,
        local_socket_addr: SocketAddr,
    ) -> io::Result<RawUdpProtocolHandler> {
        let socket = bind_async_udp_socket(local_socket_addr)?
            .ok_or(io::Error::from(io::ErrorKind::AddrInUse))?;
        Ok(RawUdpProtocolHandler::new(
            registry,
            Arc::new(socket),
            local_socket_addr.is_ipv6(),
        ))
    }
}
