pub mod wrtc;
pub mod ws;

use super::*;
use std::io;

#[derive(Debug)]
pub(super) enum WasmProtocolNetworkConnection {
    //Dummy(DummyNetworkConnection),
    Ws(ws::WebsocketNetworkConnection),
    //WebRTC(wrtc::WebRTCNetworkConnection),
}

impl PlatformProtocolNetworkConnection for WasmProtocolNetworkConnection {
    fn flow(&self) -> Flow {
        match self {
            //            Self::Dummy(d) => d.flow(),
            Self::Ws(w) => w.flow(),
        }
    }

    fn direction(&self) -> Direction {
        match self {
            // WASM can only connect, never accept
            Self::Ws(_) => Direction::Out,
        }
    }

    fn close(&self) -> PinBoxFuture<'_, std::io::Result<NetworkResult<()>>> {
        Box::pin(async move {
            match self {
                //            Self::Dummy(d) => d.close(),
                Self::Ws(w) => w.close().await,
            }
        })
    }

    fn send(&self, message: Bytes) -> PinBoxFuture<'_, std::io::Result<NetworkResult<()>>> {
        Box::pin(async move {
            match self {
                //            Self::Dummy(d) => d.send(message),
                Self::Ws(w) => w.send(message).await,
            }
        })
    }

    fn recv(&self) -> PinBoxFuture<'_, std::io::Result<NetworkResult<Bytes>>> {
        Box::pin(async move {
            match self {
                //            Self::Dummy(d) => d.recv(),
                Self::Ws(w) => w.recv().await,
            }
        })
    }
}

impl WasmProtocolNetworkConnection {
    pub async fn connect(
        registry: VeilidComponentRegistry,
        dial_info: DialInfo,
        timeout_ms: u32,
    ) -> io::Result<NetworkResult<ProtocolNetworkConnection>> {
        match dial_info.protocol_type() {
            ProtocolType::UDP => {
                bail_io_error_other!("UDP dial info is not supported on WASM targets");
            }
            ProtocolType::TCP => {
                bail_io_error_other!("TCP dial info is not supported on WASM targets");
            }
            ProtocolType::WS => {
                ws::WebsocketProtocolHandler::connect(registry, dial_info, timeout_ms).await
            }
            #[cfg(feature = "enable-protocol-wss")]
            ProtocolType::WSS => {
                ws::WebsocketProtocolHandler::connect(registry, dial_info, timeout_ms).await
            }
        }
    }
}
