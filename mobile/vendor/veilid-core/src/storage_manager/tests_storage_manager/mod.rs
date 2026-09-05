use super::*;

mod test_encrypted_value_data;
mod test_inbound_set_value_signature;

pub async fn test_all() {
    test_encrypted_value_data::test_all().await;
    test_inbound_set_value_signature::test_all().await;
    record_store::tests::test_record_store().await;
}
