mod test_limited_size;
mod test_record_index;
use super::*;

pub async fn test_record_store_inner() {
    test_limited_size::test_limited_size().await;
    test_record_index::test_record_index().await;
}
