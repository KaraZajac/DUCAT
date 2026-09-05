use super::*;

pub const FOURCC_SEQUENCING_PREFER_UNORDERED: u32 = u32::from_be_bytes(*b"sqNP"); // XXX: At next breaking change to capnp, switch to "sqPU", and add "sqEU"
pub const FOURCC_SEQUENCING_PREFER_ORDERED: u32 = u32::from_be_bytes(*b"sqPO");
pub const FOURCC_SEQUENCING_ENSURE_ORDERED: u32 = u32::from_be_bytes(*b"sqEO");

pub fn decode_sequencing(sequencing: u32) -> Result<Sequencing, RPCError> {
    match sequencing {
        FOURCC_SEQUENCING_PREFER_UNORDERED => Ok(Sequencing::PreferUnordered),
        FOURCC_SEQUENCING_PREFER_ORDERED => Ok(Sequencing::PreferOrdered),
        FOURCC_SEQUENCING_ENSURE_ORDERED => Ok(Sequencing::EnsureOrdered),
        _ => Err(RPCError::ignore("unsupported sequencing")),
    }
}

pub fn encode_sequencing(sequencing: Sequencing) -> u32 {
    match sequencing {
        Sequencing::PreferUnordered => FOURCC_SEQUENCING_PREFER_UNORDERED,
        Sequencing::PreferOrdered => FOURCC_SEQUENCING_PREFER_ORDERED,
        Sequencing::EnsureOrdered => FOURCC_SEQUENCING_ENSURE_ORDERED,
    }
}
