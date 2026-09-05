//! Test suite for the Web and headless browsers.
#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#![recursion_limit = "256"]

use tracing::*;
use veilid_core::tests::*;
use veilid_tools::parking_lot::Once;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

static SETUP_ONCE: Once = Once::new();

pub fn setup() {
    SETUP_ONCE.call_once(|| {
        console_error_panic_hook::set_once();

        let config = veilid_core::tools::wasm_tracing::WasmLayerConfig::new()
            .remove_timings()
            .with_max_level(Level::INFO)
            .remove_color();
        let _ = veilid_core::tools::wasm_tracing::set_as_global_default_with_config(config);
    });
}

#[wasm_bindgen_test]
async fn wasm_run_all_tests() {
    setup();
    run_all_tests().await;
}
