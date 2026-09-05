use super::*;

/// The tracing log field used by veilid-core to indicate which instance of the `VeilidAPI` is doing the logging.
///
/// This field is added by the `veilid_log!()` macro (and others) to enable per-instance log filtering.
pub const VEILID_LOG_KEY_FIELD: &str = "__VEILID_LOG_KEY";

/// The type of a filtering closure accepted by `VeilidLayerFilter`.
/// The filter is passed a log key to filter on and returns true if the log key matches the desired log key of
/// the application. This is an advanced filter for when a basic string comparison against a VeilidLogKey is
/// insufficient, for example defining a logging layer that aggregates the logs of several `VeilidAPI` instances
pub type VeilidLayerLogKeyFilter = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// A log filtering key that is a combination of the 'program name' and 'namespace' of the `VeilidAPI` instance
/// This has a static lifetime because it is used in the `veilid_log!()` macros
pub type VeilidLogKey = &'static str;

/// Selects which log keys a [VeilidLayerFilter] accepts.
#[derive(Clone, Default)]
#[must_use]
pub enum VeilidLogKeyFilterMode {
    /// Filter that includes all log keys
    #[default]
    All,
    /// Filter that includes only a single log key
    LogKey(String),
    /// Filter that uses a callback to decide if the log event should be included
    Filter(VeilidLayerLogKeyFilter),
}

impl fmt::Debug for VeilidLogKeyFilterMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(f, "All"),
            Self::LogKey(arg0) => f.debug_tuple("LogKey").field(arg0).finish(),
            Self::Filter(_) => f.debug_tuple("Filter").finish(),
        }
    }
}

pub(super) struct LogKeyFilterVisitor {
    log_key: Option<String>,
}
impl LogKeyFilterVisitor {
    pub fn new() -> Self {
        LogKeyFilterVisitor { log_key: None }
    }
    pub fn into_log_key(self) -> Option<String> {
        self.log_key
    }
}

impl tracing::field::Visit for LogKeyFilterVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == VEILID_LOG_KEY_FIELD {
            self.log_key = Some(value.to_string());
        }
    }
    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn fmt::Debug) {}
}
