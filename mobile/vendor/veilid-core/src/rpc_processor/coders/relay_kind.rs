use super::*;

pub const FOURCC_RELAY_KIND_INBOUND: u32 = u32::from_be_bytes(*b"rkIB");
pub const FOURCC_RELAY_KIND_OUTBOUND: u32 = u32::from_be_bytes(*b"rkOB");

pub fn decode_relay_kind(relay_kind: u32) -> Result<RelayKind, RPCError> {
    match relay_kind {
        FOURCC_RELAY_KIND_INBOUND => Ok(RelayKind::Inbound),
        FOURCC_RELAY_KIND_OUTBOUND => Ok(RelayKind::Outbound),
        _ => Err(RPCError::ignore("unsupported relay kind")),
    }
}

pub fn encode_relay_kind(relay_kind: RelayKind) -> u32 {
    match relay_kind {
        RelayKind::Inbound => FOURCC_RELAY_KIND_INBOUND,
        RelayKind::Outbound => FOURCC_RELAY_KIND_OUTBOUND,
    }
}
