use super::*;

/// Log level for VeilidCore.
#[apply(api_data_enum!)]
#[api(eq, copy, ord, ts(namespace))]
pub enum VeilidLogLevel {
    /// Errors.
    Error = 1,
    /// Warnings.
    Warn = 2,
    /// Informational messages.
    Info = 3,
    /// Debugging messages.
    Debug = 4,
    /// Tracing messages, the most verbose level.
    Trace = 5,
}

impl From<VeilidConfigLogLevel> for Option<VeilidLogLevel> {
    fn from(value: VeilidConfigLogLevel) -> Self {
        match value {
            VeilidConfigLogLevel::Off => None,
            VeilidConfigLogLevel::Error => Some(VeilidLogLevel::Error),
            VeilidConfigLogLevel::Warn => Some(VeilidLogLevel::Warn),
            VeilidConfigLogLevel::Info => Some(VeilidLogLevel::Info),
            VeilidConfigLogLevel::Debug => Some(VeilidLogLevel::Debug),
            VeilidConfigLogLevel::Trace => Some(VeilidLogLevel::Trace),
        }
    }
}

impl From<tracing::Level> for VeilidLogLevel {
    fn from(value: tracing::Level) -> Self {
        match value {
            tracing::Level::ERROR => VeilidLogLevel::Error,
            tracing::Level::WARN => VeilidLogLevel::Warn,
            tracing::Level::INFO => VeilidLogLevel::Info,
            tracing::Level::DEBUG => VeilidLogLevel::Debug,
            tracing::Level::TRACE => VeilidLogLevel::Trace,
        }
    }
}

impl From<VeilidLogLevel> for tracing::Level {
    fn from(val: VeilidLogLevel) -> Self {
        match val {
            VeilidLogLevel::Error => tracing::Level::ERROR,
            VeilidLogLevel::Warn => tracing::Level::WARN,
            VeilidLogLevel::Info => tracing::Level::INFO,
            VeilidLogLevel::Debug => tracing::Level::DEBUG,
            VeilidLogLevel::Trace => tracing::Level::TRACE,
        }
    }
}

impl From<tracing::log::Level> for VeilidLogLevel {
    fn from(value: log::Level) -> Self {
        match value {
            tracing::log::Level::Error => VeilidLogLevel::Error,
            tracing::log::Level::Warn => VeilidLogLevel::Warn,
            tracing::log::Level::Info => VeilidLogLevel::Info,
            tracing::log::Level::Debug => VeilidLogLevel::Debug,
            tracing::log::Level::Trace => VeilidLogLevel::Trace,
        }
    }
}

impl From<VeilidLogLevel> for tracing::log::Level {
    fn from(val: VeilidLogLevel) -> Self {
        match val {
            VeilidLogLevel::Error => tracing::log::Level::Error,
            VeilidLogLevel::Warn => tracing::log::Level::Warn,
            VeilidLogLevel::Info => tracing::log::Level::Info,
            VeilidLogLevel::Debug => tracing::log::Level::Debug,
            VeilidLogLevel::Trace => tracing::log::Level::Trace,
        }
    }
}

impl TryFrom<&str> for VeilidLogLevel {
    type Error = VeilidAPIError;

    fn try_from(value: &str) -> Result<Self, <Self as TryFrom<&str>>::Error> {
        Self::from_str(value)
    }
}

impl TryFrom<String> for VeilidLogLevel {
    type Error = VeilidAPIError;

    fn try_from(value: String) -> Result<Self, <Self as TryFrom<String>>::Error> {
        Self::from_str(value.as_str())
    }
}

impl TryFrom<&String> for VeilidLogLevel {
    type Error = VeilidAPIError;

    fn try_from(value: &String) -> Result<Self, <Self as TryFrom<&String>>::Error> {
        Self::from_str(value.as_str())
    }
}

impl FromStr for VeilidLogLevel {
    type Err = VeilidAPIError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "error" => Self::Error,
            "warn" => Self::Warn,
            "info" => Self::Info,
            "debug" => Self::Debug,
            "trace" => Self::Trace,
            _ => {
                apibail_invalid_argument!("invalid VeilidLogLevel string", "s", s);
            }
        })
    }
}
impl fmt::Display for VeilidLogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let text = if f.alternate() {
            match self {
                Self::Error => "ERROR",
                Self::Warn => "WARN",
                Self::Info => "INFO",
                Self::Debug => "DEBUG",
                Self::Trace => "TRACE",
            }
        } else {
            match self {
                Self::Error => "Error",
                Self::Warn => "Warn",
                Self::Info => "Info",
                Self::Debug => "Debug",
                Self::Trace => "Trace",
            }
        };
        write!(f, "{}", text)
    }
}
/// A VeilidCore log message with optional backtrace.
#[apply(api_data_struct!)]
#[api(eq, ts)]
pub struct VeilidLog {
    /// Severity of the message.
    pub log_level: VeilidLogLevel,
    /// The log message text.
    pub message: String,
    /// Backtrace, present for errors when available.
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), tsify(optional))]
    pub backtrace: Option<String>,
}

impl fmt::Display for VeilidLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}{}",
            f.to_string(self.log_level),
            self.message,
            if let Some(backtrace) = &self.backtrace {
                format!("\n{}", backtrace)
            } else {
                "".to_string()
            }
        )
    }
}
