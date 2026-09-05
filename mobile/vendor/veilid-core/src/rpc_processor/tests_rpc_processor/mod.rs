use super::*;
mod test_dial_info;
mod test_signed_value_data;

#[expect(clippy::unused_async)]
pub async fn test_all() {
    test_signed_value_data::test_encode_and_decode_signed_value_data();
    test_dial_info::test_encode_and_decode_dial_info_udp();
    test_dial_info::test_encode_and_decode_dial_info_tcp();
    test_dial_info::test_encode_and_decode_dial_info_ws();
    #[cfg(feature = "enable-protocol-wss")]
    test_dial_info::test_encode_and_decode_dial_info_wss();
    test_dial_info::test_decode_dial_info_rejects_unspecified_ipv4();
    test_dial_info::test_decode_dial_info_rejects_unspecified_ipv6();
}
