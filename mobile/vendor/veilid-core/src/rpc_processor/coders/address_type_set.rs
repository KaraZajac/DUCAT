use super::*;

pub const FOURCC_ADDRESS_TYPE_IPV6: u32 = u32::from_be_bytes(*b"ipV6");
pub const FOURCC_ADDRESS_TYPE_IPV4: u32 = u32::from_be_bytes(*b"ipV4");

pub fn decode_address_type_set(
    reader: &::capnp::primitive_list::Reader<'_, u32>,
) -> AddressTypeSet {
    let mut out = AddressTypeSet::new();

    for at in reader.iter() {
        match at {
            FOURCC_ADDRESS_TYPE_IPV6 => {
                out.insert(AddressType::IPV6);
            }
            FOURCC_ADDRESS_TYPE_IPV4 => {
                out.insert(AddressType::IPV4);
            }
            _ => {
                // skip unknown address types
                continue;
            }
        }
    }
    out
}

pub fn encode_address_type_set(
    address_type_set: &AddressTypeSet,
    builder: &mut ::capnp::primitive_list::Builder<'_, u32>,
) {
    for (n, x) in address_type_set.iter().enumerate() {
        builder.set(
            n as u32,
            match x {
                AddressType::IPV6 => FOURCC_ADDRESS_TYPE_IPV6,
                AddressType::IPV4 => FOURCC_ADDRESS_TYPE_IPV4,
            },
        );
    }
}
