use super::*;

const RESERVED_LOG_FACILITY_NAMES: &[&[u8]] = &[
    b"all",
    b"external",
    b"enabled",
    b"veilid",
    b"common",
    b"default",
    b"error",
    b"warn",
    b"warning",
    b"info",
    b"debug",
    b"trace",
];

const RESERVED_LOG_FACILITY_TAGS: &[&[u8]] = &[
    b"#all",
    b"#external",
    b"#enabled",
    b"#veilid",
    b"#default",
    b"#error",
    b"#warn",
    b"#warning",
    b"#info",
    b"#debug",
    b"#trace",
    b"#unspecified",
];

/// A single log facility as registered by a `VeilidComponent`
#[derive(Debug, Clone)]
pub(crate) struct VeilidComponentLogFacility {
    /// Name of the facility
    name: String,
    /// Tags used to collect facilities into commonly used groups
    tags: BTreeSet<String>,
}

impl VeilidComponentLogFacility {
    /// Creates a new log facility
    pub fn try_new(name: &str) -> VeilidAPIResult<Self> {
        Self::check_valid_log_facility_name(name)?;
        Ok(Self {
            name: name.to_owned(),
            tags: BTreeSet::new(),
        })
    }
    /// Creates a new log facility
    pub fn try_new_with_tags<'a, I: IntoIterator<Item = &'a str>>(
        name: &str,
        tags: I,
    ) -> VeilidAPIResult<Self> {
        Self::check_valid_log_facility_name(name)?;
        let mut this = Self {
            name: name.to_owned(),
            tags: BTreeSet::new(),
        };
        this.apply_tags(tags)?;
        Ok(this)
    }

    /// Gets the name of this component log facility
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the tags on this component log facility
    pub fn tags(&self) -> impl Iterator<Item = &'_ str> {
        self.tags.iter().map(|x| x.as_str())
    }

    /// Adds tags to a component log facility
    pub fn apply_tags<'a, I: IntoIterator<Item = &'a str>>(
        &mut self,
        tags: I,
    ) -> VeilidAPIResult<()> {
        self.tags = tags
            .into_iter()
            .map(|t| {
                Self::check_valid_log_facility_tag(t)?;
                Ok(t.to_string())
            })
            .collect::<VeilidAPIResult<BTreeSet<_>>>()?;
        Ok(())
    }

    /// Passthrough-adds tags to a component log facility
    #[expect(dead_code)]
    pub fn with_tags<'a, I: IntoIterator<Item = &'a str>>(
        mut self,
        tags: I,
    ) -> VeilidAPIResult<Self> {
        self.apply_tags(tags)?;
        Ok(self)
    }

    fn check_reserved_log_facility_tag(name: &str) -> VeilidAPIResult<()> {
        let namebytes = name.as_bytes();

        for reserved in RESERVED_LOG_FACILITY_TAGS {
            if namebytes == *reserved {
                return Err(VeilidAPIError::parse_error(
                    "log facility tag can not be a reserved tag",
                    name.to_owned(),
                ));
            }
        }

        Ok(())
    }

    fn check_reserved_log_facility_name(name: &str) -> VeilidAPIResult<()> {
        let namebytes = name.as_bytes();

        for reserved in RESERVED_LOG_FACILITY_NAMES {
            if namebytes.starts_with(reserved) {
                return Err(VeilidAPIError::parse_error(
                    "log facility name can not start with a reserved name",
                    name.to_owned(),
                ));
            }
        }

        Ok(())
    }

    fn check_valid_log_facility_name(name: &str) -> VeilidAPIResult<()> {
        // Check facility name rules
        VeilidLogDirective::check_valid_log_facility_name(name)?;

        // Facilities can't be reserved names here
        Self::check_reserved_log_facility_name(name)?;

        Ok(())
    }

    fn check_valid_log_facility_tag(tag: &str) -> VeilidAPIResult<()> {
        // Check facility name rules as they also apply to tags
        VeilidLogDirective::check_valid_log_facility_tag(tag)?;

        // Tags can't be reserved names here
        Self::check_reserved_log_facility_tag(tag)?;

        Ok(())
    }
}

/// A group of log facilities to register with the `VeilidLayerFilter``
/// Used by `VeilidComponent`s to add their log facilities to the layer
#[derive(Debug, Clone, Default)]
#[must_use]
pub(crate) struct VeilidComponentLogFacilities {
    /// The list of all log facilities associated with this component
    all_facilities: BTreeMap<String, VeilidComponentLogFacility>,
}

impl VeilidComponentLogFacilities {
    /// Create a new layer filter facilities list for a component
    pub fn new() -> Self {
        Self {
            all_facilities: BTreeMap::new(),
        }
    }

    /// Adds a `VeilidComponentLogFacility` to the layer filter facilities list.
    /// If the facility's name is used more than once, the most recent
    /// facility definition is used
    pub fn with_facility(mut self, facility: VeilidComponentLogFacility) -> Self {
        self.all_facilities.insert(facility.name.clone(), facility);
        self
    }

    /// Iterates all the layer filter facilities in this list
    pub fn iter(&self) -> impl Iterator<Item = &VeilidComponentLogFacility> {
        self.all_facilities.values()
    }
}

impl IntoIterator for VeilidComponentLogFacilities {
    type Item = VeilidComponentLogFacility;
    type IntoIter = alloc::collections::btree_map::IntoValues<String, VeilidComponentLogFacility>;
    fn into_iter(self) -> Self::IntoIter {
        self.all_facilities.into_values()
    }
}
