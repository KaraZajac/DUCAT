#![allow(non_snake_case)]

use super::*;

#[allow(clippy::derived_hash_with_manual_eq)]
#[derive(Debug, PartialOrd, Ord, Hash, EnumSetType, Serialize, Deserialize)]
#[enumset(repr = "u8")]
pub enum Direction {
    In = 0,
    Out = 1,
}

impl Direction {
    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub fn set_from_str(s: &str) -> VeilidAPIResult<DirectionSet> {
        let s = s.to_ascii_lowercase();
        if s == "all-direection" {
            return Ok(DirectionSet::all());
        }
        if s == "no-direction" || s.is_empty() {
            return Ok(DirectionSet::empty());
        }
        let mut dset = DirectionSet::empty();
        for dstr in s.split("|") {
            let d = Direction::from_str(dstr)?;
            dset |= d;
        }
        Ok(dset)
    }

    pub fn set_pretty_string(dset: &DirectionSet) -> String {
        if dset.is_empty() {
            "NONE".to_string()
        } else {
            dset.iter()
                .map(|d| d.to_string().to_uppercase())
                .collect::<Vec<_>>()
                .join("|")
        }
    }
}

pub type DirectionSet = EnumSet<Direction>;

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Direction::In => "in",
            Direction::Out => "out",
        };

        write!(f, "{}", s)
    }
}

impl FromStr for Direction {
    type Err = VeilidAPIError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_ascii_lowercase();
        if "in".starts_with(&s) {
            Ok(Self::In)
        } else if "out".starts_with(&s) {
            Ok(Self::Out)
        } else {
            Err(VeilidAPIError::parse_error("Direction::from_str failed", s))
        }
    }
}
