use super::*;
use crate::crypto::tests_crypto::*;
use crate::storage_manager::{EncryptedValueData, SignedValueData};

pub fn test_encode_and_decode_signed_value_data() {
    let keypair = mock_keypair();
    let fake_nonce = fake_nonce();
    let fake_bare_signature = fake_bare_signature();

    let mut message_builder = ::capnp::message::Builder::new_default();
    let mut builder = message_builder.init_root::<veilid_capnp::signed_value_data::Builder>();

    let signed_value_data = SignedValueData::new(
        EncryptedValueData::new(
            10.into(),
            Bytes::from(vec![1, 2, 3, 4, 5, 6]),
            keypair.key(),
            Some(fake_nonce),
        )
        .unwrap(),
        Signature::new(keypair.kind(), fake_bare_signature),
    );
    encode_signed_value_data(&signed_value_data, &mut builder).unwrap();

    let buffer = canonical_message_builder_to_bytes_writer_packed(message_builder, |size| {
        BytesWriter::with_capacity(size)
    })
    .unwrap()
    .into_inner()
    .freeze();

    println!("buffer[{}] = {:02x?}", buffer.len(), &buffer);

    let mut value_data_cursor = &mut &buffer[..];
    let tmp_reader = capnp::serialize_packed::read_message(
        &mut value_data_cursor,
        capnp::message::ReaderOptions::new(),
    )
    .unwrap();
    let reader = tmp_reader
        .get_root::<veilid_capnp::signed_value_data::Reader>()
        .unwrap();

    let decoded = decode_signed_value_data(&reader).unwrap();

    assert_eq!(
        signed_value_data.value_data().seq(),
        decoded.value_data().seq()
    );
    assert_eq!(
        signed_value_data.value_data().data(),
        decoded.value_data().data()
    );
    assert_eq!(
        signed_value_data.value_data().writer(),
        decoded.value_data().writer()
    );
    assert_eq!(signed_value_data.signature(), decoded.signature());
}
