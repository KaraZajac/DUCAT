use super::*;
use crate::crypto::tests_crypto::*;

pub fn value_data_ok() {
    assert!(ValueData::new(vec![0; ValueData::MAX_LEN], fake_public_key()).is_ok());
    assert!(ValueData::new_with_seq(
        ValueSeqNum::ZERO,
        vec![0; ValueData::MAX_LEN],
        fake_public_key()
    )
    .is_ok());
}

pub fn value_data_too_long() {
    assert!(ValueData::new(vec![0; ValueData::MAX_LEN + 1], fake_public_key()).is_err());
    assert!(ValueData::new_with_seq(
        ValueSeqNum::ZERO,
        vec![0; ValueData::MAX_LEN + 1],
        fake_public_key()
    )
    .is_err());
}
