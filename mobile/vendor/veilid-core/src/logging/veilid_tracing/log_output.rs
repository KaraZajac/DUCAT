use tracing_subscriber::Registry;

use super::*;

use std::path::{Path, PathBuf};

/// Where a [LogOutput] sends its formatted log events.
#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LogOutputKind {
    /// Standard output of the process.
    StdOut,
    /// Standard error of the process.
    StdErr,
    /// A file on disk.
    File,
    /// `VeilidUpdate::Log` events delivered through the API.
    Api,
    /// A caller-supplied `tracing` layer, identified by name.
    Layer(String),
}

/// A log output to be included in the `log_outputs` parameter of `VeilidLog::try_init`
///
/// Example: (debug logs to the terminal, and informational logs to the api tracing layer)
/// ```rust,no_run
/// # use veilid_core::*;
/// let log_outputs = [
///     LogOutput::stdout(true).with_common_log_level(VeilidConfigLogLevel::Debug),
///     LogOutput::api().with_common_log_level(VeilidConfigLogLevel::Info)
/// ];
/// ```
/// Example: (no logs to the terminal)
/// ```
/// # use veilid_core::*;
/// let log_outputs = [LogOutput::stdout(true)];
///
/// let logs = VeilidTracing::try_init(log_outputs).expect("logs failed to initialize");
/// // ...
/// logs.try_apply_facility_level("#common", VeilidConfigLogLevel::Debug).expect("should set log level");
/// ```
#[must_use]
pub struct LogOutput {
    pub(super) kind: LogOutputKind,
    pub(super) color: bool,
    pub(super) path: PathBuf,
    pub(super) append: bool,
    pub(super) directives: Vec<VeilidLogDirective>,
    pub(super) layer: Option<LogOutputLayer>,
}

pub type LogOutputLayer =
    Box<dyn tracing_subscriber::layer::Layer<Registry> + Send + Sync + 'static>;

impl LogOutput {
    /// Creates a log writing to standard output
    pub fn stdout(color: bool) -> Self {
        Self {
            kind: LogOutputKind::StdOut,
            color,
            path: PathBuf::new(),
            append: false,
            directives: vec![],
            layer: None,
        }
    }

    /// Creates a log writing to standard error
    pub fn stderr(color: bool) -> Self {
        Self {
            kind: LogOutputKind::StdErr,
            color,
            path: PathBuf::new(),
            append: false,
            directives: vec![],
            layer: None,
        }
    }

    /// Creates a log writing to a file on disk
    pub fn file<P: AsRef<Path>>(path: P, append: bool) -> Self {
        Self {
            kind: LogOutputKind::File,
            color: false,
            path: path.as_ref().to_owned(),
            append,
            directives: vec![],
            layer: None,
        }
    }

    /// Create a log that sends log output to `VeilidUpdate::Log` events
    pub fn api() -> Self {
        Self {
            kind: LogOutputKind::Api,
            color: false,
            path: PathBuf::new(),
            append: false,
            directives: vec![],
            layer: None,
        }
    }

    /// Creates a log that accepts an arbitrary `tracing` layer
    pub fn layer<L>(name: String, layer: LogOutputLayer) -> Self {
        Self {
            kind: LogOutputKind::Layer(name),
            color: false,
            path: PathBuf::new(),
            append: false,
            directives: vec![],
            layer: Some(layer),
        }
    }

    /// Convenience function that applies a default log level to the 'veilid::common' Veilid log tags
    pub fn with_common_log_level(mut self, level: VeilidConfigLogLevel) -> Self {
        self.directives
            .push(VeilidLogDirective::try_facility_level("veilid::common", Some(level)).unwrap());
        self
    }

    /// Change which log facilities are enabled by default on this log output.
    /// This can also be changed after the VeilidLog is initialized.
    pub fn try_with_directives<C: TryIntoIterVeilidLogDirective>(
        mut self,
        directives: C,
    ) -> VeilidAPIResult<Self> {
        let mut directives = directives.try_into_iter()?.collect();
        self.directives.append(&mut directives);
        Ok(self)
    }
}
