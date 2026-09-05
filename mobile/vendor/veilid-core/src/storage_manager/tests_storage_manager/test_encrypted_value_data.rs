use super::*;
use crate::crypto::tests_crypto::*;

pub fn test_value_data_ok() {
    assert!(EncryptedValueData::new(
        ValueSeqNum::ZERO,
        Bytes::from(vec![0; EncryptedValueData::MAX_LEN]),
        fake_public_key(),
        None,
    )
    .is_ok());
}

pub fn test_value_data_too_long() {
    assert!(EncryptedValueData::new(
        ValueSeqNum::ZERO,
        Bytes::from(vec![0; EncryptedValueData::MAX_LEN + 1]),
        fake_public_key(),
        None,
    )
    .is_err());
}

pub fn test_serialize_deserialize() {
    let orig = EncryptedValueData::new(
        42.into(),
        Bytes::copy_from_slice(b"Brent Spiner"),
        fake_public_key(),
        None,
    );
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

#[expect(clippy::unused_async)]
pub async fn test_all() {
    test_value_data_ok();
    test_value_data_too_long();
    test_serialize_deserialize();
}
