use super::*;

pub async fn test_record_store() {
    record_store_inner::tests::test_record_store_inner().await;
}
