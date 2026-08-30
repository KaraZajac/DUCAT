use std::array::TryFromSliceError;

use veilid_core::VeilidAPIError;

/// Errors that can occur during protocol operations.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("message length {length} exceeds limit {limit}")]
    MessageTooLarge { length: usize, limit: usize },
    #[error("layout: {0}")]
    Layout(anyhow::Error),
    #[error("capnp: {0}")]
    Capnp(capnp::Error),
    #[error("sign: {0}")]
    Sign(anyhow::Error),
    #[error("verify: {0}")]
    Verify(anyhow::Error),
    #[error("veilid: {0}")]
    Veilid(#[from] VeilidAPIError),
}

/// Result type for protocol operations.
pub type Result<T> = std::result::Result<T, Error>;

impl From<TryFromSliceError> for Error {
    fn from(err: TryFromSliceError) -> Self {
        Self::Layout(err.into())
    }
}

impl From<capnp::Error> for Error {
    fn from(err: capnp::Error) -> Self {
        Self::Capnp(err)
    }
}
