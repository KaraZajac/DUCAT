use super::*;

pub const FOURCC_PROTOCOL_TYPE_UDP: u32 = u32::from_be_bytes(*b"pUDP");
pub const FOURCC_PROTOCOL_TYPE_TCP: u32 = u32::from_be_bytes(*b"pTCP");
pub const FOURCC_PROTOCOL_TYPE_WS: u32 = u32::from_be_bytes(*b"p_WS");
#[cfg(feature = "enable-protocol-wss")]
pub const FOURCC_PROTOCOL_TYPE_WSS: u32 = u32::from_be_bytes(*b"pWSS");

pub fn decode_protocol_type_set(
    reader: &::capnp::primitive_list::Reader<'_, u32>,
) -> ProtocolTypeSet {
    let mut out = ProtocolTypeSet::new();

    for pt in reader.iter() {
        match pt {
            FOURCC_PROTOCOL_TYPE_UDP => {
                out.insert(ProtocolType::UDP);
            }
            FOURCC_PROTOCOL_TYPE_TCP => {
                out.insert(ProtocolType::TCP);
            }
            FOURCC_PROTOCOL_TYPE_WS => {
                out.insert(ProtocolType::WS);
            }
            #[cfg(feature = "enable-protocol-wss")]
            FOURCC_PROTOCOL_TYPE_WSS => {
                out.insert(ProtocolType::WSS);
            }
            _ => {
                // skip unknown protocol types
                continue;
            }
        }
    }
    out
}

pub fn encode_protocol_type_set(
    protocol_type_set: &ProtocolTypeSet,
    builder: &mut ::capnp::primitive_list::Builder<'_, u32>,
) {
    for (n, x) in protocol_type_set.iter().enumerate() {
        builder.set(
            n as u32,
            match x {
                ProtocolType::UDP => FOURCC_PROTOCOL_TYPE_UDP,
                ProtocolType::TCP => FOURCC_PROTOCOL_TYPE_TCP,
                ProtocolType::WS => FOURCC_PROTOCOL_TYPE_WS,
                #[cfg(feature = "enable-protocol-wss")]
                ProtocolType::WSS => FOURCC_PROTOCOL_TYPE_WSS,
            },
        );
    }
}
