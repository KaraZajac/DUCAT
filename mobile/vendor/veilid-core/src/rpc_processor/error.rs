use super::*;

#[derive(ThisError, Debug, Clone, PartialOrd, PartialEq, Eq, Ord)]
#[must_use]
pub(crate) enum RPCError {
    #[error("[RPCError: Unimplemented({0})]")]
    Unimplemented(String),
    #[error("[RPCError: InvalidFormat({0})]")]
    InvalidFormat(String),
    #[error("[RPCError: Protocol({0})]")]
    Protocol(String),
    #[error("[RPCError: Internal({0})]")]
    Internal(String),
    #[error("[RPCError: Network({0})]")]
    Network(String),
    #[error("[RPCError: TryAgain({0})]")]
    TryAgain(String),
    #[error("[RPCError: Ignore({0})]")]
    Ignore(String),
}

impl RPCError {
    #[expect(dead_code)]
    pub fn unimplemented<X: ToString>(x: X) -> Self {
        Self::Unimplemented(x.to_string())
    }
    pub fn invalid_format<X: ToString>(x: X) -> Self {
        Self::InvalidFormat(x.to_string())
    }
    pub fn map_invalid_format<M: ToString, X: ToString>(message: M) -> impl FnOnce(X) -> Self {
        move |x| Self::InvalidFormat(format!("{}: {}", message.to_string(), x.to_string()))
    }
    pub fn protocol<X: ToString>(x: X) -> Self {
        Self::Protocol(x.to_string())
    }
    pub fn map_protocol<M: ToString, X: ToString>(message: M) -> impl FnOnce(X) -> Self {
        move |x| Self::Protocol(format!("{}: {}", message.to_string(), x.to_string()))
    }
    pub fn internal<X: ToString>(x: X) -> Self {
        Self::Internal(x.to_string())
    }
    pub fn map_internal<M: ToString, X: ToString>(message: M) -> impl FnOnce(X) -> Self {
        move |x| Self::Internal(format!("{}: {}", message.to_string(), x.to_string()))
    }
    pub fn else_internal<M: ToString>(message: M) -> impl FnOnce() -> Self {
        move || Self::Internal(message.to_string())
    }
    pub fn network<X: ToString>(x: X) -> Self {
        Self::Network(x.to_string())
    }
    #[expect(dead_code)]
    pub fn map_network<M: ToString, X: ToString>(message: M) -> impl FnOnce(X) -> Self {
        move |x| Self::Network(format!("{}: {}", message.to_string(), x.to_string()))
    }
    pub fn try_again<X: ToString>(x: X) -> Self {
        Self::TryAgain(x.to_string())
    }
    pub fn map_try_again<M: ToString, X: ToString>(message: M) -> impl FnOnce(X) -> Self {
        move |x| Self::TryAgain(format!("{}: {}", message.to_string(), x.to_string()))
    }
    pub fn ignore<X: ToString>(x: X) -> Self {
        Self::Ignore(x.to_string())
    }
    #[expect(dead_code)]
    pub fn map_ignore<M: ToString, X: ToString>(message: M) -> impl FnOnce(X) -> Self {
        move |x| Self::Ignore(format!("{}: {}", message.to_string(), x.to_string()))
    }
    pub fn else_ignore<M: ToString>(message: M) -> impl FnOnce() -> Self {
        move || Self::Ignore(message.to_string())
    }
}

impl From<RPCError> for VeilidAPIError {
    fn from(e: RPCError) -> Self {
        match e {
            RPCError::Unimplemented(message) => VeilidAPIError::Unimplemented { message },
            RPCError::InvalidFormat(message) => VeilidAPIError::NoConnection { message },
            RPCError::Protocol(message) => VeilidAPIError::NoConnection { message },
            RPCError::Internal(message) => VeilidAPIError::Internal { message },
            RPCError::Network(message) => VeilidAPIError::NoConnection { message },
            RPCError::TryAgain(message) => VeilidAPIError::TryAgain { message },
            RPCError::Ignore(message) => VeilidAPIError::Generic { message },
        }
    }
}

pub type RPCNetworkResult<T> = Result<NetworkResult<T>, RPCError>;

pub(crate) trait ToRPCNetworkResult<T> {
    fn to_rpc_network_result(self) -> RPCNetworkResult<T>;
}

impl<T> ToRPCNetworkResult<T> for VeilidAPIResult<T> {
    fn to_rpc_network_result(self) -> RPCNetworkResult<T> {
        match self {
            Err(VeilidAPIError::TryAgain { message }) => Err(RPCError::TryAgain(message)),
            Err(VeilidAPIError::Timeout) => Ok(NetworkResult::timeout()),
            Err(VeilidAPIError::Unimplemented { message }) => Err(RPCError::Unimplemented(message)),
            Err(e) => Err(RPCError::internal(e)),
            Ok(v) => Ok(NetworkResult::value(v)),
        }
    }
}

impl From<capnp::NotInSchema> for RPCError {
    fn from(_value: capnp::NotInSchema) -> Self {
        RPCError::ignore("not in schema")
    }
}

impl From<capnp::Error> for RPCError {
    fn from(value: capnp::Error) -> Self {
        RPCError::protocol(value)
    }
}

pub trait RPCErrorIgnoreOk<T> {
    fn ignore_ok(self) -> Result<Option<T>, RPCError>;
}

impl<T> RPCErrorIgnoreOk<T> for Result<T, RPCError> {
    fn ignore_ok(self) -> Result<Option<T>, RPCError> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(RPCError::Ignore(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Returns an `RPCError::Ignore` from the enclosing function if the capnp reader lacks `$propname`.
macro_rules! rpc_ignore_missing_property {
    ($reader:expr, $propname:tt) => {
        pastey::paste! {
            if !$reader.[<has_ $propname>]() {
                return Err(RPCError::ignore(concat!("missing ", stringify!($propname))));
            }
        }
    };
}
pub(crate) use rpc_ignore_missing_property;

/// Returns the reader's length, or an `RPCError::Ignore` from the enclosing function if it exceeds `$max_len`.
macro_rules! rpc_ignore_max_len {
    ($reader:expr, $max_len:expr) => {{
        let _len = $reader.len() as usize;
        if _len > $max_len {
            return Err(RPCError::ignore(concat!(
                stringify!($reader),
                " length > ",
                stringify!($max_len)
            )));
        }
        _len
    }};
}
pub(crate) use rpc_ignore_max_len;

/// Returns the reader's length, or an `RPCError::Ignore` from the enclosing function if it falls outside `[$min_len, $max_len]`.
macro_rules! rpc_ignore_min_max_len {
    ($reader:expr, $min_len:expr, $max_len:expr) => {{
        let _len = $reader.len() as usize;
        if _len < $min_len {
            return Err(RPCError::ignore(concat!(
                stringify!($reader),
                " length < ",
                stringify!($min_len)
            )));
        }
        if _len > $max_len {
            return Err(RPCError::ignore(concat!(
                stringify!($reader),
                " length > ",
                stringify!($max_len)
            )));
        }
        _len
    }};
}
pub(crate) use rpc_ignore_min_max_len;
