mod component_log_facility;
mod config;
mod facility_enable_map;
mod filter_globals;
mod log_directive;
mod log_key_filter;

use super::*;

use tracing::level_filters::LevelFilter;
use tracing::subscriber::Interest;
use tracing_subscriber::layer;

pub use config::*;
pub use log_directive::*;
pub use log_key_filter::*;

pub(crate) use component_log_facility::*;

use facility_enable_map::*;
use filter_globals::*;

#[derive(Clone)]
struct VeilidLayerFilterInner {
    /// The log key filter this filter applies
    log_key_filter: Option<VeilidLayerLogKeyFilter>,

    /// The current config for this layer filter
    current_config: VeilidLayerFilterConfig,

    /// The default config for this layer filter
    default_config: VeilidLayerFilterConfig,

    /// Built by compile_config for access speed in the event filter
    max_level_hint: LevelFilter,
    facility_enable_map: FacilityEnableMap,
    default_log_level: LevelFilter,
}

/// A `tracing` layer filter that gates events and spans by Veilid log facility,
/// level, and log key. Its configuration can be changed at runtime.
#[derive(Clone)]
#[must_use]
pub struct VeilidLayerFilter {
    inner: Arc<ArcSwap<VeilidLayerFilterInner>>,
}

impl Default for VeilidLayerFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl VeilidLayerFilter {
    /// Creates a `VeilidLayerFilter` with the default configuration
    pub fn new() -> Self {
        Self::new_with_config(VeilidLayerFilterConfig::default())
    }

    /// Creates a `VeilidLayerFilter` and overrides the default configuration
    pub fn new_with_config(default_config: VeilidLayerFilterConfig) -> VeilidLayerFilter {
        let mut inner = VeilidLayerFilterInner {
            log_key_filter: None,
            current_config: default_config.clone(),
            default_config,
            max_level_hint: LevelFilter::OFF,
            facility_enable_map: FacilityEnableMap::new(),
            default_log_level: LevelFilter::OFF,
        };

        Self::compile_config(&mut inner);

        Self {
            inner: Arc::new(ArcSwap::from_pointee(inner)),
        }
    }

    // Takes the current running state and turns it back into a simplified config
    // This is required because directive changes are simply appended into a list
    // before compilation but they could have multiple changes that are conflicting
    // or redundant
    fn simplify_config(inner: &mut VeilidLayerFilterInner) {
        let mut cfg = VeilidLayerFilterConfig::new();

        // Rewrite all directives from empty state
        cfg.apply_directives(VeilidLogDirective::global_level(Some(
            inner.default_log_level.into(),
        )));

        cfg.apply_directives(inner.facility_enable_map.to_directives());

        inner.current_config = cfg;
    }

    // Reset the layer filter facilites enable map and default log level
    fn compile_config_with_default(
        inner: &mut VeilidLayerFilterInner,
        config: &VeilidLayerFilterConfig,
        opt_default_config: Option<&VeilidLayerFilterConfig>,
    ) {
        inner.facility_enable_map.clear();
        inner.default_log_level = LevelFilter::OFF;
        for facility in config.directives() {
            match facility.kind() {
                VeilidLogDirectiveKind::FacilityLevel {
                    facility,
                    opt_level,
                } => {
                    if let Some(level) = *opt_level {
                        inner
                            .facility_enable_map
                            .insert_facility(facility, FacilityEnableState { level });
                    } else {
                        inner.facility_enable_map.remove_facility(facility);
                    }
                }
                VeilidLogDirectiveKind::GlobalLevel { opt_level } => {
                    if let Some(level) = *opt_level {
                        inner.facility_enable_map.clear();
                        inner.default_log_level = level.into();
                    } else if let Some(default_facilities_config) = opt_default_config {
                        // Recurse to handle defaults reset, will only recurse once due to option parameter
                        Self::compile_config_with_default(inner, default_facilities_config, None);
                    }
                }
            }
        }
    }

    // Resets the layer filter state using the current config
    fn compile_config(inner: &mut VeilidLayerFilterInner) {
        // compile facilities configs, incorporating defaults where appropraite
        let config = inner.current_config.clone();
        let default_config = inner.default_config.clone();
        Self::compile_config_with_default(inner, &config, Some(&default_config));

        // Compute max level hint
        inner.max_level_hint = inner.facility_enable_map.max_level_hint();
        inner.max_level_hint.max_assign(inner.default_log_level);

        // Deduplicate config directives
        Self::simplify_config(inner);
    }

    /// Builds the log key for a `VeilidAPI` instance from its program name and namespace.
    /// An empty namespace yields just the program name; otherwise the two are joined with `|`.
    #[must_use]
    pub fn make_veilid_log_key(program_name: &str, namespace: &str) -> VeilidLogKey {
        if namespace.is_empty() {
            program_name.to_static_str()
        } else {
            format!("{}|{}", program_name, namespace).to_static_str()
        }
    }

    #[must_use]
    fn make_simple_log_key_filter(filter_log_key: String) -> VeilidLayerLogKeyFilter {
        Arc::new(move |log_key| log_key == filter_log_key)
    }

    /// Restricts this layer to the log keys accepted by `filter`.
    pub fn set_log_key_filter(&self, filter: VeilidLayerLogKeyFilter) {
        self.inner.rcu(|inner| {
            let mut inner = Arc::as_ref(inner).clone();
            inner.log_key_filter = Some(filter.clone());
            inner
        });
    }

    /// Restricts this layer to events whose log key matches `log_key` exactly.
    pub fn set_log_key(&self, log_key: String) {
        self.inner.rcu(|inner| {
            let mut inner = Arc::as_ref(inner).clone();
            inner.log_key_filter = Some(Self::make_simple_log_key_filter(log_key.clone()));
            inner
        });
    }

    /// Try to apply a list of things that might be directives
    pub fn try_apply_directives<C: TryIntoIterVeilidLogDirective>(
        &self,
        directives: C,
    ) -> VeilidAPIResult<()> {
        let change_list = directives.try_into_iter()?;
        self.apply_directives(change_list);
        Ok(())
    }

    /// Applies a list of directives
    pub fn apply_directives<C: IntoIterator<Item = VeilidLogDirective>>(&self, directives: C) {
        let directives = directives.into_iter().collect::<Vec<_>>();
        self.inner.rcu(|inner| {
            let mut inner = Arc::as_ref(inner).clone();
            inner.current_config.apply_directives(directives.clone());
            Self::compile_config(&mut inner);
            inner
        });

        callsite::rebuild_interest_cache();
    }

    /// Try to apply a comma-separated list of directives
    pub fn try_apply_directives_string<S: AsRef<str>>(&self, directives: S) -> VeilidAPIResult<()> {
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

    /// Applies directives from an environment variable
    /// If the `var_name` is `None`, the environment variable read is `RUST_LOG`
    /// If the `var_name` is `Some("other_env_var")`, it will use the variable you specify
    pub fn try_apply_env<S: AsRef<std::ffi::OsStr>>(&self, var_name: S) -> VeilidAPIResult<()> {
        let var = std::env::var(var_name).unwrap_or_default();
        self.try_apply_directives_string(var)?;
        Ok(())
    }

    /// Apply directives in `log` format from the `RUST_LOG` environment variable
    pub fn try_apply_default_env(&self) -> VeilidAPIResult<()> {
        self.try_apply_env("RUST_LOG")
    }

    /// Convenience function that applies a log level to the 'veilid::common' Veilid log tags
    pub fn apply_common_log_level(&self, level: VeilidConfigLogLevel) {
        self.apply_directives([
            VeilidLogDirective::try_facility_level("#common", Some(level)).unwrap(),
        ])
    }

    /// Convenience function that applies a single log level to all the currently enabled (not VeilidConfigLogLevel::Off) Veilid log tags
    pub fn apply_enabled_log_level(&self, level: VeilidConfigLogLevel) {
        self.apply_directives([
            VeilidLogDirective::try_facility_level("#enabled", Some(level)).unwrap(),
        ])
    }

    /// Applies a comma-separated list of legacy `ignore` facility changes.
    #[deprecated(
        since = "0.5.2",
        note = "'ignore' syntax was confusing, migrate to apply_log_change_string"
    )]
    pub fn apply_ignore_change_string(&self, target_change: String) {
        let target_change = target_change
            .split(',')
            .map(|c| c.trim().to_owned())
            .collect::<Vec<String>>();
        #[allow(deprecated)]
        self.apply_ignore_change_list(&target_change);
    }

    /// Applies a list of legacy `ignore` facility changes. Recognizes `all`, `none`,
    /// `default`, a `-`-prefixed facility (re-enable at the `#common` level), and a bare
    /// facility name (turn off).
    #[deprecated(
        since = "0.5.2",
        note = "'ignore' syntax was confusing, migrate to apply_log_change_list"
    )]
    pub fn apply_ignore_change_list(&self, target_change: &[String]) {
        self.inner.rcu(|inner| {
            let mut inner = Arc::as_ref(inner).clone();
            let config = &mut inner.current_config;
            let opt_common_level = inner
                .facility_enable_map
                .get_tag("#common")
                .map(|state| state.level);

            for change in target_change {
                if change.is_empty() {
                    continue;
                }
                if change == "all" {
                    *config = VeilidLayerFilterConfig::new().with_directives([
                        VeilidLogDirective::global_level(Some(VeilidConfigLogLevel::Off)),
                    ]);
                } else if change == "none" {
                    *config = VeilidLayerFilterConfig::new().with_directives([
                        VeilidLogDirective::global_level(Some(inner.default_log_level.into())),
                    ]);
                } else if change == "default" {
                    *config = VeilidLayerFilterConfig::new()
                        .with_directives([VeilidLogDirective::global_level(None)]);
                } else if let Some(target) = change.strip_prefix('-') {
                    if VeilidLogDirective::check_valid_log_facility(target).is_ok() {
                        if let Some(common_level) = opt_common_level {
                            config.apply_directives(
                                VeilidLogDirective::try_facility_level(target, Some(common_level))
                                    .unwrap(),
                            );
                        } else {
                            config.apply_directives(
                                VeilidLogDirective::try_facility_level(target, None).unwrap(),
                            );
                        }
                    }
                } else if VeilidLogDirective::check_valid_log_facility(change).is_ok() {
                    config.apply_directives(
                        VeilidLogDirective::try_facility_level(
                            change,
                            Some(VeilidConfigLogLevel::Off),
                        )
                        .unwrap(),
                    );
                }
            }

            Self::compile_config(&mut inner);

            inner
        });

        callsite::rebuild_interest_cache();
    }

    /// Add to the list of log facilities, for a list of log keys
    /// Can be used to group logs from both veilid and external crates for debugging purposes
    /// If no log keys are provided, the facilities are added to the default empty log key `""`
    /// To specify the default log key, use an empty string as the log key
    /// All provided facility names are added to all provided tags, for all provided log keys
    ///
    /// Example:
    /// ```rust,no_run
    /// # use veilid_core::*;
    /// VeilidLayerFilter::add_facilities([], ["tstore", "keyvaluedb"], ["#database"]).expect("should add facilities");
    /// ```
    pub fn add_facilities<'a, L, N, T>(log_keys: L, names: N, tags: T) -> VeilidAPIResult<()>
    where
        L: IntoIterator<Item = &'a str>,
        N: IntoIterator<Item = &'a str>,
        T: IntoIterator<Item = &'a str>,
    {
        let log_keys = log_keys
            .into_iter()
            .map(|x| x.to_owned())
            .collect::<Vec<_>>();
        let names = names.into_iter().map(|x| x.to_owned()).collect::<Vec<_>>();
        let tags = tags.into_iter().map(|x| x.to_owned()).collect::<Vec<_>>();

        for name in names.iter() {
            VeilidLogDirective::check_valid_log_facility_name(name.as_str())?;
        }
        for tag in tags.iter() {
            VeilidLogDirective::check_valid_log_facility_tag(tag.as_str())?;
        }

        let log_keys = if log_keys.is_empty() {
            vec!["".to_owned()]
        } else {
            log_keys
        };

        FILTER_GLOBALS.rcu(|inner| {
            let mut filter_globals = Arc::as_ref(inner).clone();

            for log_key in log_keys.iter() {
                let component_log_key_facilities =
                    filter_globals.get_or_create_facilities(log_key.as_str());
                for name in names.iter() {
                    component_log_key_facilities.add_tags(name.as_str(), tags.clone());
                }
            }
            filter_globals
        });

        Ok(())
    }

    pub(crate) fn init_veilid_component_log_facilities(
        log_key: VeilidLogKey,
        all_facilities: Vec<VeilidComponentLogFacilities>,
    ) -> VeilidAPIResult<()> {
        FILTER_GLOBALS.rcu(|inner| {
            let mut filter_globals = Arc::as_ref(inner).clone();

            let component_log_key_facilities = filter_globals.get_or_create_facilities(log_key);

            // Add component-registered log facilities to all log keys
            for facilities in all_facilities.iter() {
                for facility in facilities.iter() {
                    // Add all log facilities and their tags, including the default `#veilid` tag common to all component-registered facilities
                    component_log_key_facilities.add_tags(
                        facility.name(),
                        facility.tags().chain(std::iter::once("#veilid")),
                    );
                }
            }

            filter_globals
        });
        Ok(())
    }

    pub(crate) fn terminate_veilid_component_log_facilities(
        log_key: VeilidLogKey,
    ) -> VeilidAPIResult<()> {
        FILTER_GLOBALS.rcu(|inner| {
            let mut filter_globals = Arc::as_ref(inner).clone();

            filter_globals.remove_facilities(log_key);

            filter_globals
        });

        Ok(())
    }

    fn interesting(&self, metadata: &tracing::Metadata<'_>) -> Interest {
        let inner = self.inner.load();

        let target = metadata.target();
        let level = *metadata.level();

        if LevelFilter::from(level) > inner.max_level_hint {
            return Interest::never();
        }

        if metadata.is_span() {
            // Spans go through enabled() only; there is no event_enabled()
            // pass to gate by tag/default_log_level later. Make a final
            // decision here, applying tag fallback against the empty
            // log_key (the per-log_key tag check needs a runtime field
            // value we don't have at callsite time).
            if Self::check_enabled(&inner, target, level, None) {
                Interest::always()
            } else {
                Interest::never()
            }
        } else {
            // For events, defer the full tag + default_log_level decision
            // to event_enabled, which has the recorded log_key field.
            if let Some(enable_state) = inner.facility_enable_map.get_name(target) {
                let level_filter: LevelFilter = enable_state.level.into();
                if let Some(tracing_level) = level_filter.into_level() {
                    if level <= tracing_level {
                        return Interest::always();
                    }
                } else {
                    return Interest::never();
                }
            }
            Interest::sometimes()
        }
    }

    /// Decide whether `target` at `level` is enabled, using the same logic
    /// as `event_enabled` (direct facility match → tag match → default level).
    /// `opt_log_key` is the runtime log key for events; pass `None` for spans
    /// or when no log key is available — the tag check then only consults
    /// the global ("") log_key fallback.
    fn check_enabled(
        inner: &VeilidLayerFilterInner,
        target: &str,
        level: tracing::Level,
        opt_log_key: Option<&str>,
    ) -> bool {
        // Direct facility name override
        if let Some(state) = inner.facility_enable_map.get_name(target) {
            let level_filter: LevelFilter = state.level.into();
            match level_filter.into_level() {
                Some(tl) if level <= tl => return true,
                Some(_) => {}         // too low — fall through to tag/default
                None => return false, // explicitly off
            }
        }

        // Tag-via-log_key override
        let filter_globals = FILTER_GLOBALS.load();
        let log_keys: &[&str] = match opt_log_key {
            Some(k) => &[k, ""],
            None => &[""],
        };
        for log_key in log_keys {
            if let Some(component) = filter_globals.get_facilities(log_key) {
                let tags = component.get_tags_for_facility(target);
                for tag in tags {
                    if let Some(state) = inner.facility_enable_map.get_tag(tag.as_str()) {
                        let level_filter: LevelFilter = state.level.into();
                        match level_filter.into_level() {
                            Some(tl) if level <= tl => return true,
                            Some(_) => {}         // too low — keep looking
                            None => return false, // tag explicitly off
                        }
                    }
                }
            }
        }

        // No override — gate by default_log_level
        LevelFilter::from(level) <= inner.default_log_level
    }
}

impl<S: tracing::Subscriber> layer::Filter<S> for VeilidLayerFilter {
    fn enabled(&self, metadata: &tracing::Metadata<'_>, _cx: &layer::Context<'_, S>) -> bool {
        !self.interesting(metadata).is_never()
    }

    fn callsite_enabled(&self, metadata: &'static tracing::Metadata<'static>) -> Interest {
        self.interesting(metadata)
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        let inner = self.inner.load();
        Some(inner.max_level_hint)
    }

    fn event_enabled(&self, event: &Event<'_>, _cx: &layer::Context<'_, S>) -> bool {
        // Get this event's log key
        let mut visitor = LogKeyFilterVisitor::new();
        event.record(&mut visitor);
        let log_key = visitor.into_log_key().unwrap_or_default();

        let inner = self.inner.load();

        // See if this log key is enabled on this layer
        let log_key_enabled = inner
            .log_key_filter
            .as_ref()
            .map(|f| f(&log_key))
            .unwrap_or(true);
        if !log_key_enabled {
            return false;
        }

        let event_target = event.metadata().target();
        let event_level = *event.metadata().level();
        Self::check_enabled(&inner, event_target, event_level, Some(log_key.as_str()))
    }
}
