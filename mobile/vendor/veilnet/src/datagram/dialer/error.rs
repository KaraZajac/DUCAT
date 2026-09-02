use veilid_core::VeilidAPIError;

use crate::{DHTAddr, proto};

/// Errors that can occur during dialing operations.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Connection-related error.
    #[error("connection: {0}")]
    Connection(#[from] crate::connection::Error),
    /// Route resolution or routing error.
    #[error("bad route: {0}")]
    BadRoute(VeilidAPIError),
    /// No route found at the specified DHT address.
    #[error("resolve: route not found at address {0}")]
    RouteNotFound(DHTAddr),
    #[error("proto: {0}")]
    Protocol(proto::Error),
}

/// Result type for dialer operations.
pub type Result<T> = std::result::Result<T, Error>;

impl From<VeilidAPIError> for Error {
    fn from(err: VeilidAPIError) -> Self {
        let conn_err: crate::connection::Error = err.into();
        match conn_err {
            crate::connection::Error::Routing(verr) => Self::BadRoute(verr),
            _ => Self::Connection(conn_err),
        }
    }
}

impl From<proto::Error> for Error {
    fn from(err: proto::Error) -> Self {
        Error::Protocol(err)
    }
}
