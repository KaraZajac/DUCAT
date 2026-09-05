use super::*;

#[derive(Debug, Clone)]
pub(super) struct PerLogKeyComponentLogFacilities {
    tags_per_facility_name: BTreeMap<String, BTreeSet<String>>,
}
impl Default for PerLogKeyComponentLogFacilities {
    fn default() -> Self {
        let mut this = Self {
            tags_per_facility_name: BTreeMap::new(),
        };

        // Add default core log facilities to all log keys
        this.add_tags("attach", ["#common", "#veilid"]);
        this.add_tags("corectx", ["#common", "#veilid"]);
        this.add_tags("compression", ["#common", "#veilid"]);
        this.add_tags("registry", ["#common", "#veilid"]);
        this.add_tags("veilid_core", ["#common", "#veilid"]);
        this.add_tags("veilid_tools", ["#common", "#veilid"]);
        this.add_tags("veilid_core::*", ["#common", "#veilid"]);
        this.add_tags("veilid_tools::*", ["#common", "#veilid"]);

        this
    }
}
impl PerLogKeyComponentLogFacilities {
    pub fn add_tags<T: AsRef<str>, I: IntoIterator<Item = T>>(&mut self, name: &str, tags: I) {
        for tag in tags.into_iter() {
            self.tags_per_facility_name
                .entry(name.to_string())
                .or_default()
                .insert(tag.as_ref().to_string());
        }
    }
    pub fn get_tags_for_facility<F: AsRef<str>>(&self, name: F) -> BTreeSet<String> {
        let name = name.as_ref();
        let mut out = BTreeSet::new();
        for (facility_name, facility_tags) in self
            .tags_per_facility_name
            .range((
                std::ops::Bound::<String>::Unbounded,
                std::ops::Bound::<String>::Included(name.to_owned()),
            ))
            .rev()
        {
            if let Some(prefix) = facility_name.strip_suffix("*") {
                // Prefix match
                if name.starts_with(prefix) {
                    out.extend(facility_tags.iter().cloned());
                }
            } else if facility_name == name {
                // Whole-name match
                out.extend(facility_tags.iter().cloned());
            } else {
                break;
            }
        }
        out
    }
}

// Because components don't know about the existence of `VeilidLayerFilter`s
// The `VeilidComponentRegistry` has to put their log facility registrations
// somewhere.
#[derive(Debug, Clone)]
pub(super) struct FilterGlobals {
    component_log_facilities_per_log_key: BTreeMap<String, PerLogKeyComponentLogFacilities>,
}

impl Default for FilterGlobals {
    fn default() -> Self {
        let mut this = Self {
            component_log_facilities_per_log_key: BTreeMap::new(),
        };

        // Add default empty log key with default core log facilities
        this.component_log_facilities_per_log_key
            .insert("".to_owned(), PerLogKeyComponentLogFacilities::default());

        this
    }
}

impl FilterGlobals {
    pub fn get_or_create_facilities(
        &mut self,
        log_key: &str,
    ) -> &mut PerLogKeyComponentLogFacilities {
        self.component_log_facilities_per_log_key
            .entry(log_key.to_owned())
            .or_default()
    }

    pub fn get_facilities(&self, log_key: &str) -> Option<&PerLogKeyComponentLogFacilities> {
        self.component_log_facilities_per_log_key.get(log_key)
    }

    pub fn remove_facilities(&mut self, log_key: &str) {
        self.component_log_facilities_per_log_key.remove(log_key);
    }
}

pub(super) static FILTER_GLOBALS: LazyLock<ArcSwap<FilterGlobals>> =
    LazyLock::new(|| ArcSwap::new(Arc::new(FilterGlobals::default())));
