use super::*;

cfg_if::cfg_if! {
    if #[cfg(feature = "rt-async-std")] {
        #[allow(dead_code)]
        pub fn block_on<F: Future<Output = T>, T>(f: F) -> T {
            async_std::task::block_on(f)
        }
    } else if #[cfg(feature = "rt-tokio")] {
        static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build Tokio runtime")
        });

        #[allow(dead_code)]
        pub fn block_on<F: Future<Output = T>, T>(f: F) -> T {
            RUNTIME.block_on(f)
        }
    } else {
        compile_error!("needs executor implementation");
    }
}

///////////////////////////////////////////////////////////////////////////

cfg_if! {
    if #[cfg(test)] {
        use std::sync::Once;
        use pastey::paste;

        // Utility function to wait for a debugger to attach
        #[allow(dead_code)]
        pub fn wait_for_debugger() {
            eprintln!("Waiting for debugger: pid={}", std::process::id());
            veilid_tools::wait_until_debugger_attached(None)
                .expect("debugger state not implemented on this platform");
            eprintln!("Debugger attached");
        }

        macro_rules! run_test {
            // Nearly all test runner code is cookie cutter, and copy-pasting makes it too easy to make a typo.

            // Pass in a module and test module, and we'll run its `test_all`.
            (module $parent_module:ident $test_module:ident) => {
                paste! {
                    #[test]
                    fn [<run_ $parent_module _ $test_module>]() {
                        native_setup();
                        block_on(async {
                            $parent_module::tests::$test_module::test_all().await;
                        })
                    }
                }
            };

            // Pass in a component name, and we'll run its `tests::test_all`.
            (component $component:ident) => {
                paste! {
                    #[test]
                    fn [<run_tests_ $component>]() {
                        native_setup();
                        block_on(async {
                            $component::[<tests_ $component>]::test_all().await;
                        })
                    }
                }
            };

            // Pass in a 'common' test module name, and we'll run its `test_all`.
            (common $test_module:ident) => {
                paste! {
                    #[test]
                    fn [<run_ $test_module>]() {
                        native_setup();
                        block_on(async {
                            tests::$test_module::test_all().await;
                        })
                    }
                }
            };

        }

        static SETUP_ONCE: Once = Once::new();

        pub fn native_setup() {
            SETUP_ONCE.call_once(|| {
                if std::env::var("NEXTEST").is_err() {
                    eprintln!("WARNING: nextest not detected. Use `cargo nextest run` for optimal test running.");
                }
                if std::env::var("RUST_LOG").unwrap_or_default().is_empty() {
                    eprintln!("INFO: To enable test logging, set the RUST_LOG environment variable. Example: RUST_LOG=#common=debug");
                }
                block_on(
                    #[allow(clippy::unused_async)]
                    async {
                        VeilidTracing::stdout().try_apply_default_env().expect("RUST_LOG string is invalid");
                        #[cfg(feature = "debug-locks")]
                        veilid_tools::deadlock_detector::start_deadlock_detector();
                    }
                );
            });
        }


        run_test!(common test_attachment_manager);
        // run_test!(common test_dht_transactions);
        // run_test!(common test_dht);

        run_test!(component crypto);
        run_test!(component table_store);
        run_test!(component veilid_api);
        run_test!(component routing_table);
        run_test!(component network_manager);
        run_test!(component protected_store);
        run_test!(component storage_manager);
        run_test!(component rpc_processor);
        run_test!(component attachment_manager);

    }


}
