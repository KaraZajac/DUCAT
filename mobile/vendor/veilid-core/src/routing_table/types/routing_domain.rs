#![allow(non_snake_case)]

use super::*;

// Routing domain here is listed in order of preference, keep in order
#[allow(clippy::derived_hash_with_manual_eq)]
#[derive(Debug, Ord, PartialOrd, Hash, EnumSetType, Serialize, Deserialize)]
#[enumset(repr = "u8")]
pub enum RoutingDomain {
    LocalNetwork = 0,
    PublicInternet = 1,
}
impl RoutingDomain {
    pub const fn count() -> usize {
        2
    }
    pub const fn all() -> [RoutingDomain; RoutingDomain::count()] {
        // Routing domain here is listed in order of preference, keep in order
        [RoutingDomain::LocalNetwork, RoutingDomain::PublicInternet]
    }

    pub fn set_from_str(s: &str) -> VeilidAPIResult<RoutingDomainSet> {
        let s = s.to_ascii_lowercase();
        if s == "all-routing-domain" {
            return Ok(RoutingDomainSet::all());
        }
        if s == "no-routing-domain" || s.is_empty() {
            return Ok(RoutingDomainSet::empty());
        }
        let mut rset = RoutingDomainSet::empty();
        for rstr in s.split("|") {
            let r = RoutingDomain::from_str(rstr)?;
            rset |= r;
        }
        Ok(rset)
    }
}

impl fmt::Display for RoutingDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                RoutingDomain::LocalNetwork => write!(f, "LOC"),
                RoutingDomain::PublicInternet => write!(f, "PUB"),
            }
        } else {
            match self {
                RoutingDomain::LocalNetwork => write!(f, "LocalNetwork"),
                RoutingDomain::PublicInternet => write!(f, "PublicInternet"),
            }
        }
    }
}
impl FromStr for RoutingDomain {
    type Err = VeilidAPIError;
    fn from_str(s: &str) -> VeilidAPIResult<RoutingDomain> {
        let s = s.to_ascii_lowercase();
        if "localnetwork".starts_with(&s) {
            Ok(Self::LocalNetwork)
        } else if "publicinternet".starts_with(&s) {
            Ok(Self::PublicInternet)
        } else {
            Err(VeilidAPIError::parse_error(
                "RoutingDomain::from_str failed",
                s,
            ))
        }
    }
}

pub type RoutingDomainSet = EnumSet<RoutingDomain>;
