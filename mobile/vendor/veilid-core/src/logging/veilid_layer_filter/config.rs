use super::*;

/// How to initialize the list of filtered facilities
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct VeilidLayerFilterConfig {
    /// Log facility directives in Veilid's log facility format
    directives: Vec<VeilidLogDirective>,
}

impl VeilidLayerFilterConfig {
    /// Creates an empty configuration with no directives.
    pub fn new() -> Self {
        Self { directives: vec![] }
    }

    /// Appends directives to this configuration.
    pub fn apply_directives<C: IntoIterator<Item = VeilidLogDirective>>(&mut self, directives: C) {
        let mut directives = directives.into_iter().collect();
        self.directives.append(&mut directives);
    }

    /// Appends directives and returns the configuration for chaining.
    pub fn with_directives<C: IntoIterator<Item = VeilidLogDirective>>(
        mut self,
        directives: C,
    ) -> Self {
        self.apply_directives(directives);
        self
    }

    /// Appends items that can be parsed into directives, returning an error if any fail to parse.
    pub fn try_apply_directives<C: TryIntoIterVeilidLogDirective>(
        &mut self,
        directives: C,
    ) -> VeilidAPIResult<()> {
        self.apply_directives(directives.try_into_iter()?);
        Ok(())
    }

    /// Appends items that can be parsed into directives and returns the configuration for chaining.
    pub fn try_with_directives<C: TryIntoIterVeilidLogDirective>(
        self,
        directives: C,
    ) -> VeilidAPIResult<Self> {
        Ok(self.with_directives(directives.try_into_iter()?))
    }

    /// Iterates over this configuration's directives.
    pub fn directives(&self) -> impl Iterator<Item = &VeilidLogDirective> {
        self.directives.iter()
    }

    /// Try to apply a comma-separated list of directives
    pub fn try_apply_directives_string<S: AsRef<str>>(
        &mut self,
        directives: S,
    ) -> VeilidAPIResult<()> {
        let directives = directives
            .as_ref()
            .split(",")
            .filter_map(|x| {
                let x = x.trim();
                if x.is_empty() {
                    None
                } else {
                    Some(x)
                }
            })
            .collect::<Vec<_>>();
        if directives.is_empty() {
            return Ok(());
        }
        self.try_apply_directives(directives)
    }

    /// Try to pass-through apply a comma-separated list of directives
    pub fn try_with_directives_string<S: AsRef<str>>(
        mut self,
        directives: S,
    ) -> VeilidAPIResult<Self> {
        self.try_apply_directives_string(directives)?;
        Ok(self)
    }

    /// Applies directives from an environment variable
    /// If the `var_name` is `None`, the environment variable read is `RUST_LOG`
    /// If the `var_name` is `Some("other_env_var")`, it will use the variable you specify
    pub fn try_apply_env<S: AsRef<std::ffi::OsStr>>(&mut self, var_name: S) -> VeilidAPIResult<()> {
        let var = std::env::var(var_name).unwrap_or_default();
        self.try_apply_directives_string(var)?;
        Ok(())
    }

    /// Pass-through applies directives from an environment variable
    /// If the `var_name` is `None`, the environment variable read is `RUST_LOG`
    /// If the `var_name` is `Some("other_env_var")`, it will use the variable you specify
    pub fn try_with_env<S: AsRef<std::ffi::OsStr>>(mut self, var_name: S) -> VeilidAPIResult<Self> {
        self.try_apply_env(var_name)?;
        Ok(self)
    }

    /// Apply directives in `log` format from the `RUST_LOG` environment variable
    pub fn try_apply_default_env(&mut self) -> VeilidAPIResult<()> {
        self.try_apply_env("RUST_LOG")
    }

    /// Apply directives in `log` format from the `RUST_LOG` environment variable
    pub fn try_with_default_env(self) -> VeilidAPIResult<Self> {
        self.try_with_env("RUST_LOG")
    }

    /// Convenience function that applies a default log level to the 'veilid::common' Veilid log tags
    pub fn apply_common_log_level(&mut self, level: VeilidConfigLogLevel) {
        self.apply_directives([
            VeilidLogDirective::try_facility_level("#common", Some(level)).unwrap(),
        ]);

        // If verbose tracing is enabled, apply to the #verbose log tag as well
        #[cfg(feature = "verbose-tracing")]
        self.apply_directives([
            VeilidLogDirective::try_facility_level("#verbose", Some(level)).unwrap(),
        ]);
    }

    /// Convenience function that pass-through applies a default log level to the 'veilid::common' Veilid log tags
    pub fn with_common_log_level(mut self, level: VeilidConfigLogLevel) -> Self {
        self.apply_directives([
            VeilidLogDirective::try_facility_level("#common", Some(level)).unwrap(),
        ]);

        // If verbose tracing is enabled, apply to the #verbose log tag as well
        #[cfg(feature = "verbose-tracing")]
        self.apply_directives([
            VeilidLogDirective::try_facility_level("#verbose", Some(level)).unwrap(),
        ]);
        self
    }
}
