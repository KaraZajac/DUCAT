pub mod mocks;
pub mod test_config;
pub mod test_serialize_json;
pub mod test_value_data;

pub use mocks::*;

use super::*;

#[expect(clippy::unused_async)]
pub async fn test_all() {
    test_serialize_json::test_serialize_json();
    test_value_data::value_data_ok();
    test_value_data::value_data_too_long();
    test_config::test_internal_footgun_gate();
}
