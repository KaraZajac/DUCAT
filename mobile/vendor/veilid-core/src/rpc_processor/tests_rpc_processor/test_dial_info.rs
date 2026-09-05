use super::*;
use std::net::Ipv4Addr;

fn round_trip_dial_info(dial_info: &DialInfo) {
    let mut message_builder = capnp::message::Builder::new_default();
    let mut builder = message_builder.init_root::<veilid_capnp::dial_info::Builder>();

    encode_dial_info(dial_info, &mut builder).unwrap();

    let buffer = canonical_message_builder_to_bytes_writer_packed(message_builder, |size| {
        BytesWriter::with_capacity(size)
    })
    .unwrap()
    .into_inner()
    .to_vec();

    let mut cursor = &mut &buffer[..];
    let tmp_reader =
        capnp::serialize_packed::read_message(&mut cursor, capnp::message::ReaderOptions::new())
            .unwrap();
    let reader = tmp_reader
        .get_root::<veilid_capnp::dial_info::Reader>()
        .unwrap();

    let decoded = decode_dial_info(&reader).unwrap();

    assert_eq!(dial_info, &decoded);
}

pub fn test_encode_and_decode_dial_info_udp() {
    let socket_address = SocketAddress::new(Address::IPV4(Ipv4Addr::new(192, 168, 1, 1)), 8080);
    let dial_info = DialInfo::udp(socket_address);

    round_trip_dial_info(&dial_info);
}

pub fn test_encode_and_decode_dial_info_tcp() {
    let socket_address = SocketAddress::new(Address::IPV4(Ipv4Addr::new(10, 0, 0, 1)), 443);
    let dial_info = DialInfo::tcp(socket_address);

    round_trip_dial_info(&dial_info);
}

pub fn test_encode_and_decode_dial_info_ws() {
    let socket_address = SocketAddress::new(Address::IPV4(Ipv4Addr::new(192, 168, 1, 1)), 8080);
    let dial_info =
        DialInfo::try_ws(socket_address, "ws://192.168.1.1:8080/ws".to_string()).unwrap();

    round_trip_dial_info(&dial_info);
}

#[cfg(feature = "enable-protocol-wss")]
pub fn test_encode_and_decode_dial_info_wss() {
    let socket_address = SocketAddress::new(Address::IPV4(Ipv4Addr::new(10, 0, 0, 1)), 443);
    let dial_info = DialInfo::try_wss(socket_address, "wss://10.0.0.1:443/ws".to_string()).unwrap();

    round_trip_dial_info(&dial_info);
}

fn build_dial_info_message_unspecified_ipv4() -> Vec<u8> {
    let mut message_builder = capnp::message::Builder::new_default();
    let mut builder = message_builder.init_root::<veilid_capnp::dial_info::Builder>();

    builder.set_protocol_type(FOURCC_PROTOCOL_TYPE_UDP);
    let mut udp_builder = builder
        .reborrow()
        .init_detail()
        .init_as::<veilid_capnp::dial_info_u_d_p::Builder>();
    let mut sa_builder = udp_builder.reborrow().init_socket_address();
    let mut addr_builder = sa_builder.reborrow().init_address();
    addr_builder.set_address_type(FOURCC_ADDRESS_TYPE_IPV4);
    let mut v4_builder = addr_builder
        .reborrow()
        .init_detail()
        .init_as::<veilid_capnp::address_i_p_v4::Builder>();
    v4_builder.set_addr(0u32); // 0.0.0.0 in big endian
    sa_builder.set_port(8080);

    canonical_message_builder_to_bytes_writer_packed(message_builder, |size| {
        BytesWriter::with_capacity(size)
    })
    .unwrap()
    .into_inner()
    .to_vec()
}

fn build_dial_info_message_unspecified_ipv6() -> Vec<u8> {
    let mut message_builder = capnp::message::Builder::new_default();
    let mut builder = message_builder.init_root::<veilid_capnp::dial_info::Builder>();

    builder.set_protocol_type(FOURCC_PROTOCOL_TYPE_UDP);
    let mut udp_builder = builder
        .reborrow()
        .init_detail()
        .init_as::<veilid_capnp::dial_info_u_d_p::Builder>();
    let mut sa_builder = udp_builder.reborrow().init_socket_address();
    let mut addr_builder = sa_builder.reborrow().init_address();
    addr_builder.set_address_type(FOURCC_ADDRESS_TYPE_IPV6);
    let mut v6_builder = addr_builder
        .reborrow()
        .init_detail()
        .init_as::<veilid_capnp::address_i_p_v6::Builder>();
    v6_builder.set_addr0(0u32);
    v6_builder.set_addr1(0u32);
    v6_builder.set_addr2(0u32);
    v6_builder.set_addr3(0u32);
    sa_builder.set_port(8080);

    canonical_message_builder_to_bytes_writer_packed(message_builder, |size| {
        BytesWriter::with_capacity(size)
    })
    .unwrap()
    .into_inner()
    .to_vec()
}

pub fn test_decode_dial_info_rejects_unspecified_ipv4() {
    let buffer = build_dial_info_message_unspecified_ipv4();

    let mut cursor = &mut &buffer[..];
    let tmp_reader =
        capnp::serialize_packed::read_message(&mut cursor, capnp::message::ReaderOptions::new())
            .unwrap();
    let reader = tmp_reader
        .get_root::<veilid_capnp::dial_info::Reader>()
        .unwrap();

    let result = decode_dial_info(&reader);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("unspecified"));
}

pub fn test_decode_dial_info_rejects_unspecified_ipv6() {
    let buffer = build_dial_info_message_unspecified_ipv6();

    let mut cursor = &mut &buffer[..];
    let tmp_reader =
        capnp::serialize_packed::read_message(&mut cursor, capnp::message::ReaderOptions::new())
            .unwrap();
    let reader = tmp_reader
        .get_root::<veilid_capnp::dial_info::Reader>()
        .unwrap();

    let result = decode_dial_info(&reader);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("unspecified"));
}
