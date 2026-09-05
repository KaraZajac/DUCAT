use super::*;

/// Changes to the VeilidLayerFilter enabled facilities
///
/// Veilid log facilities are string names that can refer to two things:
///
/// * The name of a tracing log target
/// * A tag name which represents a group of tracing log targets
///
/// Tags are used to group commonly used log facilities that can all be turned on and off together for ease of debugging.
///
/// Veilid log directives are instructions to set a facility to a visibility level in the log output.
/// The format is an extension of the `RUST_LOG` / `EnvFilter` format.
///
/// * enable: `facility=level` to enable 'facility' with a max level, where level is one of `error`, `warn`, `info`, `debug`, or `trace`
/// * disable: `facility=off` to disable 'facility' in the logs
/// * default: `facility=default` to remove the facility from the filter and use whatever defaults exist
/// * reset: `level` or `off` by itself removes all logs facility customizations and resets the base log level for -all- targets including external crates. If you change this level you will turn on or off every log target in the system.
/// * reset default: `default` by itself sets the logs to the application-specific default log string, which is customizable on the `VeilidTracing` or `VeilidLayerFilter` structs. By default the 'default' is all logs turned off.
///
/// Log facilities can be combined with a comma, like this:
///
/// `RUST_LOG="common=info,rpc_message=debug"`
///
/// Log facility names must follow these validity rules:
///
/// * log facility name can not be empty
/// * first character of log facility name must be ASCII alphanumeric or '-'
/// * characters of log facility name must be ASCII alphanumeric or one of '-_:'
/// * log facility tags follows the same rules as facility names but must start with '#'
///
/// Some of the defined log tags include:
///
/// * `#veilid` - All of the veilid log targets
/// * `#common` - The most commonly useful veilid log targets
/// * `#enabled` - The set of currently enabled `#veilid` log targets (those not `VeilidConfigLogLevel::Off`)

#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct VeilidLogDirective {
    kind: VeilidLogDirectiveKind,
}

impl VeilidLogDirective {
    /// Set a specific facility or tag to log level
    pub fn try_facility_level<S: AsRef<str>>(
        facility: S,
        opt_level: Option<VeilidConfigLogLevel>,
    ) -> VeilidAPIResult<Self> {
        Self::check_valid_log_facility(facility.as_ref())?;
        Ok(Self {
            kind: VeilidLogDirectiveKind::FacilityLevel {
                facility: facility.as_ref().to_string(),
                opt_level,
            },
        })
    }

    /// Clear all log facility mappings and reset the global log facility level to a specific log level or the default levels if `None` is specified.
    pub fn global_level(opt_level: Option<VeilidConfigLogLevel>) -> Self {
        Self {
            kind: VeilidLogDirectiveKind::GlobalLevel { opt_level },
        }
    }

    pub(super) fn check_valid_log_facility_name(name: &str) -> VeilidAPIResult<()> {
        let namebytes = name.as_bytes();
        if namebytes.is_empty() {
            return Err(VeilidAPIError::parse_error(
                "log facility name can not be empty",
                name.to_owned(),
            ));
        }
        let firstbyte = namebytes[0];
        if !firstbyte.is_ascii_alphanumeric() && firstbyte != b'_' {
            return Err(VeilidAPIError::parse_error(
                "first character of log facility name must be ASCII alphanumeric or '_'",
                name.to_owned(),
            ));
        }
        if !namebytes[1..]
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_' || *c == b':')
        {
            return Err(VeilidAPIError::parse_error(
                "characters of log facility name must be ASCII alphanumeric or one of '-_:'",
                name.to_owned(),
            ));
        }
        Ok(())
    }

    pub(super) fn check_valid_log_facility_tag(tag: &str) -> VeilidAPIResult<()> {
        let tagbytes = tag.as_bytes();
        if tagbytes.len() < 2 {
            return Err(VeilidAPIError::parse_error(
                "log facility tag can not be empty",
                tag.to_owned(),
            ));
        }
        if tagbytes[0] != b'#' {
            return Err(VeilidAPIError::parse_error(
                "log facility tags must begin with '#'",
                tag.to_owned(),
            ));
        }

        // The rest of the tag follows the same rules as a facility name
        Self::check_valid_log_facility_name(&tag[1..])?;

        Ok(())
    }

    pub(super) fn check_valid_log_facility(name: &str) -> VeilidAPIResult<()> {
        if name.is_empty() {
            return Err(VeilidAPIError::parse_error(
                "log facility can not be empty",
                name.to_owned(),
            ));
        }
        if &name[0..1] == "#" {
            Self::check_valid_log_facility_tag(name)?;
        } else {
            Self::check_valid_log_facility_name(name)?;
        }

        Ok(())
    }

    pub(super) fn kind(&self) -> &VeilidLogDirectiveKind {
        &self.kind
    }
}

impl fmt::Display for VeilidLogDirective {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match &self.kind {
            VeilidLogDirectiveKind::FacilityLevel {
                facility,
                opt_level,
            } => {
                if let Some(level) = opt_level {
                    format!("{}={}", facility, level)
                } else {
                    format!("{}=default", facility)
                }
            }
            VeilidLogDirectiveKind::GlobalLevel { opt_level } => {
                if let Some(level) = opt_level {
                    format!("{}", level)
                } else {
                    "default".to_string()
                }
            }
        };
        f.write_str(&s)
    }
}

impl TryFrom<&str> for VeilidLogDirective {
    type Error = VeilidAPIError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_str(value)
    }
}
impl TryFrom<String> for VeilidLogDirective {
    type Error = VeilidAPIError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_str(&value)
    }
}
impl TryFrom<&String> for VeilidLogDirective {
    type Error = VeilidAPIError;

    fn try_from(value: &String) -> Result<Self, Self::Error> {
        Self::from_str(value)
    }
}

impl FromStr for VeilidLogDirective {
    type Err = VeilidAPIError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.split("=").collect::<Vec<_>>();
        if parts.len() == 2 {
            let opt_level = if parts[1] == "default" {
                None
            } else {
                Some(VeilidConfigLogLevel::from_str(parts[1])?)
            };
            VeilidLogDirective::try_facility_level(parts[0], opt_level)
        } else if parts.len() == 1 {
            let opt_level = if parts[0] == "default" {
                None
            } else {
                Some(VeilidConfigLogLevel::from_str(parts[0])?)
            };
            Ok(VeilidLogDirective::global_level(opt_level))
        } else {
            Err(VeilidAPIError::parse_error(
                "invalid log directive format",
                s,
            ))
        }
    }
}

impl IntoIterator for VeilidLogDirective {
    type Item = Self;
    type IntoIter = core::iter::Once<Self>;

    fn into_iter(self) -> Self::IntoIter {
        core::iter::once(self)
    }
}

pub trait TryIntoIterVeilidLogDirective {
    fn try_into_iter(self) -> VeilidAPIResult<impl Iterator<Item = VeilidLogDirective>>;
}

impl TryIntoIterVeilidLogDirective for VeilidLogDirective {
    fn try_into_iter(self) -> VeilidAPIResult<impl Iterator<Item = VeilidLogDirective>> {
        Ok(std::iter::once(self))
    }
}

impl<I, C> TryIntoIterVeilidLogDirective for I
where
    C: TryInto<VeilidLogDirective, Error = VeilidAPIError>,
    I: IntoIterator<Item = C>,
{
    fn try_into_iter(self) -> VeilidAPIResult<impl Iterator<Item = VeilidLogDirective>> {
        let mut changes = vec![];
        for c in self.into_iter() {
            changes.push(c.try_into()?);
        }
        Ok(changes.into_iter())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub(super) enum VeilidLogDirectiveKind {
    FacilityLevel {
        facility: String,
        opt_level: Option<VeilidConfigLogLevel>,
    },
    GlobalLevel {
        opt_level: Option<VeilidConfigLogLevel>,
    },
}
