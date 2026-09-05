pub mod get_directories;

use super::native::*;
use crate::tests::*;
use crate::*;
use backtrace::Backtrace;
use jni::{objects::JClass, objects::JObject, EnvUnowned};
use std::panic;
use tracing_subscriber::prelude::*;

#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn Java_com_veilid_veilid_1core_1android_1tests_MainActivity_run_1tests(
    env: EnvUnowned,
    _class: JClass,
    ctx: JObject,
) {
    veilid_core_setup_android_tests(env, ctx);
    block_on(async {
        run_all_tests().await;
    })
}

pub fn veilid_core_setup_android_tests(env: EnvUnowned, ctx: JObject) {
    veilid_core_setup_android(env, ctx);

    // Set up panic hook for backtraces
    panic::set_hook(Box::new(|panic_info| {
        let bt = Backtrace::new();
        if let Some(location) = panic_info.location() {
            error!(
                "panic occurred in file '{}' at line {}",
                location.file(),
                location.line(),
            );
        } else {
            error!("panic occurred but can't get location information...");
        }
        if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            error!("panic payload: {:?}", s);
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            error!("panic payload: {:?}", s);
        } else if let Some(a) = panic_info.payload().downcast_ref::<std::fmt::Arguments>() {
            error!("panic payload: {:?}", a);
        } else {
            error!("no panic payload");
        }
        error!("Backtrace:\n{:?}", bt);
    }));

    // Set up subscriber and layers
    let filter = VeilidLayerFilter::default();
    filter.apply_common_log_level(VeilidConfigLogLevel::Info);
    let layer = paranoid_android::layer("com.veilid.veilid_core_android_tests")
        .with_target(true)
        //.map_fmt_fields(FmtStripVeilidFields::mapper())
        .with_filter(filter);

    tracing_subscriber::registry().with(layer).init();
}
