mod log_output;

use std::io::IsTerminal;

use super::*;

pub use log_output::*;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{layer::SubscriberExt as _, Layer as _};

/// A simple high-level logging mechanism for Veilid applications
///
/// Abstracts the `tracing` interface to simplify getting started with `veilid-core`.
/// More complex log configurations are possible by using the lower-level `VeilidLayerFilter`,
/// `tracing_subscriber`, `FmtStripVeilidFields` and `ApiTracingLayer` directly.
///
/// Example:
/// ```rust,no_run
/// # use veilid_core::*;
/// VeilidTracing::try_init([
///     LogOutput::stderr(true),
///     LogOutput::file("/tmp/debug.txt", false)
///         .with_common_log_level(VeilidConfigLogLevel::Debug)
///     ]).expect("logs failed to initialize");
/// ```
pub struct VeilidTracing {
    filters: BTreeMap<LogOutputKind, VeilidLayerFilter>,
}

impl VeilidTracing {
    /// Creates a basic stderr logger with terminal colors and nothing else
    #[allow(clippy::must_use_candidate)]
    pub fn stderr() -> Self {
        Self::try_init([LogOutput::stderr(true)]).expect("initializing stderr logging failed")
    }

    /// Creates a basic stdout logger with terminal colors and nothing else
    #[allow(clippy::must_use_candidate)]
    pub fn stdout() -> Self {
        Self::try_init([LogOutput::stdout(true)]).expect("initializing stdout logging failed")
    }

    /// Creates an empty log configuration with no outputs
    pub fn try_init<I>(log_outputs: I) -> VeilidAPIResult<Self>
    where
        I: IntoIterator<Item = LogOutput>,
    {
        let subscriber = tracing_subscriber::Registry::default();
        let mut layers = Vec::new();
        let mut filters = BTreeMap::new();

        for log_output in log_outputs.into_iter() {
            if filters.contains_key(&log_output.kind) {
                apibail_generic!(
                    "Can not specify multiple log outputs of kind: {:?}",
                    log_output.kind
                );
            }

            let filter = VeilidLayerFilter::new_with_config(
                VeilidLayerFilterConfig::new().with_directives(log_output.directives),
            );
            filters.insert(log_output.kind.clone(), filter.clone());

            match log_output.kind {
                LogOutputKind::StdOut => {
                    let layer = tracing_subscriber::fmt::Layer::new()
                        .compact()
                        .map_fmt_fields(FmtStripVeilidFields::mapper())
                        .with_ansi(log_output.color && std::io::stdout().is_terminal())
                        .with_writer(std::io::stdout)
                        .with_filter(filter);

                    layers.push(layer.boxed());
                }
                LogOutputKind::StdErr => {
                    let layer = tracing_subscriber::fmt::Layer::new()
                        .compact()
                        .map_fmt_fields(FmtStripVeilidFields::mapper())
                        .with_ansi(log_output.color && std::io::stderr().is_terminal())
                        .with_writer(std::io::stderr)
                        .with_filter(filter);

                    layers.push(layer.boxed());
                }
                LogOutputKind::File => {
                    let file = std::fs::OpenOptions::new()
                        .append(log_output.append)
                        .create(true)
                        .open(log_output.path)
                        .map_err(|e| VeilidAPIError::generic(e.to_string()))?;

                    let layer = tracing_subscriber::fmt::Layer::new()
                        .compact()
                        .map_fmt_fields(FmtStripVeilidFields::mapper())
                        .with_ansi(false)
                        .with_writer(file)
                        .with_filter(filter);

                    layers.push(layer.boxed());
                }
                LogOutputKind::Api => {
                    let layer = ApiTracingLayer::init().with_filter(filter);
                    layers.push(layer.boxed());
                }
                LogOutputKind::Layer(_) => {
                    let layer = log_output.layer.unwrap().with_filter(filter);
                    layers.push(layer.boxed());
                }
            }
        }

        let subscriber = subscriber.with(layers);
        subscriber
            .try_init()
            .map_err(|e| VeilidAPIError::generic(e.to_string()))?;

        Ok(Self { filters })
    }

    /// Get the filter for a specific log output
    #[must_use]
    pub fn get_filter_for_output(
        &self,
        log_output_kind: LogOutputKind,
    ) -> Option<&VeilidLayerFilter> {
        self.filters.get(&log_output_kind)
    }

    /// Get the mutable filter for a specific log output
    #[must_use]
    pub fn get_filter_for_output_mut(
        &mut self,
        log_output_kind: LogOutputKind,
    ) -> Option<&mut VeilidLayerFilter> {
        self.filters.get_mut(&log_output_kind)
    }

    /// Get the filters for all log outputs
    pub fn get_filters(&self) -> impl Iterator<Item = (&LogOutputKind, &VeilidLayerFilter)> {
        self.filters.iter()
    }

    /// Get the mutable filters for all log outputs
    pub fn get_filters_mut(
        &mut self,
    ) -> impl Iterator<Item = (&LogOutputKind, &mut VeilidLayerFilter)> {
        self.filters.iter_mut()
    }

    /// Change the enabled log facilities for a specific log output
    pub fn try_apply_directives_for_output<C: TryIntoIterVeilidLogDirective>(
        &self,
        log_output_kind: LogOutputKind,
        directives: C,
    ) -> VeilidAPIResult<()> {
        let directives = directives.try_into_iter()?.collect::<Vec<_>>();

        if let Some(filter) = self.filters.get(&log_output_kind) {
            filter.apply_directives(directives.clone());
        }

        Ok(())
    }

    /// Change the enabled log facilities for all log outputs
    pub fn try_apply_directives<C: TryIntoIterVeilidLogDirective>(
        &self,
        directives: C,
    ) -> VeilidAPIResult<()> {
        let directives = directives.try_into_iter()?.collect::<Vec<_>>();

        for filter in self.filters.values() {
            filter.apply_directives(directives.clone());
        }

        Ok(())
    }

    /// Change the enabled log facilities for a specific log output with a comma-delimited string
    pub fn try_apply_directives_string_for_output<S: AsRef<str>>(
        &self,
        log_output_kind: LogOutputKind,
        directives: S,
    ) -> VeilidAPIResult<()> {
        if let Some(filter) = self.filters.get(&log_output_kind) {
            filter.try_apply_directives_string(directives)?;
        }

        Ok(())
    }

    /// Change the enabled log facilities for all log outputs with a comma-delimited string
    pub fn try_apply_directives_string<S: AsRef<str>>(&self, directives: S) -> VeilidAPIResult<()> {
        for filter in self.filters.values() {
            filter.try_apply_directives_string(&directives)?;
        }

        Ok(())
    }

    /// Change a single log facility for a single log output
    pub fn try_apply_facility_level_for_output<S: AsRef<str>>(
        &self,
        log_output_kind: LogOutputKind,
        facility: S,
        level: VeilidConfigLogLevel,
    ) -> VeilidAPIResult<()> {
        let directive = VeilidLogDirective::try_facility_level(facility, Some(level))?;

        if let Some(filter) = self.filters.get(&log_output_kind) {
            filter.apply_directives([directive]);
        }
        Ok(())
    }

    /// Change a single log facility for all log outputs
    pub fn try_apply_facility_level<S: AsRef<str>>(
        &self,
        facility: S,
        level: VeilidConfigLogLevel,
    ) -> VeilidAPIResult<()> {
        let directive = VeilidLogDirective::try_facility_level(facility, Some(level))?;

        for filter in self.filters.values() {
            filter.apply_directives([directive.clone()]);
        }

        Ok(())
    }

    /// Try to apply directives from an environment variable in `RUST_LOG` / `EnvFilter` format to a single log output
    pub fn try_apply_env_for_output<S: AsRef<std::ffi::OsStr>>(
        &self,
        log_output_kind: LogOutputKind,
        var_name: S,
    ) -> VeilidAPIResult<()> {
        if let Some(filter) = self.filters.get(&log_output_kind) {
            filter.try_apply_env(var_name)?;
        }
        Ok(())
    }

    /// Try to apply directives from an environment variable in `RUST_LOG` / `EnvFilter` format to all log outputs
    pub fn try_apply_env<S: AsRef<std::ffi::OsStr>>(&self, var_name: S) -> VeilidAPIResult<()> {
        for filter in self.filters.values() {
            filter.try_apply_env(&var_name)?;
        }
        Ok(())
    }

    /// Try to apply directives from the `RUST_LOG` environment variable in `EnvFilter` format to a single log output
    pub fn try_apply_default_env_for_output<S: AsRef<std::ffi::OsStr>>(
        &self,
        log_output_kind: LogOutputKind,
    ) -> VeilidAPIResult<()> {
        self.try_apply_env_for_output(log_output_kind, "RUST_LOG")
    }

    /// Try to apply directives from the `RUST_LOG` environment variable in `EnvFilter` format to all log outputs
    pub fn try_apply_default_env(&self) -> VeilidAPIResult<()> {
        self.try_apply_env("RUST_LOG")
    }

    /// Convenience function that applies a log level to the 'veilid::common' Veilid log tags to a single log output
    pub fn apply_common_log_level_for_output(
        &self,
        log_output_kind: LogOutputKind,
        level: VeilidConfigLogLevel,
    ) {
        if let Some(filter) = self.filters.get(&log_output_kind) {
            filter.apply_directives([VeilidLogDirective::try_facility_level(
                "#common",
                Some(level),
            )
            .unwrap()]);
        }
    }

    /// Convenience function that applies a log level to the 'veilid::common' Veilid log tags to all log outputs
    pub fn apply_common_log_level(&self, level: VeilidConfigLogLevel) {
        for filter in self.filters.values() {
            filter.apply_directives([VeilidLogDirective::try_facility_level(
                "#common",
                Some(level),
            )
            .unwrap()]);
        }
    }

    /// Convenience function that applies a single log level to all the currently enabled (not VeilidConfigLogLevel::Off) Veilid log tags to a single log output
    pub fn apply_enabled_log_level_for_output(
        &self,
        log_output_kind: LogOutputKind,
        level: VeilidConfigLogLevel,
    ) {
        if let Some(filter) = self.filters.get(&log_output_kind) {
            filter.apply_directives([VeilidLogDirective::try_facility_level(
                "#enabled",
                Some(level),
            )
            .unwrap()]);
        }
    }

    /// Convenience function that applies a single log level to all the currently enabled (not VeilidConfigLogLevel::Off) Veilid log tags to all log outputs
    pub fn apply_enabled_log_level(&self, level: VeilidConfigLogLevel) {
        for filter in self.filters.values() {
            filter.apply_directives([VeilidLogDirective::try_facility_level(
                "#enabled",
                Some(level),
            )
            .unwrap()]);
        }
    }
}

impl Default for VeilidTracing {
    fn default() -> Self {
        Self::stderr()
    }
}
