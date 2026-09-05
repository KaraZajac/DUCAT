use super::*;
use core::convert::TryInto;

pub fn decode_address(reader: &veilid_capnp::address::Reader) -> Result<Address, RPCError> {
    let at = reader.get_address_type();
    match at {
        FOURCC_ADDRESS_TYPE_IPV6 => {
            let v6 = reader
                .get_detail()
                .get_as::<veilid_capnp::address_i_p_v6::Reader>()?;
            let v6b0 = v6.get_addr0().to_be_bytes();
            let v6b1 = v6.get_addr1().to_be_bytes();
            let v6b2 = v6.get_addr2().to_be_bytes();
            let v6b3 = v6.get_addr3().to_be_bytes();
            Ok(Address::IPV6(Ipv6Addr::from([
                v6b0[0], v6b0[1], v6b0[2], v6b0[3], v6b1[0], v6b1[1], v6b1[2], v6b1[3], v6b2[0],
                v6b2[1], v6b2[2], v6b2[3], v6b3[0], v6b3[1], v6b3[2], v6b3[3],
            ])))
        }
        FOURCC_ADDRESS_TYPE_IPV4 => {
            let v4 = reader
                .get_detail()
                .get_as::<veilid_capnp::address_i_p_v4::Reader>()?;
            let v4b = v4.get_addr().to_be_bytes();
            Ok(Address::IPV4(Ipv4Addr::new(v4b[0], v4b[1], v4b[2], v4b[3])))
        }
        _ => Err(RPCError::ignore("unknown address type")),
    }
}

pub fn encode_address(address: &Address, builder: &mut veilid_capnp::address::Builder) {
    match address {
        Address::IPV6(v6) => {
            builder.set_address_type(FOURCC_ADDRESS_TYPE_IPV6);
            let mut v6b = builder
                .reborrow()
                .init_detail()
                .init_as::<veilid_capnp::address_i_p_v6::Builder>();
            v6b.set_addr0(u32::from_be_bytes(
                v6.octets()[0..4]
                    .try_into()
                    .expect_or_log("slice with incorrect length"),
            ));
            v6b.set_addr1(u32::from_be_bytes(
                v6.octets()[4..8]
                    .try_into()
                    .expect_or_log("slice with incorrect length"),
            ));
            v6b.set_addr2(u32::from_be_bytes(
                v6.octets()[8..12]
                    .try_into()
                    .expect_or_log("slice with incorrect length"),
            ));
            v6b.set_addr3(u32::from_be_bytes(
                v6.octets()[12..16]
                    .try_into()
                    .expect_or_log("slice with incorrect length"),
            ));
        }
        Address::IPV4(v4) => {
            builder.set_address_type(FOURCC_ADDRESS_TYPE_IPV4);
            let mut v4b = builder
                .reborrow()
                .init_detail()
                .init_as::<veilid_capnp::address_i_p_v4::Builder>();
            v4b.set_addr(u32::from_be_bytes(v4.octets()));
        }
    };
}
