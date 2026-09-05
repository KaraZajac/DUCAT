use crate::tests_crypto::*;

use super::*;

// Mirror of WASM VeilidRoutingContext DHT transaction tests
// Simple test parameters
const SIMPLE_DHT_RECORD_COUNT: usize = 4;
const SIMPLE_DHT_SUBKEY_SIZE: usize = 16;
const SIMPLE_DHT_SUBKEY_COUNT: u32 = 2;

// Full records test parameters
const FULL_DHT_RECORD_COUNT: usize = 4;
const FULL_DHT_SUBKEY_SIZE: usize = 32768;
const FULL_DHT_SUBKEY_COUNT: u32 = 16;

fn make_test_data(record_count: usize, subkey_size: usize) -> Vec<Vec<u8>> {
    (0..record_count)
        .map(|rec| vec![rec as u8; subkey_size])
        .collect()
}

async fn create_dht_records(
    rc: &RoutingContext,
    record_count: usize,
    subkey_count: u32,
) -> Vec<RecordKey> {
    let mut keys = Vec::new();
    for _ in 0..record_count {
        let rec = rc
            .create_dht_record(
                TEST_CRYPTO_KIND,
                DHTSchema::dflt(subkey_count as u16).unwrap(),
                None,
            )
            .await
            .expect("should create DHT record");
        keys.push(rec.key());
    }
    keys
}

async fn delete_dht_records(rc: &RoutingContext, keys: &[RecordKey]) {
    for key in keys {
        rc.delete_dht_record(key.clone())
            .await
            .expect("should delete DHT record");
    }
}

async fn test_create_empty_transaction_and_drop(api: VeilidAPI) {
    let rc = api.routing_context().unwrap();
    let keys = create_dht_records(&rc, SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_COUNT).await;

    let tx = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction");
    drop(tx);

    delete_dht_records(&rc, &keys).await;
}

async fn test_create_empty_transaction_and_rollback(api: VeilidAPI) {
    let rc = api.routing_context().unwrap();
    let keys = create_dht_records(&rc, SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_COUNT).await;

    let tx = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction");
    tx.rollback().await.expect("should rollback");

    delete_dht_records(&rc, &keys).await;
}

async fn test_create_empty_transaction_and_commit(api: VeilidAPI) {
    let rc = api.routing_context().unwrap();
    let keys = create_dht_records(&rc, SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_COUNT).await;

    let tx = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction");
    tx.commit().await.expect("should commit");

    delete_dht_records(&rc, &keys).await;
}

async fn test_transaction_sets_and_rollback(api: VeilidAPI) {
    let rc = api.routing_context().unwrap();
    let keys = create_dht_records(&rc, SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_COUNT).await;
    let data = make_test_data(SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_SIZE);

    let tx = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction");

    for rec in 0..SIMPLE_DHT_RECORD_COUNT {
        let res = tx
            .set(keys[rec].clone(), 0, data[rec].clone(), None)
            .await
            .expect("set should succeed");
        assert!(res.is_none(), "set should return None");
    }

    tx.rollback().await.expect("should rollback");
    delete_dht_records(&rc, &keys).await;
}

async fn test_transaction_sets_and_commit(api: VeilidAPI) {
    let rc = api.routing_context().unwrap();
    let keys = create_dht_records(&rc, SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_COUNT).await;
    let data = make_test_data(SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_SIZE);

    let tx = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction");

    for rec in 0..SIMPLE_DHT_RECORD_COUNT {
        let res = tx
            .set(keys[rec].clone(), 0, data[rec].clone(), None)
            .await
            .expect("set should succeed");
        assert!(res.is_none(), "set should return None");
    }

    tx.commit().await.expect("should commit");
    delete_dht_records(&rc, &keys).await;
}

async fn test_transaction_sets_gets_and_commit(api: VeilidAPI) {
    let rc = api.routing_context().unwrap();
    let keys = create_dht_records(&rc, SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_COUNT).await;
    let data = make_test_data(SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_SIZE);

    let tx = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction");

    let mut set_futs = Vec::new();
    for rec in 0..SIMPLE_DHT_RECORD_COUNT {
        let tx = tx.clone();
        let key = keys[rec].clone();
        let d = data[rec].clone();
        set_futs.push(async move { tx.set(key, 0, d, None).await });
    }
    let set_results = futures_util::future::join_all(set_futs).await;
    for res in set_results {
        assert!(
            res.expect("set should succeed").is_none(),
            "set should return None"
        );
    }

    let mut get_futs = Vec::new();
    for rec in 0..SIMPLE_DHT_RECORD_COUNT {
        let tx = tx.clone();
        let key = keys[rec].clone();
        get_futs.push(async move { tx.get(key, 0).await });
    }
    let get_results = futures_util::future::join_all(get_futs).await;
    for res in get_results {
        assert!(
            res.expect("get should succeed").is_none(),
            "get should return None before commit"
        );
    }

    tx.commit().await.expect("should commit");
    delete_dht_records(&rc, &keys).await;
}

async fn test_transaction_fail_non_transactional_sets(api: VeilidAPI) {
    let rc = api.routing_context().unwrap();
    let keys = create_dht_records(&rc, SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_COUNT).await;
    let data = make_test_data(SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_SIZE);

    let tx = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction");

    let mut set_futs = Vec::new();
    for subkey in 0..SIMPLE_DHT_SUBKEY_COUNT {
        for rec in 0..SIMPLE_DHT_RECORD_COUNT {
            let rc = rc.clone();
            let key = keys[rec].clone();
            let d = data[rec].clone();
            set_futs
                .push(async move { rc.set_dht_value(key, subkey as ValueSubkey, d, None).await });
        }
    }
    let set_results = futures_util::future::join_all(set_futs).await;
    let mut had_try_again = false;
    for res in set_results {
        if let Err(e) = res {
            match e {
                VeilidAPIError::TryAgain { .. } => {
                    had_try_again = true;
                }
                _ => panic!("expected TryAgain error, got: {}", e),
            }
        }
    }
    assert!(
        had_try_again,
        "should have gotten at least one TryAgain error"
    );

    tx.rollback().await.expect("should rollback");
    delete_dht_records(&rc, &keys).await;
}

async fn test_transaction_inspect_set_commit_then_get(api: VeilidAPI) {
    let rc = api.routing_context().unwrap();
    let keys = create_dht_records(&rc, SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_COUNT).await;
    let data = make_test_data(SIMPLE_DHT_RECORD_COUNT, SIMPLE_DHT_SUBKEY_SIZE);

    let tx = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction");

    let mut inspect_futs = Vec::new();
    for rec in 0..SIMPLE_DHT_RECORD_COUNT {
        let tx = tx.clone();
        let key = keys[rec].clone();
        inspect_futs.push(async move { tx.inspect(key, None, DHTReportScope::SyncGet).await });
    }
    let inspect_results = futures_util::future::join_all(inspect_futs).await;
    for report in inspect_results {
        let report = report.expect("inspect should succeed");
        assert_eq!(
            report.local_seqs(),
            &vec![ValueSeqNum::NONE; SIMPLE_DHT_SUBKEY_COUNT as usize],
            "local seqs should all be None"
        );
        assert_eq!(
            report.network_seqs(),
            &vec![ValueSeqNum::NONE; SIMPLE_DHT_SUBKEY_COUNT as usize],
            "network seqs should all be None"
        );
    }

    let mut set_futs = Vec::new();
    for rec in 0..SIMPLE_DHT_RECORD_COUNT {
        let tx = tx.clone();
        let key = keys[rec].clone();
        let d = data[rec].clone();
        set_futs.push(async move { tx.set(key, 1, d, None).await });
    }
    let set_results = futures_util::future::join_all(set_futs).await;
    for res in set_results {
        assert!(
            res.expect("set should succeed").is_none(),
            "set should return None"
        );
    }

    let mut inspect_futs = Vec::new();
    for rec in 0..SIMPLE_DHT_RECORD_COUNT {
        let tx = tx.clone();
        let key = keys[rec].clone();
        inspect_futs.push(async move { tx.inspect(key, None, DHTReportScope::SyncGet).await });
    }
    let inspect_results = futures_util::future::join_all(inspect_futs).await;
    for report in inspect_results {
        let report = report.expect("inspect should succeed");
        assert_eq!(
            report.local_seqs(),
            &vec![ValueSeqNum::NONE; SIMPLE_DHT_SUBKEY_COUNT as usize],
            "local seqs should still all be None before commit"
        );
        assert_eq!(
            report.network_seqs(),
            &vec![ValueSeqNum::NONE; SIMPLE_DHT_SUBKEY_COUNT as usize],
            "network seqs should still all be None before commit"
        );
    }

    tx.commit().await.expect("should commit");

    let tx2 = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction 2");

    let mut inspect_futs = Vec::new();
    for rec in 0..SIMPLE_DHT_RECORD_COUNT {
        let tx2 = tx2.clone();
        let key = keys[rec].clone();
        inspect_futs.push(async move { tx2.inspect(key, None, DHTReportScope::SyncGet).await });
    }
    let inspect_results = futures_util::future::join_all(inspect_futs).await;
    let expected_local_seqs: Vec<ValueSeqNum> = vec![ValueSeqNum::NONE, ValueSeqNum::ZERO];
    let expected_network_seqs: Vec<ValueSeqNum> = vec![ValueSeqNum::NONE, ValueSeqNum::ZERO];
    for report in inspect_results {
        let report = report.expect("inspect should succeed");
        assert_eq!(
            report.local_seqs(),
            &expected_local_seqs,
            "local seqs should show subkey 1 as seq 0"
        );
        assert_eq!(
            report.network_seqs(),
            &expected_network_seqs,
            "network seqs should show subkey 1 as seq 0"
        );
    }

    let expected = [None, Some(0u32)];
    for subkey in 0..SIMPLE_DHT_SUBKEY_COUNT {
        let mut get_futs = Vec::new();
        for rec in 0..SIMPLE_DHT_RECORD_COUNT {
            let tx2 = tx2.clone();
            let key = keys[rec].clone();
            get_futs.push(async move { (rec, tx2.get(key, subkey as ValueSubkey).await) });
        }
        let get_results = futures_util::future::join_all(get_futs).await;
        for (rec, res) in get_results {
            let val = res.expect("get should succeed");
            if expected[subkey as usize].is_none() {
                assert!(val.is_none(), "subkey {} should be None", subkey);
            } else {
                let val = val.unwrap_or_else(|| {
                    panic!("subkey {} should have data for rec {}", subkey, rec)
                });
                assert_eq!(val.data(), data[rec], "data should match");
                assert_eq!(val.seq(), 0.into(), "seq should be 0");
            }
        }
    }

    tx2.commit().await.expect("should commit tx2");
    delete_dht_records(&rc, &keys).await;
}

async fn test_full_records_inspect_and_gets(api: VeilidAPI) {
    let rc = api.routing_context().unwrap();
    let keys = create_dht_records(&rc, FULL_DHT_RECORD_COUNT, FULL_DHT_SUBKEY_COUNT).await;

    let tx = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction");

    let mut inspect_futs = Vec::new();
    for rec in 0..FULL_DHT_RECORD_COUNT {
        let tx = tx.clone();
        let key = keys[rec].clone();
        inspect_futs.push(async move { tx.inspect(key, None, DHTReportScope::SyncGet).await });
    }
    let inspect_results = futures_util::future::join_all(inspect_futs).await;
    let expected_seqs = vec![ValueSeqNum::NONE; FULL_DHT_SUBKEY_COUNT as usize];
    for report in inspect_results {
        let report = report.expect("inspect should succeed");
        assert_eq!(report.local_seqs(), &expected_seqs);
        assert_eq!(report.network_seqs(), &expected_seqs);
    }

    for subkey in 0..FULL_DHT_SUBKEY_COUNT {
        let mut get_futs = Vec::new();
        for rec in 0..FULL_DHT_RECORD_COUNT {
            let tx = tx.clone();
            let key = keys[rec].clone();
            get_futs.push(async move { tx.get(key, subkey as ValueSubkey).await });
        }
        let get_results = futures_util::future::join_all(get_futs).await;
        for res in get_results {
            assert!(
                res.expect("get should succeed").is_none(),
                "should be None for empty record"
            );
        }
    }

    tx.commit().await.expect("should commit");
    delete_dht_records(&rc, &keys).await;
}

async fn test_full_records_fill_commit_then_get(api: VeilidAPI) {
    let rc = api.routing_context().unwrap();
    let keys = create_dht_records(&rc, FULL_DHT_RECORD_COUNT, FULL_DHT_SUBKEY_COUNT).await;
    let data = make_test_data(FULL_DHT_RECORD_COUNT, FULL_DHT_SUBKEY_SIZE);

    let start = std::time::Instant::now();
    let tx = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction");
    info!("begin transaction: {:?}", start.elapsed());

    for subkey in 0..FULL_DHT_SUBKEY_COUNT {
        let start_set = std::time::Instant::now();
        let mut set_futs = Vec::new();
        for rec in 0..FULL_DHT_RECORD_COUNT {
            let tx = tx.clone();
            let key = keys[rec].clone();
            let d = data[rec].clone();
            set_futs.push(async move { tx.set(key, subkey as ValueSubkey, d, None).await });
        }
        let set_results = futures_util::future::join_all(set_futs).await;
        for res in set_results {
            assert!(
                res.expect("set should succeed").is_none(),
                "set should return None"
            );
        }
        info!("set subkey {}: {:?}", subkey, start_set.elapsed());
    }

    let start_commit = std::time::Instant::now();
    tx.commit().await.expect("should commit");
    info!("commit transaction: {:?}", start_commit.elapsed());

    let start_begin2 = std::time::Instant::now();
    let tx2 = api
        .transact_dht_records(keys.clone(), None)
        .await
        .expect("should create transaction 2");
    info!("begin transaction 2: {:?}", start_begin2.elapsed());

    for subkey in 0..FULL_DHT_SUBKEY_COUNT {
        let start_get = std::time::Instant::now();
        let mut get_futs = Vec::new();
        for rec in 0..FULL_DHT_RECORD_COUNT {
            let tx2 = tx2.clone();
            let key = keys[rec].clone();
            let d = data[rec].clone();
            get_futs.push(async move {
                let val = tx2.get(key, subkey as ValueSubkey).await;
                (rec, d, val)
            });
        }
        let get_results = futures_util::future::join_all(get_futs).await;
        for (rec, expected_data, res) in get_results {
            let val = res
                .unwrap_or_else(|e| panic!("get rec={} subkey={} failed: {}", rec, subkey, e))
                .unwrap_or_else(|| panic!("get rec={} subkey={} returned None", rec, subkey));
            assert_eq!(
                val.data(),
                expected_data,
                "data should match for rec={} subkey={}",
                rec,
                subkey
            );
            assert_eq!(
                val.seq(),
                0.into(),
                "seq should be 0 for rec={} subkey={}",
                rec,
                subkey
            );
        }
        info!("get subkey {}: {:?}", subkey, start_get.elapsed());
    }

    let start_rollback = std::time::Instant::now();
    tx2.rollback().await.expect("should rollback tx2");
    info!("rollback transaction 2: {:?}", start_rollback.elapsed());

    delete_dht_records(&rc, &keys).await;
}

pub async fn test_all() {
    let (update_callback, config) = fixture_veilid_core();
    let api = api_startup(update_callback, config)
        .await
        .expect("startup failed");

    let _ = api.attach().await;
    fixture_wait_for_public_internet_ready(&api).await;

    info!("=== Simple transaction tests ===");

    info!("--- test_create_empty_transaction_and_drop ---");
    test_create_empty_transaction_and_drop(api.clone()).await;

    info!("--- test_create_empty_transaction_and_rollback ---");
    test_create_empty_transaction_and_rollback(api.clone()).await;

    info!("--- test_create_empty_transaction_and_commit ---");
    test_create_empty_transaction_and_commit(api.clone()).await;

    info!("--- test_transaction_sets_and_rollback ---");
    test_transaction_sets_and_rollback(api.clone()).await;

    info!("--- test_transaction_sets_and_commit ---");
    test_transaction_sets_and_commit(api.clone()).await;

    info!("--- test_transaction_sets_gets_and_commit ---");
    test_transaction_sets_gets_and_commit(api.clone()).await;

    info!("--- test_transaction_fail_non_transactional_sets ---");
    test_transaction_fail_non_transactional_sets(api.clone()).await;

    info!("--- test_transaction_inspect_set_commit_then_get ---");
    test_transaction_inspect_set_commit_then_get(api.clone()).await;

    info!("=== Full records transaction tests ===");

    info!("--- test_full_records_inspect_and_gets ---");
    test_full_records_inspect_and_gets(api.clone()).await;

    info!("--- test_full_records_fill_commit_then_get ---");
    test_full_records_fill_commit_then_get(api.clone()).await;

    api.shutdown().await;
}
