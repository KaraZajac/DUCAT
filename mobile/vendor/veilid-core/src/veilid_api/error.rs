use super::*;

/// Return early with a [VeilidAPIError::NotInitialized] error.
#[macro_export]
macro_rules! apibail_not_initialized {
    () => {
        return Err(VeilidAPIError::not_initialized())
    };
}

/// Return early with a [VeilidAPIError::Timeout] error.
#[macro_export]
macro_rules! apibail_timeout {
    () => {
        return Err(VeilidAPIError::timeout())
    };
}

/// Return early with a [VeilidAPIError::TryAgain] error carrying the given message.
#[macro_export]
macro_rules! apibail_try_again {
    ($x:expr) => {
        return Err(VeilidAPIError::try_again($x))
    };
    ($fmt:literal, $($args:tt)*) => {
        return Err(VeilidAPIError::try_again( format!($fmt, $($args)*) ))
    };
}

/// Return early with a [VeilidAPIError::Generic] error carrying the given message.
#[macro_export]
macro_rules! apibail_generic {
    ($x:expr) => {
        return Err(VeilidAPIError::generic($x))
    };
    ($fmt:literal, $($args:tt)*) => {
        return Err(VeilidAPIError::generic( format!($fmt, $($args)*) ))
    };
}

/// Return early with a [VeilidAPIError::Internal] error carrying the given message.
#[macro_export]
macro_rules! apibail_internal {
    ($x:expr) => {
        return Err(VeilidAPIError::internal($x))
    };
    ($fmt:literal, $($args:tt)*) => {
        return Err(VeilidAPIError::internal( format!($fmt, $($args)*) ))
    };
}

/// Return early with a [VeilidAPIError::ParseError] error carrying the given message and value.
#[macro_export]
macro_rules! apibail_parse_error {
    ($x:expr, $y:expr) => {
        return Err(VeilidAPIError::parse_error($x, $y))
    };
}

/// Return early with a [VeilidAPIError::MissingArgument] error naming the calling context and the missing argument.
#[macro_export]
macro_rules! apibail_missing_argument {
    ($x:expr, $y:expr) => {
        return Err(VeilidAPIError::missing_argument($x, $y))
    };
}

/// Return early with a [VeilidAPIError::InvalidArgument] error naming the calling context, the argument, and its rejected value.
#[macro_export]
macro_rules! apibail_invalid_argument {
    ($x:expr, $y:expr, $z:expr) => {
        return Err(VeilidAPIError::invalid_argument($x, $y, $z))
    };
}

/// Return early with a [VeilidAPIError::NoConnection] error carrying the given message.
#[macro_export]
macro_rules! apibail_no_connection {
    ($x:expr) => {
        return Err(VeilidAPIError::no_connection($x))
    };
    ($fmt:literal, $($args: tt)* ) => {
        return Err(VeilidAPIError::no_connection( format!($fmt, arg $($args)*) ))
    };

}

/// Return early with a [VeilidAPIError::KeyNotFound] error carrying the missing record key.
#[macro_export]
macro_rules! apibail_key_not_found {
    ($x:expr) => {
        return Err(VeilidAPIError::key_not_found($x))
    };
}

/// Return early with a [VeilidAPIError::InvalidTarget] error carrying the given message.
#[macro_export]
macro_rules! apibail_invalid_target {
    ($x:expr) => {
        return Err(VeilidAPIError::invalid_target($x))
    };
}

/// Return early with a [VeilidAPIError::TransactionNotFound] error carrying the given message.
#[macro_export]
macro_rules! apibail_transaction_not_found {
    ($x:expr) => {
        return Err(VeilidAPIError::transaction_not_found($x))
    };
    ($fmt:literal, $($args:tt)*) => {
        return Err(VeilidAPIError::transaction_not_found( format!($fmt, $($args)*) ))
    };
}

/// Return early with a [VeilidAPIError::AlreadyInitialized] error.
#[macro_export]
macro_rules! apibail_already_initialized {
    () => {
        return Err(VeilidAPIError::already_initialized())
    };
}

/// Error type returned by all fallible Veilid API operations.
#[apply(api_data_enum!)]
#[api(eq, ord, ts(into_wasm_abi))]
#[derive(ThisError)]
#[serde(tag = "kind")]
pub enum VeilidAPIError {
    /// The API was used before [VeilidAPI](crate::VeilidAPI) was attached, or after it was detached.
    #[error("Not initialized")]
    NotInitialized,
    /// An attempt was made to initialize Veilid while it was already running.
    #[error("Already initialized")]
    AlreadyInitialized,
    /// The operation did not complete within its time budget.
    #[error("Timeout")]
    Timeout,
    /// The operation could not be completed yet and should be retried later.
    #[error("TryAgain: {message}")]
    TryAgain {
        /// Why the operation could not complete this time.
        message: String,
    },
    /// The destination for an operation could not be reached or is malformed.
    #[error("Invalid target: {message}")]
    InvalidTarget {
        /// Details about the unreachable or malformed target.
        message: String,
    },
    /// No network connection could be established to carry out the operation.
    #[error("No connection: {message}")]
    NoConnection {
        /// Details about the connection failure.
        message: String,
    },
    /// The API is shutting down and can no longer service requests.
    #[error("Shutdown")]
    Shutdown,
    /// The requested DHT record key is not present in local storage.
    #[error("Key not found: {key}")]
    KeyNotFound {
        /// The record key that was not found.
        #[cfg_attr(feature = "schemars", schemars(with = "String"))]
        key: OpaqueRecordKey,
    },
    /// An internal invariant was violated; indicates a bug in Veilid itself.
    #[error("Internal: {message}")]
    Internal {
        /// Details about the internal failure.
        message: String,
    },
    /// The requested feature exists in the API surface but is not yet implemented on this platform or build.
    #[error("Unimplemented: {message}")]
    Unimplemented {
        /// Which functionality is unimplemented.
        message: String,
    },
    /// A value could not be parsed into its expected form.
    #[error("Parse error: '{message}' with value '{value}'")]
    ParseError {
        /// What went wrong while parsing.
        message: String,
        /// The input value that failed to parse.
        value: String,
    },
    /// An argument was supplied but its value was rejected.
    #[error("Invalid argument: '{context}' for '{argument}' with value '{value}'")]
    InvalidArgument {
        /// The calling context that rejected the argument.
        context: String,
        /// The name of the offending argument.
        argument: String,
        /// The rejected value.
        value: String,
    },
    /// A required argument was not supplied.
    #[error("Missing argument: '{context}' for '{argument}'")]
    MissingArgument {
        /// The calling context that required the argument.
        context: String,
        /// The name of the missing argument.
        argument: String,
    },
    /// A failure that does not fit any more specific category.
    #[error("Generic: {message}")]
    Generic {
        /// Details about the failure.
        message: String,
    },
    /// The referenced DHT transaction does not exist, having expired or never been opened.
    #[error("Transaction not found: {message}")]
    TransactionNotFound {
        /// Details about the missing transaction.
        message: String,
    },
}

impl VeilidAPIError {
    /// Construct a [VeilidAPIError::NotInitialized] error.
    pub fn not_initialized() -> Self {
        Self::NotInitialized
    }
    /// Construct a [VeilidAPIError::AlreadyInitialized] error.
    pub fn already_initialized() -> Self {
        Self::AlreadyInitialized
    }
    /// Construct a [VeilidAPIError::Timeout] error.
    pub fn timeout() -> Self {
        Self::Timeout
    }
    /// Construct a [VeilidAPIError::TryAgain] error with the given message.
    pub fn try_again<T: ToVeilidAPIErrorArgument>(msg: T) -> Self {
        Self::TryAgain {
            message: msg.to_veilid_api_error_argument(),
        }
    }
    /// Construct a [VeilidAPIError::Shutdown] error.
    pub fn shutdown() -> Self {
        Self::Shutdown
    }
    /// Construct a [VeilidAPIError::InvalidTarget] error with the given message.
    pub fn invalid_target<T: ToVeilidAPIErrorArgument>(msg: T) -> Self {
        Self::InvalidTarget {
            message: msg.to_veilid_api_error_argument(),
        }
    }
    /// Construct a [VeilidAPIError::NoConnection] error with the given message.
    pub fn no_connection<T: ToVeilidAPIErrorArgument>(msg: T) -> Self {
        Self::NoConnection {
            message: msg.to_veilid_api_error_argument(),
        }
    }
    /// Construct a [VeilidAPIError::KeyNotFound] error for the given record key.
    pub fn key_not_found(key: OpaqueRecordKey) -> Self {
        Self::KeyNotFound { key }
    }
    /// Construct a [VeilidAPIError::ParseError] error with the given message and offending value.
    pub fn parse_error<T: ToVeilidAPIErrorArgument, S: ToVeilidAPIErrorArgument>(
        msg: T,
        value: S,
    ) -> Self {
        Self::ParseError {
            message: msg.to_veilid_api_error_argument(),
            value: value.to_veilid_api_error_argument(),
        }
    }
    /// Construct a [VeilidAPIError::InvalidArgument] error naming the context, argument, and rejected value.
    pub fn invalid_argument<
        T: ToVeilidAPIErrorArgument,
        S: ToVeilidAPIErrorArgument,
        R: ToVeilidAPIErrorArgument,
    >(
        context: T,
        argument: S,
        value: R,
    ) -> Self {
        Self::InvalidArgument {
            context: context.to_veilid_api_error_argument(),
            argument: argument.to_veilid_api_error_argument(),
            value: value.to_veilid_api_error_argument(),
        }
    }
    /// Construct a [VeilidAPIError::MissingArgument] error naming the context and the missing argument.
    pub fn missing_argument<T: ToVeilidAPIErrorArgument, S: ToVeilidAPIErrorArgument>(
        context: T,
        argument: S,
    ) -> Self {
        Self::MissingArgument {
            context: context.to_veilid_api_error_argument(),
            argument: argument.to_veilid_api_error_argument(),
        }
    }
    /// Construct a [VeilidAPIError::Generic] error with the given message.
    pub fn generic<T: ToVeilidAPIErrorArgument>(msg: T) -> Self {
        Self::Generic {
            message: msg.to_veilid_api_error_argument(),
        }
    }
    pub(crate) fn transaction_not_found<T: ToVeilidAPIErrorArgument>(msg: T) -> Self {
        Self::TransactionNotFound {
            message: msg.to_veilid_api_error_argument(),
        }
    }

    /// Convert a [NetworkResult] into a [VeilidAPIResult], mapping each non-value outcome to the matching error variant.
    pub fn from_network_result<T>(nr: NetworkResult<T>) -> Result<T, Self> {
        match nr {
            NetworkResult::Timeout => Err(VeilidAPIError::timeout()),
            NetworkResult::ServiceUnavailable(m) => Err(VeilidAPIError::invalid_target(m)),
            NetworkResult::NoConnection(m) => Err(VeilidAPIError::no_connection(m.to_string())),
            NetworkResult::AlreadyExists(m) => Err(VeilidAPIError::no_connection(format!(
                "Already exists: {}",
                m
            ))),
            NetworkResult::InvalidMessage(m) => {
                Err(VeilidAPIError::parse_error("Invalid message", m))
            }
            NetworkResult::Value(v) => Ok(v),
        }
    }

    /// The [tracing] log level at which this error should be reported.
    pub fn log_level(&self) -> Level {
        match self {
            VeilidAPIError::NotInitialized
            | VeilidAPIError::AlreadyInitialized
            | VeilidAPIError::InvalidTarget { message: _ }
            | VeilidAPIError::Internal { message: _ }
            | VeilidAPIError::Generic { message: _ }
            | VeilidAPIError::ParseError {
                message: _,
                value: _,
            }
            | VeilidAPIError::InvalidArgument {
                context: _,
                argument: _,
                value: _,
            }
            | VeilidAPIError::MissingArgument {
                context: _,
                argument: _,
            }
            | VeilidAPIError::Shutdown => Level::ERROR,

            VeilidAPIError::NoConnection { message: _ }
            | VeilidAPIError::KeyNotFound { key: _ }
            | VeilidAPIError::Unimplemented { message: _ } => Level::WARN,

            VeilidAPIError::Timeout
            | VeilidAPIError::TryAgain { message: _ }
            | VeilidAPIError::TransactionNotFound { message: _ } => Level::DEBUG,
        }
    }

    /// Construct a [VeilidAPIError::Internal] error with the given message. Constructing one signals a bug in Veilid.
    pub fn internal<T: ToString>(msg: T) -> Self {
        let message = msg.to_string();
        // Constructing an internal error should get logged because it must be a programming error on our part
        // veilid_log!(acc error "Internal error: {}", &message);
        Self::Internal { message }
    }
    /// Construct a [VeilidAPIError::Unimplemented] error with the given message.
    pub fn unimplemented<T: ToString>(msg: T) -> Self {
        let message = msg.to_string();
        // Constructing an unimplemented error should get logged because it must be a programming error on our part
        // veilid_log!(acc error "Unimplemented: {}", &message);
        Self::Unimplemented { message }
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

/// Trait for types that can be directly converted into parameters for VeilidAPIError
pub trait ToVeilidAPIErrorArgument {
    /// Render this value as a string for inclusion in a [VeilidAPIError] message field.
    fn to_veilid_api_error_argument(&self) -> String;
}

impl ToVeilidAPIErrorArgument for String {
    fn to_veilid_api_error_argument(&self) -> String {
        self.clone()
    }
}
impl ToVeilidAPIErrorArgument for str {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for [u8] {
    fn to_veilid_api_error_argument(&self) -> String {
        hex::encode(self)
    }
}
impl ToVeilidAPIErrorArgument for Vec<u8> {
    fn to_veilid_api_error_argument(&self) -> String {
        hex::encode(self)
    }
}

impl ToVeilidAPIErrorArgument for usize {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for u64 {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for u32 {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for u16 {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for u8 {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for isize {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for i64 {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for i32 {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for i16 {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for i8 {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for f64 {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for f32 {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}

impl<T: ToVeilidAPIErrorArgument> ToVeilidAPIErrorArgument for core::ops::Range<T> {
    fn to_veilid_api_error_argument(&self) -> String {
        format!(
            "{}..{}",
            self.start.to_veilid_api_error_argument(),
            self.end.to_veilid_api_error_argument()
        )
    }
}

impl ToVeilidAPIErrorArgument for Timestamp {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for TimestampDuration {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for ValueSeqNum {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for ValueSubkeyRangeSet {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for CryptoKind {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for VeilidCapability {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}

impl ToVeilidAPIErrorArgument for RouteId {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for NodeId {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for PublicKey {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for SecretKey {
    fn to_veilid_api_error_argument(&self) -> String {
        format!(
            "{}:{}",
            self.kind(),
            self.ref_value().to_veilid_api_error_argument()
        )
    }
}
impl ToVeilidAPIErrorArgument for SharedSecret {
    fn to_veilid_api_error_argument(&self) -> String {
        format!(
            "{}:{}",
            self.kind(),
            self.ref_value().to_veilid_api_error_argument()
        )
    }
}
impl ToVeilidAPIErrorArgument for Signature {
    fn to_veilid_api_error_argument(&self) -> String {
        format!(
            "{}:{}",
            self.kind(),
            self.ref_value().to_veilid_api_error_argument()
        )
    }
}
impl ToVeilidAPIErrorArgument for HashDigest {
    fn to_veilid_api_error_argument(&self) -> String {
        format!(
            "{}:{}",
            self.kind(),
            self.ref_value().to_veilid_api_error_argument()
        )
    }
}
impl ToVeilidAPIErrorArgument for KeyPair {
    fn to_veilid_api_error_argument(&self) -> String {
        format!(
            "{}:{}",
            self.kind(),
            self.ref_value().to_veilid_api_error_argument(),
        )
    }
}
impl ToVeilidAPIErrorArgument for RecordKey {
    fn to_veilid_api_error_argument(&self) -> String {
        format!(
            "{}:{}",
            self.kind(),
            self.value().to_veilid_api_error_argument()
        )
    }
}
impl ToVeilidAPIErrorArgument for MemberId {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for OpaqueRecordKey {
    fn to_veilid_api_error_argument(&self) -> String {
        format!(
            "{}:{}",
            self.kind(),
            self.ref_value().to_veilid_api_error_argument()
        )
    }
}
impl ToVeilidAPIErrorArgument for BareRecordKey {
    fn to_veilid_api_error_argument(&self) -> String {
        format!(
            "{}{}",
            self.ref_key(),
            self.ref_encryption_key()
                .map(|ek| ek.to_veilid_api_error_argument())
                .unwrap_or("".to_string())
        )
    }
}
impl ToVeilidAPIErrorArgument for BareKeyPair {
    fn to_veilid_api_error_argument(&self) -> String {
        format!(
            "{}:{}",
            self.ref_key().to_veilid_api_error_argument(),
            self.ref_secret().to_veilid_api_error_argument()
        )
    }
}
impl ToVeilidAPIErrorArgument for BarePublicKey {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for BareSecretKey {
    fn to_veilid_api_error_argument(&self) -> String {
        "*".repeat(self.to_string().len())
    }
}
impl ToVeilidAPIErrorArgument for BareSharedSecret {
    fn to_veilid_api_error_argument(&self) -> String {
        "*".repeat(self.to_string().len())
    }
}
impl ToVeilidAPIErrorArgument for BareSignature {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for BareHashDigest {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for BareOpaqueRecordKey {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}
impl ToVeilidAPIErrorArgument for BareMemberId {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}

impl ToVeilidAPIErrorArgument for serde_json::Error {
    fn to_veilid_api_error_argument(&self) -> String {
        self.to_string()
    }
}

impl<T: ToVeilidAPIErrorArgument + ?Sized> ToVeilidAPIErrorArgument for &T {
    fn to_veilid_api_error_argument(&self) -> String {
        (**self).to_veilid_api_error_argument()
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

/// Result type for public Veilid API errors
pub type VeilidAPIResult<T> = Result<T, VeilidAPIError>;

/// Extension methods for turning recoverable [VeilidAPIError] outcomes into `Ok(None)`.
pub trait OkVeilidAPIResult<T> {
    /// Map a [VeilidAPIError::TryAgain] error to `Ok(None)`, passing through other results unchanged.
    fn ok_try_again(self) -> VeilidAPIResult<Option<T>>;
    /// Map a [VeilidAPIError::TryAgain] or [VeilidAPIError::Timeout] error to `Ok(None)`, passing through other results unchanged.
    fn ok_try_again_timeout(self) -> VeilidAPIResult<Option<T>>;
}

impl<T> OkVeilidAPIResult<T> for VeilidAPIResult<Option<T>> {
    fn ok_try_again(self) -> VeilidAPIResult<Option<T>> {
        match self {
            Ok(v) => Ok(v),
            Err(VeilidAPIError::TryAgain { message: _ }) => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn ok_try_again_timeout(self) -> VeilidAPIResult<Option<T>> {
        match self {
            Ok(v) => Ok(v),
            Err(VeilidAPIError::TryAgain { message: _ }) => Ok(None),
            Err(VeilidAPIError::Timeout) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl From<std::io::Error> for VeilidAPIError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::TimedOut => VeilidAPIError::timeout(),
            std::io::ErrorKind::ConnectionRefused => VeilidAPIError::no_connection(e.to_string()),
            std::io::ErrorKind::ConnectionReset => VeilidAPIError::no_connection(e.to_string()),
            // #[cfg(feature = "io_error_more")]
            // std::io::ErrorKind::HostUnreachable => VeilidAPIError::no_connection(e.to_string()),
            // #[cfg(feature = "io_error_more")]
            // std::io::ErrorKind::NetworkUnreachable => VeilidAPIError::no_connection(e.to_string()),
            std::io::ErrorKind::ConnectionAborted => VeilidAPIError::no_connection(e.to_string()),
            std::io::ErrorKind::NotConnected => VeilidAPIError::no_connection(e.to_string()),
            std::io::ErrorKind::AddrInUse => VeilidAPIError::no_connection(e.to_string()),
            std::io::ErrorKind::AddrNotAvailable => VeilidAPIError::no_connection(e.to_string()),
            // #[cfg(feature = "io_error_more")]
            // std::io::ErrorKind::NetworkDown => VeilidAPIError::no_connection(e.to_string()),
            // #[cfg(feature = "io_error_more")]
            // std::io::ErrorKind::ReadOnlyFilesystem => VeilidAPIError::internal(e.to_string()),
            // #[cfg(feature = "io_error_more")]
            // std::io::ErrorKind::NotSeekable => VeilidAPIError::internal(e.to_string()),
            // #[cfg(feature = "io_error_more")]
            // std::io::ErrorKind::FilesystemQuotaExceeded => VeilidAPIError::internal(e.to_string()),
            // #[cfg(feature = "io_error_more")]
            // std::io::ErrorKind::Deadlock => VeilidAPIError::internal(e.to_string()),
            std::io::ErrorKind::Unsupported => VeilidAPIError::internal(e.to_string()),
            std::io::ErrorKind::OutOfMemory => VeilidAPIError::internal(e.to_string()),
            _ => VeilidAPIError::generic(e.to_string()),
        }
    }
}
