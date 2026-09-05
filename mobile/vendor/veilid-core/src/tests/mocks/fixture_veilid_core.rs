use super::*;

cfg_if! {
    if #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))] {
        use std::fs::File;
        use std::io::prelude::*;
        use std::path::PathBuf;

        static CERTFILE: &str = r#"-----BEGIN CERTIFICATE-----
MIIDbzCCAlegAwIBAgIRALB/PvRpqN55Pk7L33NNsvcwDQYJKoZIhvcNAQELBQAw
FDESMBAGA1UEAwwJTm9jdGVtIENBMB4XDTIwMDkwODIxMDkwMFoXDTMwMDkwNjIx
MDkwMFowHDEaMBgGA1UEAwwRKi5ub2N0ZW0uaW50ZXJuYWwwggEiMA0GCSqGSIb3
DQEBAQUAA4IBDwAwggEKAoIBAQDRbAtA2dIlTPaQUN43/bdGi2wuDzCXk36TcfOr
YoxGsyJV6QpcIdmtrPN2WbkuDmA/G+0BUcQPvBfA/pFRHQElrzMhGR23Mp6IK7YR
pomUa1DQSJyMw/WM9V0+tidp5tJSeUCB+qKhLBrztD5XXjdhU6WA1J0y26XQoBqs
RZbPV8mce4LxVaQptkf4NB4/jnr3M1/FWEri60xBw3blWGaLP6gza3vqAr8pqEY4
zXU4q+egLbRIOwxwBJ0/vcyO6BdSzA1asWJCddXQJkUQrLl3OQ+44FMsAFyzCOiK
DVoqD2z4IJvIRT6TH8OcYvrotytlsNXS4ja9r32tTR1/DxUrAgMBAAGjgbMwgbAw
CQYDVR0TBAIwADAdBgNVHQ4EFgQUhjP4CArB3wWGHfavf7mRxaYshKMwRAYDVR0j
BD0wO4AUKAOv10AaiIUHgOtx0Mk6ZaZ/tGWhGKQWMBQxEjAQBgNVBAMMCU5vY3Rl
bSBDQYIJAISVWafozd3RMBMGA1UdJQQMMAoGCCsGAQUFBwMBMAsGA1UdDwQEAwIF
oDAcBgNVHREEFTATghEqLm5vY3RlbS5pbnRlcm5hbDANBgkqhkiG9w0BAQsFAAOC
AQEAMfVGtpXdkxflSQY2DzIUXLp9cZQnu4A8gww8iaLAg5CIUijP71tb2JJ+SsRx
W3p14YMhOYtswIvGTtXWzMgfAivwrxCcJefnqDAG9yviWoA0CSQe21nRjEqN6nyh
CS2BIkOcNNf10TD9sNo7z6IIXNjok7/F031JvH6pBgZ8Bq4IE/ANIuAvxwslPrqT
80qnWtAc5TzNNR1CT+fyZwMEpeW5fMZQnrSyUMsNv06Jydl/7IkGvlmbwihZOg95
Vty37pyzrXU5s/DY1zi5aYoFiK7/4bNEy9mRL9ero+kCvQfea0Yt2rITKQkCYvKu
MQTNaSyo6GTifW5InckkQIsnTQ==
-----END CERTIFICATE-----"#;

        static KEYFILE: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDRbAtA2dIlTPaQ
UN43/bdGi2wuDzCXk36TcfOrYoxGsyJV6QpcIdmtrPN2WbkuDmA/G+0BUcQPvBfA
/pFRHQElrzMhGR23Mp6IK7YRpomUa1DQSJyMw/WM9V0+tidp5tJSeUCB+qKhLBrz
tD5XXjdhU6WA1J0y26XQoBqsRZbPV8mce4LxVaQptkf4NB4/jnr3M1/FWEri60xB
w3blWGaLP6gza3vqAr8pqEY4zXU4q+egLbRIOwxwBJ0/vcyO6BdSzA1asWJCddXQ
JkUQrLl3OQ+44FMsAFyzCOiKDVoqD2z4IJvIRT6TH8OcYvrotytlsNXS4ja9r32t
TR1/DxUrAgMBAAECggEBAMIAK+CUqCbjyBliwKjvwWN5buqwKZyRBxXB3y/qJ/aq
pWkea/lzZjqMWDFP5sryiFiOHx00yMKmxP6FFMsmalSlm2DS6oM2QkP08kIhm5vB
WmjIizWfpo5BEnMwvQxOxpGeP5LpQtS5jfIrDAFVh0oC+fOBgmqFrXK5jlv+Tzmc
9PzoF5lgy8CHw3NxuScJpEhA1vTzu5N7sTdiTDKqY1ph2+RFlf30oyx4whoRVpIC
w8vp3WbLu/yAGuN5S14mYJW2Qgi8/rVCDStROEKOeB99mt1MG5lX7iuagzS/95Lr
2m1Nya0+7hkkpq6Y3Wqne9H0NLasJK8PU8ZaEc6BwTkCgYEA8iLVBrt4W/Cc5hry
8LWCMX8P25z7WIRYswnPvqwTwE0f6Q1ddWIaR9GPWUHgoRC4Z0b0MKolwo9s8RPE
GBuTOCy8ArSgYb1jNpsanGIWg6mZZgfylKdMdCMXMAAYF1/sTXeqCDY+FSCzEAvZ
hzppcCpiKV7Pa9aOo7o3/IeUBZcCgYEA3WmyvscG27R18XASJYL8Y4DuFvvnTHMp
YnxJIoS1+0TnUD2QqXUnXKbnTioWs7t990YAjbsHvK4fVsbnkuEm/as0oYbC8vU1
W3XN0HrpiacGcYIcXU4AY4XvY8t3y76FycJAT9Q6QztVofI5DmXV+8qsyrEegUys
wPIkkumCJ40CgYBKT3hTPZudk8WDNQgT6ZCQQi+Kta3Jp6xVHhC8srDJFqJRcsGY
8ceg/OZifT5EEA6X24W7naxC/qNvhSJsR6Ix3kDBD9AczvOw4X8UOWIxfA5Q6uV+
y61CAzbti0nZep3Z1HzBUmxRLZzmssxKnRmYy9keWzOLI+jYxKDEBpPd9wKBgAY1
pquvDUQwJXal+/xNViK8RPEkE3KTcD+w2KQ9MJVhc1NOxrXZ8Uap76bDi2tzAK9k
qTNQYYErKPnYDjqSUfOfT5SQIPuLYPm1rhYAvHf91TJtwbnkLCKeaP5VgICYUUw9
RGx4uUGVcmteTbdXp86t+naczQw3SEkJAXmVTu8pAoGATF7xXifMUSL1v43Ybrmc
RikQyDecRspMYLOCNmPWI2PPz6MAjm8jDCsXK52HUK4mUqrd/W3rqnl+TrJsXOnH
Ww6tESPaF1kCVyV2Jx/5m8qsE9y5Bds7eMo2JF8vnAKFX6t4KwZiyHBymj6uelNc
wFAbkZY9eS/x6P7qrpd7dUA=
-----END PRIVATE KEY-----"#;
    }
}

cfg_if! {

    if #[cfg(all(target_arch = "wasm32", target_os = "unknown"))] {
        pub fn purge_old_test_group_dirs() {
            // Does nothing on WASM
        }

        #[must_use]
        pub fn get_table_store_path(_test_instance: String) -> String {
            String::new()
        }
        #[must_use]
        pub fn get_block_store_path(_test_instance: String) -> String {
            String::new()
        }
        #[must_use]
        pub fn get_protected_store_path(_test_instance: String) -> String {
            String::new()
        }
        #[must_use]
        pub fn get_certfile_path(_test_instance: String) -> String {
            String::new()
        }
        #[must_use]
        pub fn get_keyfile_path(_test_instance: String) -> String {
            String::new()
        }
    }
    else {

        #[must_use]
        pub fn get_data_dir() -> PathBuf {
            cfg_if! {
                if #[cfg(target_os = "android")] {
                    PathBuf::from(crate::tests::android::get_directories::get_files_dir())
                } else {
                    use directories::*;

                    if let Some(my_proj_dirs) = ProjectDirs::from("org", "Veilid", "VeilidCoreTests") {
                        PathBuf::from(my_proj_dirs.data_local_dir())
                    } else {
                        PathBuf::from("./")
                    }
                }
            }
        }

        #[must_use]
        pub fn get_test_group_dir() -> PathBuf {
            let mut out = get_data_dir();
            let nextest_run_id = std::env::var("NEXTEST_RUN_ID").unwrap_or_default();
            if nextest_run_id.is_empty() {
                out.push(std::process::id().to_string());
            } else {
                out.push(nextest_run_id);
            }
            out
        }

        pub fn purge_old_test_group_dirs() {
            let test_group_dir = get_test_group_dir();
            let data_dir = get_data_dir();
            if let Ok(dirs) = data_dir.read_dir() {
                for entries in dirs.flatten() {
                    let entry_path = entries.path();
                    if !entry_path.starts_with(&test_group_dir) {
                        if entry_path.metadata().unwrap().is_dir() {
                            if let Err(e) = std::fs::remove_dir_all(&entry_path) {
                                error!("failed to remove old test group dir {:?}: {}", entry_path, e);
                            } else {
                                info!("purged old test group dir {:?}", entry_path);
                            }
                        } else if entry_path.metadata().unwrap().is_file() {
                            if let Err(e) = std::fs::remove_file(&entry_path) {
                                error!("failed to remove old test group file {:?}: {}", entry_path, e);
                            } else {
                                info!("purged old test group file {:?}", entry_path);
                            }
                        }
                    }
                }
            }
        }

        #[must_use]
        pub fn get_table_store_path(test_instance: String) -> String {
            eprintln!("get_table_store_path: {:?}", test_instance);
            let mut out = get_test_group_dir();
            out.push(test_instance);
            std::fs::create_dir_all(&out).unwrap();
            out.push("table_store");

            let out = out.into_os_string().into_string().unwrap();
            eprintln!("get_table_store_path result: {:?}", out);
            out
        }

        #[must_use]
        pub fn get_block_store_path(test_instance: String) -> String {
            let mut out = get_test_group_dir();
            out.push(test_instance);
            std::fs::create_dir_all(&out).unwrap();
            out.push("block_store");

            out.into_os_string().into_string().unwrap()
        }

        #[must_use]
        pub fn get_protected_store_path(test_instance: String) -> String {
            let mut out = get_test_group_dir();
            out.push(test_instance);
            std::fs::create_dir_all(&out).unwrap();
            out.push("protected_store");

            out.into_os_string().into_string().unwrap()
        }

        #[must_use]
        pub fn get_certfile_path(test_instance: String) -> String {
            let mut out = get_test_group_dir();
            out.push(test_instance);
            std::fs::create_dir_all(&out).unwrap();

            out.push("cert.pem");
            // Initialize certfile
            if !out.exists() {
                debug!("creating certfile at {:?}", out);
                File::create(&out).unwrap().write_all(CERTFILE.as_bytes()).unwrap();
            }

            out.into_os_string().into_string().unwrap()
        }

        #[must_use]
        pub fn get_keyfile_path(test_instance: String) -> String {
            let mut out = get_test_group_dir();
            out.push(test_instance);
            std::fs::create_dir_all(&out).unwrap();

            out.push("key.pem");

            // Initialize keyfile
            if !out.exists() {
                debug!("creating keyfile at {:?}", out);
                File::create(&out).unwrap().write_all(KEYFILE.as_bytes()).unwrap();
            }

            out.into_os_string().into_string().unwrap()
        }
    }
}

fn fixture_update_callback(update: VeilidUpdate) {
    info!("update_callback: {:?}", update);
}

pub fn fixture_veilid_core() -> (UpdateCallback, VeilidConfig) {
    (Arc::new(fixture_update_callback), fixture_veilid_config())
}

pub fn fixture_veilid_core_with_namespace<S: AsRef<str>>(
    namespace: S,
) -> (UpdateCallback, VeilidConfig) {
    let namespace = namespace.as_ref().to_string();

    let config = fixture_veilid_config();

    (
        Arc::new(fixture_update_callback),
        VeilidConfig {
            namespace: namespace.clone(),
            ..config
        },
    )
}

pub fn fixture_startup_options() -> VeilidStartupOptions {
    VeilidStartupOptions::try_new(fixture_veilid_config(), Arc::new(fixture_update_callback))
        .unwrap()
}

static VEILID_TEST_INSTANCE: LazyLock<Arc<core::sync::atomic::AtomicUsize>> =
    LazyLock::new(|| Arc::new(core::sync::atomic::AtomicUsize::new(0)));

#[must_use]
pub fn fixture_veilid_test_instance() -> String {
    let instance_id = VEILID_TEST_INSTANCE.fetch_add(1, Ordering::Relaxed);

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        instance_id.to_string()
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        format!("{}-{}", std::process::id(), instance_id)
    }
}

#[must_use]
pub fn fixture_veilid_program_name(test_instance: String) -> String {
    format!("VeilidCoreTests-{}", test_instance)
}

pub fn fixture_veilid_config() -> VeilidConfig {
    // Always be cleaning
    purge_old_test_group_dirs();

    let test_instance = fixture_veilid_test_instance();
    VeilidConfig {
        program_name: fixture_veilid_program_name(test_instance.clone()),
        table_store: VeilidConfigTableStore {
            directory: get_table_store_path(test_instance.clone()),
            delete: true,
            ..Default::default()
        },
        block_store: VeilidConfigBlockStore {
            directory: get_block_store_path(test_instance.clone()),
            delete: true,
        },
        protected_store: VeilidConfigProtectedStore {
            allow_insecure_fallback: true,
            always_use_insecure_storage: true,
            directory: get_protected_store_path(test_instance.clone()),
            delete: true,
            ..Default::default()
        },
        network: VeilidConfigNetwork {
            upnp: false,
            tls: VeilidConfigTLS {
                certificate_path: get_certfile_path(test_instance.clone()),
                private_key_path: get_keyfile_path(test_instance.clone()),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

// Network-related code to make sure veilid node is connetected to other peers
pub async fn fixture_wait_for_public_internet_ready(api: &VeilidAPI) {
    info!("wait_for_public_internet_ready");
    loop {
        let state = api.get_state().await.unwrap();
        if state.attachment.public_internet_ready {
            break;
        }
        sleep(1000).await;
    }
    info!("wait_for_public_internet_ready, done");
}
