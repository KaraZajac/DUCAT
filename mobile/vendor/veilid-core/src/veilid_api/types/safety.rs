use super::*;

/// Preferred ordering of RPC message delivery over a route or for RPC messages sent to a target
///   * `Sequencing::PreferUnordered`: any `SequenceOrdering` is acceptable, but `SequenceOrdering::Unordered` will be chosen if available
///   * `Sequencing::PreferOrdered`: any `SequenceOrdering` is acceptable, but `SequenceOrdering::Ordered` will be chosen if available
///   * `Sequencing::EnsureOrdered`: only `SequenceOrdering::Ordered` is acceptable, and if not available, the route will not be created or the message will not be sent
///
/// No sequencing preference guarantees delivery under all conditions, as there is no queuing/buffering in the network
#[apply(api_data_enum!)]
#[api(
    eq,
    copy,
    ord,
    hash,
    default,
    get_size,
    ts(from_wasm_abi, into_wasm_abi, namespace)
)]
// Ordering here matters, >= is used to check strength of sequencing requirement
pub enum Sequencing {
    /// Any `SequenceOrdering` is acceptable, but `SequenceOrdering::Unordered` will be chosen if available
    PreferUnordered = 0,
    /// Any `SequenceOrdering` is acceptable, but `SequenceOrdering::Ordered` will be chosen if available
    #[default]
    PreferOrdered = 1,
    /// Only `SequenceOrdering::Ordered` is acceptable, and if not available, the route will not be created or the message will not be sent
    EnsureOrdered = 2,
}

impl Sequencing {
    /// Returns true if the given ordering satisfies this sequencing requirement.
    #[must_use]
    pub fn matches_ordering(&self, ordering: SequenceOrdering) -> bool {
        match self {
            Sequencing::PreferUnordered => true,
            Sequencing::PreferOrdered => true,
            Sequencing::EnsureOrdered => match ordering {
                SequenceOrdering::Unordered => false,
                SequenceOrdering::Ordered => true,
            },
        }
    }
}

impl fmt::Display for Sequencing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            let s = match self {
                // Sequencing::EnsureUnordered => "ENU",
                Sequencing::PreferUnordered => "PRU",
                Sequencing::PreferOrdered => "PRO",
                Sequencing::EnsureOrdered => "ENO",
            };
            write!(f, "{}", s)
        } else {
            let s = match self {
                // Sequencing::EnsureUnordered => "EnsureUnordered",
                Sequencing::PreferUnordered => "PreferUnordered",
                Sequencing::PreferOrdered => "PreferOrdered",
                Sequencing::EnsureOrdered => "EnsureOrdered",
            };
            write!(f, "{}", s)
        }
    }
}

impl FromStr for Sequencing {
    type Err = VeilidAPIError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_ascii_lowercase();
        // if s == "ensureunordered" || s.is_empty() || s == "enu" {
        //     Ok(Self::PreferUnordered)
        // } else
        if s == "preferunordered" || s.is_empty() || s == "pru" {
            Ok(Self::PreferUnordered)
        } else if s == "preferordered" || s == "pro" {
            Ok(Self::PreferOrdered)
        } else if s == "ensureordered" || s == "eno" {
            Ok(Self::EnsureOrdered)
        } else {
            Err(VeilidAPIError::parse_error("invalid sequencing str", s))
        }
    }
}

/// Ordering of RPC message delivery over a route or for RPC messages sent to a target
///   * `SequencingOrdering::Unordered`: unordered delivery, messages may be received in any order, and may be dropped unreliably
///   * `SequencingOrdering::Ordered`: ordered delivery, messages will be received in the order they were sent, and delivered reliably
///
/// Neither SequenceOrdering guarantees delivery under all conditions, as there is no queuing/buffering in the network
#[allow(clippy::derived_hash_with_manual_eq)]
#[derive(Debug, Default, PartialOrd, Ord, EnumSetType, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(Tsify),
    tsify(from_wasm_abi, into_wasm_abi, namespace)
)]
#[must_use]
#[enumset(repr = "u8")]
// Ordering here matters, >= is used to check strength of sequencing requirement
pub enum SequenceOrdering {
    /// Unordered delivery, messages may be received in any order, and may be dropped unreliably
    Unordered = 0,
    /// Ordered delivery, messages will be received in the order they were sent, and delivered reliably
    #[default]
    Ordered = 1,
}

impl SequenceOrdering {
    /// The sequencing requirement that guarantees this ordering
    pub fn strict_sequencing(&self) -> Sequencing {
        match self {
            SequenceOrdering::Unordered => Sequencing::PreferUnordered,
            SequenceOrdering::Ordered => Sequencing::EnsureOrdered,
        }
    }
    /// The lowest sequencing requirement that matches this ordering
    pub fn minimum_sequencing(&self) -> Sequencing {
        match self {
            SequenceOrdering::Unordered => Sequencing::PreferUnordered,
            SequenceOrdering::Ordered => Sequencing::PreferOrdered,
        }
    }
    /// The highest sequencing requirement that allows this ordering
    pub fn maximum_sequencing(&self) -> Sequencing {
        match self {
            SequenceOrdering::Unordered => Sequencing::PreferOrdered,
            SequenceOrdering::Ordered => Sequencing::EnsureOrdered,
        }
    }
}

impl fmt::Display for SequenceOrdering {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            let s = match self {
                SequenceOrdering::Unordered => "uno",
                SequenceOrdering::Ordered => "ord",
            };
            write!(f, "{}", s)
        } else {
            let s = match self {
                SequenceOrdering::Unordered => "Unordered",
                SequenceOrdering::Ordered => "Ordered",
            };
            write!(f, "{}", s)
        }
    }
}

impl FromStr for SequenceOrdering {
    type Err = VeilidAPIError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_ascii_lowercase();
        if "unordered".starts_with(&s) {
            Ok(Self::Unordered)
        } else if "ordered".starts_with(&s) {
            Ok(Self::Ordered)
        } else {
            Err(VeilidAPIError::parse_error(
                "invalid sequence ordering str",
                s,
            ))
        }
    }
}

/// A set of `SequenceOrdering` values.
pub type SequenceOrderingSet = EnumSet<SequenceOrdering>;

/// Choice of nodes to include in allocated routes
/// * `Stability::LowLatency`: prefer nodes with low latency, but may be unreliable and require route reallocation
/// * `Stability::Reliable`: prefer nodes with reliable uptime, but may have higher latency
#[apply(api_data_enum!)]
#[api(
    eq,
    copy,
    ord,
    hash,
    default,
    get_size,
    ts(from_wasm_abi, into_wasm_abi, namespace)
)]
// Ordering here matters, >= is used to check strength of stability requirement
pub enum Stability {
    /// Prefer nodes with low latency, but may be unreliable and require route reallocation
    LowLatency = 0,
    /// Prefer nodes with reliable uptime, but may have higher latency
    #[default]
    Reliable = 1,
}

impl fmt::Display for Stability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            let s = match self {
                Stability::LowLatency => "low",
                Stability::Reliable => "rel",
            };
            write!(f, "{}", s)
        } else {
            let s = match self {
                Stability::LowLatency => "LowLatency",
                Stability::Reliable => "Reliable",
            };
            write!(f, "{}", s)
        }
    }
}

impl FromStr for Stability {
    type Err = VeilidAPIError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_ascii_lowercase();
        if "lowlatency".starts_with(&s) {
            Ok(Self::LowLatency)
        } else if "reliable".starts_with(&s) {
            Ok(Self::Reliable)
        } else {
            Err(VeilidAPIError::parse_error("invalid stability str", s))
        }
    }
}

/// The choice of safety route to include in compiled routes.
/// * `SafetySelection::Unsafe`: don't use a safety route, only specify the sequencing preference
/// * `SafetySelection::Safe`: use a safety route and parameters specified by a SafetySpec.
#[apply(api_data_enum!)]
#[api(eq, ord, hash, get_size)]
pub enum SafetySelection {
    /// Don't use a safety route, only specify the sequencing preference.
    Unsafe(Sequencing),
    /// Use a safety route and parameters specified by a SafetySpec.
    Safe(SafetySpec),
}

impl SafetySelection {
    /// The sequencing preference for this selection.
    pub fn get_sequencing(&self) -> Sequencing {
        match self {
            SafetySelection::Unsafe(seq) => *seq,
            SafetySelection::Safe(ss) => ss.sequencing,
        }
    }
    /// The safety route hop count, or zero when unsafe.
    pub fn get_hop_count(&self) -> usize {
        match self {
            SafetySelection::Unsafe(_) => 0,
            SafetySelection::Safe(ss) => ss.hop_count,
        }
    }
}

impl fmt::Display for SafetySelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SafetySelection::Unsafe(seq) => write!(f, "Unsafe({})", f.to_string(seq)),
            SafetySelection::Safe(ss) => write!(f, "Safe({})", f.to_string(ss)),
        }
    }
}

/// Options for safety routes (sender privacy).
#[apply(api_data_struct!)]
#[api(eq, ord, hash, default, get_size)]
pub struct SafetySpec {
    /// Preferred safety route set id if it still exists.
    #[cfg_attr(feature = "schemars", schemars(with = "Option<String>"))]
    #[serde(default)]
    pub preferred_route: Option<RouteId>,
    /// If zero this will be set to to the default route hop count
    #[serde(default)]
    pub hop_count: usize,
    /// Prefer reliability over speed.
    #[serde(default)]
    pub stability: Stability,
    /// Prefer connection-oriented sequenced protocols.
    #[serde(default)]
    pub sequencing: Sequencing,
}

impl fmt::Display for SafetySpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            // Compact: only fields differing from default. hop_count != 1 marks non-default
            // because a 1-hop safety route is the lightest meaningful default.
            let mut parts: Vec<String> = Vec::new();
            if self.hop_count != 1 {
                parts.push(format!("#{}", self.hop_count));
            }
            if self.stability != Stability::default() {
                parts.push(f.to_string(self.stability));
            }
            if self.sequencing != Sequencing::default() {
                parts.push(f.to_string(self.sequencing));
            }
            if let Some(r) = &self.preferred_route {
                parts.push(format!("@{}", f.to_string(r)));
            }
            write!(f, "{}", parts.join(","))
        } else {
            write!(
                f,
                "hops={},{},{},{}",
                self.hop_count,
                f.to_string(self.stability),
                f.to_string(self.sequencing),
                self.preferred_route
                    .as_ref()
                    .map(|r| format!("pref={}", f.to_string(r)))
                    .unwrap_or_default()
            )
        }
    }
}

/// Options for private routes (receiver privacy).
#[apply(api_data_struct!)]
#[api(eq, ord, hash, default, get_size)]
pub struct PrivateSpec {
    /// Empty value here will use all the available crypto kinds
    #[serde(default)]
    pub crypto_kinds: Vec<CryptoKind>,
    /// If zero this will be set to to the default route hop count
    #[serde(default)]
    pub hop_count: usize,
    /// Prefer reliability over speed.
    #[serde(default)]
    pub stability: Stability,
    /// Prefer connection-oriented sequenced protocols.
    #[serde(default)]
    pub sequencing: Sequencing,
}

impl fmt::Display for PrivateSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "kinds=[{}],hops={},{},{}",
            self.crypto_kinds
                .iter()
                .map(|k| f.to_string(k))
                .collect::<Vec<_>>()
                .join(","),
            self.hop_count,
            f.to_string(self.stability),
            f.to_string(self.sequencing)
        )
    }
}
