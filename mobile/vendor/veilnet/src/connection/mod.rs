use async_trait::async_trait;

pub use api::API;
pub use error::{Error, Result};
pub use routing_context::RoutingContext;
pub use updates::{HandlerChain, StateAttachmentWatcher, UpdateDispatch, UpdateHandler};
pub use veilid::connection::Connection as Veilid;
use veilid_core::{CRYPTO_KIND_VLD0, CryptoKind, CryptoSystemGuard, VeilidAPIError};

mod api;
mod routing_context;
mod updates;

pub mod error;
pub mod veilid;

// Present a connection for interacting with an embedded Veilid node.
//
// A Connection allows handling Veilid update events, interacting with API,
// resetting node and shutting down allocated resources.
#[async_trait]
pub trait Connection {
    fn add_update_handler(&self, handler: Box<dyn UpdateHandler + Send + Sync>);
    async fn require_attachment(&mut self) -> Result<()>;
    fn routing_context(&self) -> impl RoutingContext + Send;
    async fn reset(&mut self) -> Result<()>;
    async fn close(self) -> Result<()>;

    fn crypto_kind(&self) -> CryptoKind {
        CRYPTO_KIND_VLD0
    }

    fn with_crypto<T, E, F>(&self, f: F) -> std::result::Result<T, E>
    where
        E: From<VeilidAPIError>,
        F: Fn(CryptoSystemGuard<'_>) -> std::result::Result<T, E>,
    {
        let rc = self.routing_context();
        let api = rc.api();
        let crypto = api.crypto()?;
        let crypto = crypto
            .get(self.crypto_kind())
            .ok_or(VeilidAPIError::Generic {
                message: "crypto kind unavailable".to_string(),
            })?;
        f(crypto)
    }
}

/// Entities which have an underlying Connection.
pub trait Connected<C>
where
    C: Connection + Send,
{
    fn conn(&self) -> &C;
    fn conn_mut(&mut self) -> &mut C;
}

#[cfg(any(feature = "testing", test))]
pub mod testing;
