#![allow(non_snake_case)]
use super::*;

// XXX: Eventually this needs to become a FOURCC when address types become pluggable
#[allow(clippy::derived_hash_with_manual_eq)]
#[derive(Debug, PartialOrd, Ord, Hash, Serialize, Deserialize, EnumSetType)]
#[enumset(repr = "u8")]
pub(crate) enum AddressType {
    IPV6 = 0,
    IPV4 = 1,
}

impl AddressType {
    pub fn set_from_str(s: &str) -> VeilidAPIResult<AddressTypeSet> {
        let s = s.to_ascii_lowercase();
        if s == "all-address-type" {
            return Ok(AddressTypeSet::all());
        }
        if s == "no-address-type" || s.is_empty() {
            return Ok(AddressTypeSet::empty());
        }
        let mut aset = AddressTypeSet::empty();
        for astr in s.split("|") {
            let a = AddressType::from_str(astr)?;
            aset |= a;
        }
        Ok(aset)
    }
}

impl fmt::Display for AddressType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AddressType::IPV6 => write!(f, "IPV6"),
            AddressType::IPV4 => write!(f, "IPV4"),
        }
    }
}
impl FromStr for AddressType {
    type Err = VeilidAPIError;
    fn from_str(s: &str) -> VeilidAPIResult<AddressType> {
        match s.to_ascii_lowercase().as_str() {
            "v6" | "6" | "ipv6" => Ok(AddressType::IPV6),
            "v4" | "4" | "ipv4" => Ok(AddressType::IPV4),
            _ => Err(VeilidAPIError::parse_error(
                "AddressType::from_str failed",
                s,
            )),
        }
    }
}

pub(crate) type AddressTypeSet = EnumSet<AddressType>;
