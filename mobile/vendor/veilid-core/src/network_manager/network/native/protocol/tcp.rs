use super::*;
use futures_util::{AsyncReadExt, AsyncWriteExt};

pub struct RawTcpNetworkConnection {
    registry: VeilidComponentRegistry,
    flow: Flow,
    direction: Direction,
    stream: Mutex<Option<AsyncPeekStream>>,
}

impl fmt::Debug for RawTcpNetworkConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawTcpNetworkConnection")
            //.field("registry", &self.registry)
            .field("flow", &self.flow)
            .field("direction", &self.direction)
            //.field("stream", &self.stream)
            .finish()
    }
}

impl_veilid_component_accessors!(RawTcpNetworkConnection);

impl RawTcpNetworkConnection {
    pub fn new(
        registry: VeilidComponentRegistry,
        flow: Flow,
        direction: Direction,
        stream: AsyncPeekStream,
    ) -> Self {
        Self {
            registry,
            flow,
            direction,
            stream: Mutex::new(Some(stream)),
        }
    }

    pub fn flow(&self) -> Flow {
        self.flow
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "protocol", err, skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub fn close(&self) -> io::Result<NetworkResult<()>> {
        // Drop the stream, without calling close, which calls shutdown, which causes TIME_WAIT regardless of SO_LINGER settings
        drop(self.stream.lock().take());
        // let _ = stream.close().await;
        Ok(NetworkResult::value(()))
    }

    async fn send_internal(
        stream: &mut AsyncPeekStream,
        message: Bytes,
    ) -> io::Result<NetworkResult<()>> {
        if message.len() > MAX_MESSAGE_SIZE {
            bail_io_error_other!("sending too large TCP message");
        }

        let len = message.len() as u16;
        let header = [b'V', b'L', len as u8, (len >> 8) as u8];

        let mut data = Vec::with_capacity(message.len() + 4);
        data.extend_from_slice(&header);
        data.extend_from_slice(&message);

        network_result_try!(stream.write_all(&data).await.into_network_result()?);

        stream.flush().await.into_network_result()
    }

    #[cfg_attr(feature = "instrument", instrument(level="trace", target="protocol", err, skip(self, message), fields(network_result, message.len = message.len())))]
    pub async fn send(&self, message: Bytes) -> io::Result<NetworkResult<()>> {
        let Some(mut stream) = self.stream.lock().clone() else {
            bail_io_error_other!("already closed");
        };
        let out = Self::send_internal(&mut stream, message).await?;
        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("network_result", tracing::field::display(&out));
        Ok(out)
    }

    async fn recv_internal(stream: &mut AsyncPeekStream) -> io::Result<NetworkResult<Bytes>> {
        let mut header = [0u8; 4];

        network_result_try!(stream.read_exact(&mut header).await.into_network_result()?);
        if header[0] != b'V' || header[1] != b'L' {
            return Ok(NetworkResult::invalid_message(format!(
                "received invalid TCP frame header: {:02x?}",
                header
            )));
        }
        let len = ((header[3] as usize) << 8) | (header[2] as usize);
        if len > MAX_MESSAGE_SIZE {
            return Ok(NetworkResult::invalid_message(format!(
                "received too large TCP frame: len={} header={:02x?}",
                len, header
            )));
        }

        let mut out = BytesMut::zeroed(len);
        let nrout = stream.read_exact(&mut out).await.into_network_result()?;
        network_result_try!(nrout);

        Ok(NetworkResult::Value(out.into()))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "protocol", err, skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn recv(&self) -> io::Result<NetworkResult<Bytes>> {
        let Some(mut stream) = self.stream.lock().clone() else {
            bail_io_error_other!("already closed");
        };
        let out = Self::recv_internal(&mut stream).await?;
        #[cfg(feature = "verbose-tracing")]
        tracing::Span::current().record("network_result", tracing::field::display(&out));
        Ok(out)
    }
}

///////////////////////////////////////////////////////////

#[derive(Clone)]
pub struct RawTcpProtocolHandler
where
    Self: ProtocolAcceptHandler,
{
    registry: VeilidComponentRegistry,
    connection_initial_timeout_ms: u32,
}

impl_veilid_component_accessors!(RawTcpProtocolHandler);

impl RawTcpProtocolHandler {
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        let connection_initial_timeout_ms = registry
            .config()
            .internal()
            .network
            .connection_initial_timeout_ms;
        Self {
            registry,
            connection_initial_timeout_ms,
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "protocol", ret, err, skip(self, ps), fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn on_accept_async(
        self,
        ps: AsyncPeekStream,
        socket_addr: SocketAddr,
        local_addr: SocketAddr,
    ) -> io::Result<Option<ProtocolNetworkConnection>> {
        veilid_log!(self trace "TCP: on_accept_async: enter");
        let mut peekbuf: [u8; PEEK_DETECT_LEN] = [0u8; PEEK_DETECT_LEN];
        if (timeout(
            self.connection_initial_timeout_ms,
            ps.peek_exact(&mut peekbuf).in_current_span(),
        )
        .await)
            .is_err()
        {
            return Ok(None);
        }

        // Ensure this has a chance of being proper framed, otherwise drop the connection
        // This will keep upgraded WS->WSS TLS negotiations from getting punished if the
        // WSS accept handler isn't enabled
        if peekbuf[0] != b'V' || peekbuf[1] != b'L' {
            // Not framed TCP, drop it
            return Ok(None);
        }

        let peer_addr = PeerAddress::new(
            SocketAddress::from_socket_addr(socket_addr),
            ProtocolType::TCP,
        );
        let flow = Flow::new(peer_addr, SocketAddress::from_socket_addr(local_addr));
        veilid_log!(self trace target: "net", "RawTcp accept (inbound) flow: {:?}", flow);
        let conn = NativeProtocolNetworkConnection::RawTcp(RawTcpNetworkConnection::new(
            self.registry(),
            flow,
            Direction::In,
            ps,
        ));

        Ok(Some(Box::new(conn)))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "protocol", skip(registry), ret, err, fields(__VEILID_LOG_KEY = registry.log_key()))
    )]
    pub async fn connect(
        registry: VeilidComponentRegistry,
        local_address: Option<SocketAddr>,
        remote_address: SocketAddr,
        timeout_ms: u32,
    ) -> io::Result<NetworkResult<ProtocolNetworkConnection>> {
        // Non-blocking connect to remote address
        let tcp_stream = network_result_try!(connect_async_tcp_stream(
            local_address,
            remote_address,
            timeout_ms
        )
        .await
        .folded()?);

        // See what local address we ended up with and turn this into a stream
        let actual_local_address = tcp_stream.local_addr()?;
        #[cfg(feature = "rt-tokio")]
        let tcp_stream = tcp_stream.compat();
        let ps = AsyncPeekStream::new(tcp_stream);

        // Wrap the stream in a network connection and return it
        let flow = Flow::new(
            PeerAddress::new(
                SocketAddress::from_socket_addr(remote_address),
                ProtocolType::TCP,
            ),
            SocketAddress::from_socket_addr(actual_local_address),
        );
        veilid_log!(registry trace target: "net", "RawTcp connect (outbound) flow: {:?}", flow);

        let conn = NativeProtocolNetworkConnection::RawTcp(RawTcpNetworkConnection::new(
            registry,
            flow,
            Direction::Out,
            ps,
        ));

        Ok(NetworkResult::Value(Box::new(conn)))
    }
}

impl ProtocolAcceptHandler for RawTcpProtocolHandler {
    fn on_accept(
        &self,
        stream: AsyncPeekStream,
        peer_addr: SocketAddr,
        local_addr: SocketAddr,
    ) -> PinBoxFutureStatic<io::Result<Option<ProtocolNetworkConnection>>> {
        Box::pin(self.clone().on_accept_async(stream, peer_addr, local_addr))
    }
}
