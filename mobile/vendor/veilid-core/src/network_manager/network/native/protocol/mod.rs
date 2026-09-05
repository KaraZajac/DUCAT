pub mod tcp;
pub mod udp;
pub mod wrtc;
pub mod ws;

use super::*;
use std::io;

#[derive(Debug)]
pub(super) enum NativeProtocolNetworkConnection {
    //    Dummy(DummyNetworkConnection),
    RawTcp(tcp::RawTcpNetworkConnection),
    WsAccepted(ws::WebSocketNetworkConnectionAccepted),
    Ws(ws::WebsocketNetworkConnectionWS),
    #[cfg(feature = "enable-protocol-wss")]
    Wss(ws::WebsocketNetworkConnectionWSS),
    //WebRTC(wrtc::WebRTCNetworkConnection),
}

impl PlatformProtocolNetworkConnection for NativeProtocolNetworkConnection {
    fn flow(&self) -> Flow {
        match self {
            //            Self::Dummy(d) => d.flow(),
            Self::RawTcp(t) => t.flow(),
            Self::WsAccepted(w) => w.flow(),
            Self::Ws(w) => w.flow(),
            #[cfg(feature = "enable-protocol-wss")]
            Self::Wss(w) => w.flow(),
        }
    }

    fn direction(&self) -> Direction {
        match self {
            // RawTcp accepts and connects; the rest are inbound-accept or outbound-connect only
            Self::RawTcp(t) => t.direction(),
            Self::WsAccepted(_) => Direction::In,
            Self::Ws(_) => Direction::Out,
            #[cfg(feature = "enable-protocol-wss")]
            Self::Wss(_) => Direction::Out,
        }
    }

    fn close(&self) -> PinBoxFuture<'_, std::io::Result<NetworkResult<()>>> {
        Box::pin(async move {
            match self {
                //            Self::Dummy(d) => d.close(),
                Self::RawTcp(t) => t.close(),
                Self::WsAccepted(w) => w.close().await,
                Self::Ws(w) => w.close().await,
                #[cfg(feature = "enable-protocol-wss")]
                Self::Wss(w) => w.close().await,
            }
        })
    }

    fn send(&self, message: Bytes) -> PinBoxFuture<'_, std::io::Result<NetworkResult<()>>> {
        Box::pin(async move {
            match self {
                //            Self::Dummy(d) => d.send(message),
                Self::RawTcp(t) => t.send(message).await,
                Self::WsAccepted(w) => w.send(message).await,
                Self::Ws(w) => w.send(message).await,
                #[cfg(feature = "enable-protocol-wss")]
                Self::Wss(w) => w.send(message).await,
            }
        })
    }
    fn recv(&self) -> PinBoxFuture<'_, std::io::Result<NetworkResult<Bytes>>> {
        Box::pin(async move {
            match self {
                //            Self::Dummy(d) => d.recv(),
                Self::RawTcp(t) => t.recv().await,
                Self::WsAccepted(w) => w.recv().await,
                Self::Ws(w) => w.recv().await,
                #[cfg(feature = "enable-protocol-wss")]
                Self::Wss(w) => w.recv().await,
            }
        })
    }
}

impl NativeProtocolNetworkConnection {
    pub async fn connect(
        registry: VeilidComponentRegistry,
        local_address: Option<SocketAddr>,
        dial_info: DialInfo,
        timeout_ms: u32,
    ) -> io::Result<NetworkResult<ProtocolNetworkConnection>> {
        match dial_info.protocol_type() {
            ProtocolType::UDP => {
                veilid_log!(registry error "Should not 'connect' to UDP dialinfo");
                Ok(NetworkResult::service_unavailable(
                    "UDP dialinfo does not support 'connect'",
                ))
            }
            ProtocolType::TCP => {
                tcp::RawTcpProtocolHandler::connect(
                    registry,
                    local_address,
                    dial_info.to_socket_addr(),
                    timeout_ms,
                )
                .await
            }
            ProtocolType::WS => {
                ws::WebsocketProtocolHandler::connect(
                    registry,
                    local_address,
                    dial_info,
                    timeout_ms,
                )
                .await
            }
            #[cfg(feature = "enable-protocol-wss")]
            ProtocolType::WSS => {
                ws::WebsocketProtocolHandler::connect(
                    registry,
                    local_address,
                    dial_info,
                    timeout_ms,
                )
                .await
            }
        }
    }
}
