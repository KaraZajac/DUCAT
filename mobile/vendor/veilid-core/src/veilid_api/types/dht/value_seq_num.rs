use super::*;

/// Value sequence number
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(Tsify),
    tsify(from_wasm_abi, into_wasm_abi)
)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(transparent)]
pub struct ValueSeqNum(Option<u32>);

impl ValueSeqNum {
    /// The largest representable sequence number (`u32::MAX` is reserved to mean [ValueSeqNum::NONE])
    pub const MAX: Self = ValueSeqNum(Some(u32::MAX - 1));
    /// No sequence number, for a subkey that has never been written
    pub const NONE: Self = ValueSeqNum(None);
    /// The sequence number of a subkey's first write
    pub const ZERO: Self = ValueSeqNum(Some(0));
}

impl ValueSeqNum {
    /// The next sequence number, starting at [ValueSeqNum::ZERO] from [ValueSeqNum::NONE]. Fails at [ValueSeqNum::MAX].
    pub fn next(&self) -> VeilidAPIResult<Self> {
        if let Some(v) = self.0 {
            if v == u32::MAX - 1 {
                apibail_generic!("max seq reached");
            }
            Ok(ValueSeqNum(Some(v + 1)))
        } else {
            Ok(ValueSeqNum(Some(0)))
        }
    }

    /// True if this is [ValueSeqNum::MAX]
    #[must_use]
    pub const fn is_max(&self) -> bool {
        // Unwrap is the only way rust can do this in a const fn right now
        self.0.is_some() && self.0.unwrap() == u32::MAX - 1
    }

    /// True if this is [ValueSeqNum::NONE]
    #[must_use]
    pub const fn is_none(&self) -> bool {
        self.0.is_none()
    }

    /// True if this holds a sequence number
    #[must_use]
    pub const fn is_some(&self) -> bool {
        self.0.is_some()
    }

    /// The sequence number as an [Option], `None` for [ValueSeqNum::NONE]
    #[must_use]
    pub const fn to_option(&self) -> Option<u32> {
        self.0
    }
}

impl Default for ValueSeqNum {
    fn default() -> Self {
        ValueSeqNum::NONE
    }
}

impl From<u32> for ValueSeqNum {
    fn from(value: u32) -> Self {
        if value == u32::MAX {
            ValueSeqNum(None)
        } else {
            ValueSeqNum(Some(value))
        }
    }
}
impl From<Option<u32>> for ValueSeqNum {
    fn from(value: Option<u32>) -> Self {
        if let Some(v) = value {
            if v == u32::MAX {
                ValueSeqNum(None)
            } else {
                ValueSeqNum(Some(v))
            }
        } else {
            ValueSeqNum(None)
        }
    }
}

impl From<ValueSeqNum> for u32 {
    fn from(value: ValueSeqNum) -> Self {
        value.0.unwrap_or(u32::MAX)
    }
}

impl From<ValueSeqNum> for Option<u32> {
    fn from(value: ValueSeqNum) -> Self {
        value.0
    }
}

impl fmt::Display for ValueSeqNum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(v) = self.0 {
            write!(f, "{}", v)
        } else {
            write!(f, "-")
        }
    }
}
