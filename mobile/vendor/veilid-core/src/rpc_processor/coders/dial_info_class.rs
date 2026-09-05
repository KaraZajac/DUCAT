use super::*;

pub const FOURCC_DIAL_INFO_CLASS_DIRECT: u32 = u32::from_be_bytes(*b"dicD");
pub const FOURCC_DIAL_INFO_CLASS_MAPPED: u32 = u32::from_be_bytes(*b"dicM");
pub const FOURCC_DIAL_INFO_CLASS_FULL_CONE_NAT: u32 = u32::from_be_bytes(*b"dicF");
pub const FOURCC_DIAL_INFO_CLASS_BLOCKED: u32 = u32::from_be_bytes(*b"dicB");
pub const FOURCC_DIAL_INFO_CLASS_ADDRESS_RESTRICTED_NAT: u32 = u32::from_be_bytes(*b"dicA");
pub const FOURCC_DIAL_INFO_CLASS_PORT_RESTRICTED_NAT: u32 = u32::from_be_bytes(*b"dicP");

pub fn decode_dial_info_class(dial_info_class: u32) -> Result<DialInfoClass, RPCError> {
    match dial_info_class {
        FOURCC_DIAL_INFO_CLASS_DIRECT => Ok(DialInfoClass::Direct),
        FOURCC_DIAL_INFO_CLASS_MAPPED => Ok(DialInfoClass::Mapped),
        FOURCC_DIAL_INFO_CLASS_FULL_CONE_NAT => Ok(DialInfoClass::FullConeNAT),
        FOURCC_DIAL_INFO_CLASS_BLOCKED => Ok(DialInfoClass::Blocked),
        FOURCC_DIAL_INFO_CLASS_ADDRESS_RESTRICTED_NAT => Ok(DialInfoClass::AddressRestrictedNAT),
        FOURCC_DIAL_INFO_CLASS_PORT_RESTRICTED_NAT => Ok(DialInfoClass::PortRestrictedNAT),
        _ => Err(RPCError::ignore("unsupported dial info class")),
    }
}

pub fn encode_dial_info_class(dial_info_class: DialInfoClass) -> u32 {
    match dial_info_class {
        DialInfoClass::Direct => FOURCC_DIAL_INFO_CLASS_DIRECT,
        DialInfoClass::Mapped => FOURCC_DIAL_INFO_CLASS_MAPPED,
        DialInfoClass::FullConeNAT => FOURCC_DIAL_INFO_CLASS_FULL_CONE_NAT,
        DialInfoClass::Blocked => FOURCC_DIAL_INFO_CLASS_BLOCKED,
        DialInfoClass::AddressRestrictedNAT => FOURCC_DIAL_INFO_CLASS_ADDRESS_RESTRICTED_NAT,
        DialInfoClass::PortRestrictedNAT => FOURCC_DIAL_INFO_CLASS_PORT_RESTRICTED_NAT,
    }
}
