use super::*;

#[derive(Debug, Default, Clone)]
pub(super) struct FacilityEnableState {
    pub level: VeilidConfigLogLevel,
}

#[derive(Debug, Clone, Default)]
pub(super) struct FacilityEnableMap {
    name_map: BTreeMap<String, FacilityEnableState>,
    tag_map: BTreeMap<String, FacilityEnableState>,
}

impl FacilityEnableMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remove all facilities
    pub fn clear(&mut self) {
        self.name_map.clear();
        self.tag_map.clear();
    }

    /// Removes the exact facility name and its `::`-hierarchy descendants.
    /// Matches the `::` boundary, not a raw string prefix, so unrelated
    /// facilities are not clobbered (e.g. `net` must not remove `network_result`).
    fn remove_name<S: AsRef<str>>(&mut self, name: S) {
        let name = name.as_ref();
        let child_prefix = format!("{name}::");
        self.name_map
            .retain(|k, _| k.as_str() != name && !k.starts_with(child_prefix.as_str()));
    }

    /// Removes all facility tags matching this tag exactly
    fn remove_tag<S: AsRef<str>>(&mut self, tag: S) {
        let tag = tag.as_ref();
        self.tag_map.remove(tag);
    }

    // Removes facility by name or tag
    pub fn remove_facility<S: AsRef<str>>(&mut self, facility: S) {
        let facility = facility.as_ref();
        if facility.starts_with("#") {
            self.remove_tag(facility);
        } else {
            self.remove_name(facility);
        }
    }

    /// Removes all facility names matching this facility by prefix and inserts one facility by name and its enable state
    fn insert_name<S: AsRef<str>>(&mut self, name: S, state: FacilityEnableState) {
        let name = name.as_ref();
        self.remove_name(name);
        self.name_map.insert(name.to_owned(), state);
    }

    /// Inserts or replaces a facility tag and its enable state
    fn insert_tag<S: AsRef<str>>(&mut self, tag: S, state: FacilityEnableState) {
        let mut tag = tag.as_ref();

        // Special case for `#enabled` and `#unspecified`
        if tag == "#enabled" || tag == "#unspecified" {
            // Apply to all enabled names
            let mut was_enabled = false;
            for (_, current_state) in self.name_map.iter_mut() {
                match current_state.level {
                    VeilidConfigLogLevel::Off => {
                        // For disabled logs, ignore
                    }
                    VeilidConfigLogLevel::Error
                    | VeilidConfigLogLevel::Warn
                    | VeilidConfigLogLevel::Info
                    | VeilidConfigLogLevel::Debug
                    | VeilidConfigLogLevel::Trace => {
                        // For enabled logs, replace state
                        *current_state = state.clone();
                        was_enabled = true;
                    }
                }
            }

            // Apply to all enabled tags
            for (_, current_state) in self.tag_map.iter_mut() {
                match current_state.level {
                    VeilidConfigLogLevel::Off => {
                        // For disabled logs, ignore
                    }
                    VeilidConfigLogLevel::Error
                    | VeilidConfigLogLevel::Warn
                    | VeilidConfigLogLevel::Info
                    | VeilidConfigLogLevel::Debug
                    | VeilidConfigLogLevel::Trace => {
                        // For enabled logs, replace state
                        *current_state = state.clone();
                        was_enabled = true;
                    }
                }
            }

            if tag == "#enabled" {
                return;
            }

            // If unspecified and nothing was enabled, set the common tag level
            if !was_enabled {
                tag = "#common";
            }
        }

        self.tag_map.insert(tag.to_owned(), state);
    }

    // Inserts facility by name or tag
    pub fn insert_facility<S: AsRef<str>>(&mut self, facility: S, state: FacilityEnableState) {
        let facility = facility.as_ref();
        if facility.starts_with("#") {
            self.insert_tag(facility, state);
        } else {
            self.insert_name(facility, state);
        }
    }

    /// Check if a facility name is contained by prefix and if it is enabled
    pub fn get_name<S: AsRef<str>>(&self, name: S) -> Option<FacilityEnableState> {
        let name = name.as_ref();

        self.name_map
            .range::<str, _>((std::ops::Bound::Unbounded, std::ops::Bound::Included(name)))
            .next_back()
            .and_then(|(k, v)| {
                if k.starts_with(name) {
                    Some(v.clone())
                } else {
                    None
                }
            })
    }

    /// Check if a facility tag is contained exactly if it is enabled
    pub fn get_tag<S: AsRef<str>>(&self, tag: S) -> Option<FacilityEnableState> {
        let tag = tag.as_ref();

        self.tag_map.get(tag).cloned()
    }

    // Convert this map into a list of directives
    pub fn to_directives(&self) -> Vec<VeilidLogDirective> {
        let mut out = vec![];
        for (k, v) in self.tag_map.iter() {
            out.push(VeilidLogDirective::try_facility_level(k, Some(v.level)).unwrap());
        }
        for (k, v) in self.name_map.iter() {
            out.push(VeilidLogDirective::try_facility_level(k, Some(v.level)).unwrap());
        }
        out
    }

    // Get max level hint
    pub fn max_level_hint(&self) -> LevelFilter {
        let mut out = VeilidConfigLogLevel::Off;
        for (_, v) in self.tag_map.iter() {
            out.max_assign(v.level);
        }
        for (_, v) in self.name_map.iter() {
            out.max_assign(v.level);
        }
        out.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dbg_state() -> FacilityEnableState {
        FacilityEnableState {
            level: VeilidConfigLogLevel::Debug,
        }
    }

    #[test]
    fn insert_name_does_not_clobber_string_prefix_sibling() {
        // Regression: inserting `net` must not remove `network_result` just
        // because the latter begins with "net" — only `::`-hierarchy descendants
        // are superseded.
        let mut m = FacilityEnableMap::new();
        m.insert_name("network_result", dbg_state());
        m.insert_name("net", dbg_state());
        assert!(
            m.get_name("network_result").is_some(),
            "network_result was clobbered by net"
        );
        assert!(m.get_name("net").is_some());
    }

    #[test]
    fn insert_name_supersedes_hierarchy_descendants() {
        // Inserting a parent facility removes its `::` descendants.
        let mut m = FacilityEnableMap::new();
        m.insert_name("stor::record_index", dbg_state());
        m.insert_name("stor", dbg_state());
        assert_eq!(m.name_map.len(), 1);
        assert!(m.name_map.contains_key("stor"));
    }
}
