use super::*;

async fn test_startup_shutdown() {
    info!("test_startup_shutdown: starting");
    let (update_callback, config) = fixture_veilid_core();
    let api = api_startup(update_callback, config)
        .await
        .expect_or_log("startup failed");

    // Test initial state
    assert!(!api.is_shutdown(), "API should not be shut down initially");

    info!("test_startup_shutdown: shutting down");
    let api_clone = api.clone();
    api.shutdown().await;

    // Test state after shutdown
    assert!(
        api_clone.is_shutdown(),
        "API should be shut down after shutdown()"
    );

    info!("test_startup_shutdown: finished");
}

async fn test_startup_shutdown_from_config() {
    info!("test_startup_from_config: starting");
    let (update_callback, config) = fixture_veilid_core();
    let config_json = serialize_json(config);
    let api = api_startup_json(update_callback, config_json)
        .await
        .expect("startup failed");
    info!("test_startup_from_config: shutting down");
    api.shutdown().await;
    info!("test_startup_from_config: finished");
}

async fn test_attach_detach() {
    info!("test_attach_detach: --- test normal order ---");
    let (update_callback, config) = fixture_veilid_core();
    let api = api_startup(update_callback, config)
        .await
        .expect("startup failed");
    api.attach().await.unwrap();
    sleep(5000).await;
    api.detach().await.unwrap();
    sleep(2000).await;
    api.shutdown().await;

    info!("test_attach_detach: --- test auto detach ---");
    let (update_callback, config) = fixture_veilid_core();
    let api = api_startup(update_callback, config)
        .await
        .expect("startup failed");
    api.attach().await.unwrap();
    sleep(5000).await;
    api.shutdown().await;

    info!("test_attach_detach: --- test detach without attach ---");
    let (update_callback, config) = fixture_veilid_core();
    let api = api_startup(update_callback, config)
        .await
        .expect("startup failed");
    assert!(api.detach().await.is_err());
    api.shutdown().await;
}

async fn test_startup_shutdown_multiple() {
    info!("test_startup_shutdown_multiple: starting");
    let namespaces = (0..3).map(|x| format!("ns_{}", x)).collect::<Vec<_>>();
    let mut apis = vec![];
    for ns in &namespaces {
        let (update_callback, config) = fixture_veilid_core_with_namespace(ns);
        let api = api_startup(update_callback, config)
            .await
            .expect("startup failed");
        apis.push(api);
    }
    info!("test_startup_shutdown_multiple: shutting down");
    for api in apis {
        api.shutdown().await;
    }
    info!("test_startup_shutdown_multiple: finished");
}

async fn test_startup_shutdown_from_config_multiple() {
    info!("test_startup_from_config_multiple: starting");

    let namespaces = (0..3).map(|x| format!("ns_{}", x)).collect::<Vec<_>>();
    let mut apis = vec![];
    for ns in &namespaces {
        let (update_callback, config) = fixture_veilid_core_with_namespace(ns);
        let api = api_startup(update_callback, config)
            .await
            .expect("startup failed");
        apis.push(api);
    }
    info!("test_startup_from_config_multiple: shutting down");
    for api in apis {
        api.shutdown().await;
    }
    info!("test_startup_from_config_multiple: finished");
}

async fn test_attach_detach_multiple() {
    info!("test_attach_detach_multiple: --- test normal order ---");
    let namespaces = (0..3).map(|x| format!("ns_{}", x)).collect::<Vec<_>>();
    let mut apis = vec![];
    for ns in &namespaces {
        let (update_callback, config) = fixture_veilid_core_with_namespace(ns);
        let api = api_startup(update_callback, config)
            .await
            .expect("startup failed");
        apis.push(api);
    }
    for api in &apis {
        api.attach().await.unwrap();
    }
    sleep(5000).await;
    for api in &apis {
        api.detach().await.unwrap();
    }
    sleep(2000).await;
    for api in apis {
        api.shutdown().await;
    }

    info!("test_attach_detach_multiple: --- test auto detach ---");
    let mut apis = vec![];
    for ns in &namespaces {
        let (update_callback, config) = fixture_veilid_core_with_namespace(ns);
        let api = api_startup(update_callback, config)
            .await
            .expect("startup failed");
        apis.push(api);
    }

    for api in &apis {
        api.attach().await.unwrap();
    }
    sleep(5000).await;
    for api in apis {
        api.shutdown().await;
    }

    info!("test_attach_detach_multiple: --- test detach without attach ---");
    let mut apis = vec![];
    for ns in &namespaces {
        let (update_callback, config) = fixture_veilid_core_with_namespace(ns);
        let api = api_startup(update_callback, config)
            .await
            .expect("startup failed");
        apis.push(api);
    }
    for api in &apis {
        assert!(api.detach().await.is_err());
    }
    for api in apis {
        api.shutdown().await;
    }
}

pub async fn test_all() {
    test_startup_shutdown().await;
    test_startup_shutdown_from_config().await;
    test_attach_detach().await;
    test_startup_shutdown_multiple().await;
    test_startup_shutdown_from_config_multiple().await;
    test_attach_detach_multiple().await;
}
