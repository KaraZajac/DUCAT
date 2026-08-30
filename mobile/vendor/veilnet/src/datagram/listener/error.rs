use veilid_core::VeilidAPIError;

use crate::proto;

/// Errors that can occur during listener operations.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Connection-related error.
    #[error("connection: {0}")]
    Connection(#[from] crate::connection::Error),
    /// Failed to bind to the DHT address.
    #[error("bind failure: {0}")]
    BindFailure(VeilidAPIError),
    /// The private route is no longer valid.
    #[error("dead route")]
    DeadRoute,
    /// Listener has been closed.
    #[error("closed")]
    Closed,
    #[error("proto: {0}")]
    Protocol(proto::Error),
    #[error("watcher: {0}")]
    Watcher(Box<dyn std::error::Error + Send + Sync>),
}

/// Result type for listener operations.
pub type Result<T> = std::result::Result<T, Error>;

impl From<VeilidAPIError> for Error {
    fn from(err: VeilidAPIError) -> Self {
        let conn_err: crate::connection::Error = err.into();
        match conn_err {
            crate::connection::Error::Routing(verr) => Self::BindFailure(verr),
            _ => Self::Connection(conn_err),
        }
    }
}

impl From<proto::Error> for Error {
    fn from(err: proto::Error) -> Self {
        Error::Protocol(err)
    }
}
