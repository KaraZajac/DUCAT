use std::io;

use tokio::{sync::watch, task::JoinError};
use veilid_core::VeilidAPIError;

/// Errors that can occur during connection operations.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Unrecoverable Veilid error. Even a Connection::reset is unlikely to
    /// resolve. The condition is most likely fatal to the connection instance.
    #[error("veilid (unrecoverable): {0}")]
    Unrecoverable(VeilidAPIError),

    /// Veilid is down or not connected. A Connection::reset may be required to
    /// resolve the condition.
    #[error("veilid (down): {0}")]
    Down(VeilidAPIError),

    /// Veilid routing error.
    ///
    /// For local routes, such an error likely indicates the route has become
    /// invalid, needs to be replaced and re-announced.
    ///
    /// For remote routes, such an error likely indicates the route has become
    /// invalid and needs to be re-resolved.
    #[error("routing: {0}")]
    Routing(VeilidAPIError),

    /// General Veilid API error.
    #[error("veilid: {0}")]
    Veilid(VeilidAPIError),

    /// I/O operation failed.
    #[error("io: {0}")]
    IO(#[from] io::Error),

    /// Watch channel receive error.
    #[error("watch (receive): {0}")]
    WatchReceive(#[from] watch::error::RecvError),

    /// Task join error during close.
    #[error("close: {0}")]
    Close(#[from] JoinError),
}

/// Result type for connection operations.
pub type Result<T> = std::result::Result<T, Error>;

impl TryFrom<Error> for VeilidAPIError {
    type Error = anyhow::Error;

    fn try_from(err: Error) -> std::result::Result<Self, Self::Error> {
        match err {
            Error::Unrecoverable(veilid_apierror) => Ok(veilid_apierror),
            Error::Down(veilid_apierror) => Ok(veilid_apierror),
            Error::Veilid(veilid_apierror) => Ok(veilid_apierror),
            e => Err(anyhow::anyhow!("{}: not a veilid error", e)),
        }
    }
}

impl From<VeilidAPIError> for Error {
    fn from(err: VeilidAPIError) -> Self {
        match err {
            VeilidAPIError::Shutdown => Self::Unrecoverable(err),

            VeilidAPIError::NoConnection { .. } => Self::Down(err),
            VeilidAPIError::NotInitialized => Self::Down(err),

            VeilidAPIError::InvalidTarget { .. } => Self::Routing(err),
            VeilidAPIError::Generic { ref message } => match message.as_str() {
                "can't reach private route any more" => Self::Routing(err),
                "allocated route failed to test" => Self::Routing(err),
                "allocated route could not be tested" => Self::Routing(err),
                _ => Self::Veilid(err),
            },
            VeilidAPIError::TryAgain { ref message } => match message.as_str() {
                "unable to allocate route until we have a valid PublicInternet network class" => {
                    Self::Routing(err)
                }
                "not enough nodes to construct route at this time" => Self::Routing(err),
                "unable to find unique route at this time" => Self::Routing(err),
                "unable to assemble route until we have published peerinfo" => Self::Routing(err),
                _ => Self::Veilid(err),
            },
            VeilidAPIError::Internal { ref message } => match message.as_str() {
                "no best key to test allocated route" => Self::Routing(err),
                "peer info should exist for route but doesn't" => Self::Routing(err),
                "route id does not exist" => Self::Routing(err),
                _ => Self::Veilid(err),
            },
            _ => Self::Veilid(err),
        }
    }
}
