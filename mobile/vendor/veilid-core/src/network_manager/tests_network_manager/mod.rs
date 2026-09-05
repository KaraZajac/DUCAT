mod mocks;
mod test_bootstrap;
mod test_connection_table;

use super::*;
pub use mocks::*;

pub async fn test_all() {
    test_bootstrap::test_bootstrap_v0().await;
    test_bootstrap::test_bootstrap_v1().await;
    test_connection_table::test_add_get_remove().await;
    test_connection_table::test_eviction().await;
    test_connection_table::test_dead_connection_skip().await;
}
