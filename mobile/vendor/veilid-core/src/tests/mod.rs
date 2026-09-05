pub mod common;
pub mod mocks;

pub use common::*;
pub use mocks::*;

#[cfg(all(target_os = "android", feature = "veilid_core_android_tests"))]
mod android;
#[cfg(all(target_os = "ios", feature = "veilid_core_ios_tests"))]
mod ios;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod native;
#[cfg(all(test, not(all(target_arch = "wasm32", target_os = "unknown"))))]
pub use native::wait_for_debugger;

use super::*;

/// Main tests entry point for ios, android, and wasm targets
#[cfg(any(test, feature = "test-util"))]
pub async fn run_all_tests() {
    info!("TEST: veilid_core");
    test_attachment_manager::test_all().await;

    info!("TEST: attachment_manager");
    attachment_manager::tests_attachment_manager::test_all().await;

    info!("TEST: crypto");
    crypto::tests_crypto::test_all().await;

    info!("TEST: test_table_store");
    table_store::tests_table_store::test_all().await;

    info!("TEST: test_protected_store");
    protected_store::tests_protected_store::test_all().await;

    info!("TEST: veilid_api");
    veilid_api::tests_veilid_api::test_all().await;

    info!("TEST: routing_table");
    routing_table::tests_routing_table::test_all().await;

    info!("TEST: network_manager");
    network_manager::tests_network_manager::test_all().await;

    info!("TEST: storage_manager");
    storage_manager::tests_storage_manager::test_all().await;

    info!("TEST: rpc_processor");
    rpc_processor::tests_rpc_processor::test_all().await;

    info!("Finished unit tests");
}
