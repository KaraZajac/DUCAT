use super::*;
use crate::tests::*;

async fn startup() -> VeilidAPI {
    info!("test_table_store: starting");
    let (update_callback, config) = fixture_veilid_core();
    api_startup(update_callback, config)
        .await
        .expect("startup failed")
}

async fn shutdown(api: VeilidAPI) {
    info!("test_table_store: shutting down");
    api.shutdown().await;
    info!("test_table_store: finished");
}

async fn test_delete_open_delete(ts: &TableStore) {
    info!("test_delete_open_delete");

    let _ = ts.delete("test").await;
    let db = ts.open("test", 3).await.expect("should have opened");
    assert!(
        ts.delete("test").await.is_err(),
        "should fail because file is opened"
    );
    drop(db);
    assert!(
        ts.delete("test").await.is_ok(),
        "should succeed because file is closed"
    );
    let db = ts.open("test", 3).await.expect("should have opened");
    assert!(
        ts.delete("test").await.is_err(),
        "should fail because file is opened"
    );
    drop(db);
    let db = ts.open("test", 3).await.expect("should have opened");
    assert!(
        ts.delete("test").await.is_err(),
        "should fail because file is opened"
    );
    drop(db);
    assert!(
        ts.delete("test").await.is_ok(),
        "should succeed because file is closed"
    );
}

async fn test_store_delete_load(ts: &TableStore) {
    info!("test_store_delete_load");

    let _ = ts.delete("test").await;
    let db = ts.open("test", 3).await.expect("should have opened");
    assert!(
        ts.delete("test").await.is_err(),
        "should fail because file is opened"
    );

    assert_eq!(
        db.load(0, b"foo").await.unwrap(),
        None,
        "should not load missing key"
    );
    assert!(
        db.store(1, b"foo", b"1234567890").await.is_ok(),
        "should store new key"
    );
    assert_eq!(
        db.load(0, b"foo").await.unwrap(),
        None,
        "should not load missing key"
    );
    assert_eq!(
        db.load(1, b"foo").await.unwrap(),
        Some(b"1234567890".to_vec())
    );

    assert!(
        db.store(1, b"bar", b"FNORD").await.is_ok(),
        "should store new key"
    );
    assert!(
        db.store(0, b"bar", b"ABCDEFGHIJKLMNOPQRSTUVWXYZ")
            .await
            .is_ok(),
        "should store new key"
    );
    assert!(
        db.store(2, b"bar", b"FNORD").await.is_ok(),
        "should store new key"
    );
    assert!(
        db.store(2, b"baz", b"QWERTY").await.is_ok(),
        "should store new key"
    );
    assert!(
        db.store(2, b"bar", b"QWERTYUIOP").await.is_ok(),
        "should store new key"
    );

    assert_eq!(db.load(1, b"bar").await.unwrap(), Some(b"FNORD".to_vec()));
    assert_eq!(
        db.load(0, b"bar").await.unwrap(),
        Some(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ".to_vec())
    );
    assert_eq!(
        db.load(2, b"bar").await.unwrap(),
        Some(b"QWERTYUIOP".to_vec())
    );
    assert_eq!(db.load(2, b"baz").await.unwrap(), Some(b"QWERTY".to_vec()));

    assert_eq!(db.delete(1, b"bar").await.unwrap(), Some(b"FNORD".to_vec()));
    assert_eq!(db.delete(1, b"bar").await.unwrap(), None);
    assert!(
        db.delete(4, b"bar").await.is_err(),
        "can't delete from column that doesn't exist"
    );

    drop(db);
    let db = ts.open("test", 3).await.expect("should have opened");

    assert_eq!(db.load(1, b"bar").await.unwrap(), None);
    assert_eq!(
        db.load(0, b"bar").await.unwrap(),
        Some(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ".to_vec())
    );
    assert_eq!(
        db.load(2, b"bar").await.unwrap(),
        Some(b"QWERTYUIOP".to_vec())
    );
    assert_eq!(db.load(2, b"baz").await.unwrap(), Some(b"QWERTY".to_vec()));
}

async fn test_transaction(ts: &TableStore) {
    info!("test_transaction");

    let _ = ts.delete("test").await;
    let db = ts.open("test", 3).await.expect("should have opened");
    assert!(
        ts.delete("test").await.is_err(),
        "should fail because file is opened"
    );

    let tx = db.transact();
    assert!(tx.store(0, b"aaa", b"a-value").await.is_ok());
    assert!(tx
        .store_json(1, b"bbb", &"b-value".to_owned())
        .await
        .is_ok());
    assert!(tx.store(3, b"ddd", b"d-value").await.is_err());
    assert!(tx.store(0, b"ddd", b"d-value").await.is_ok());
    assert!(tx.delete(0, b"ddd").await.is_ok());
    assert!(tx.commit().await.is_ok());

    let tx = db.transact();
    assert!(tx.delete(2, b"ccc").await.is_ok());
    tx.rollback();

    assert_eq!(db.load(0, b"aaa").await, Ok(Some(b"a-value".to_vec())));
    assert_eq!(
        db.load_json::<String>(1, b"bbb").await,
        Ok(Some("b-value".to_owned()))
    );
    assert_eq!(db.load(0, b"ddd").await, Ok(None));
}

async fn test_json(vcrypto: &AsyncCryptoSystemGuard<'_>, ts: &TableStore) {
    info!("test_json");

    let _ = ts.delete("test").await;
    let db = ts.open("test", 3).await.expect("should have opened");
    let keypair = vcrypto.generate_keypair().await;

    assert!(db.store_json(0, b"asdf", &keypair).await.is_ok());

    assert_eq!(db.load_json::<KeyPair>(0, b"qwer").await.unwrap(), None);

    let d = match db.load_json::<KeyPair>(0, b"asdf").await {
        Ok(x) => x,
        Err(e) => {
            panic!("couldn't decode: {}", e);
        }
    };
    assert_eq!(d, Some(keypair.clone()), "keys should be equal");

    let d = match db.delete_json::<KeyPair>(0, b"asdf").await {
        Ok(x) => x,
        Err(e) => {
            panic!("couldn't decode: {}", e);
        }
    };
    assert_eq!(d, Some(keypair.clone()), "keys should be equal");

    assert!(
        db.store(1, b"foo", b"1234567890").await.is_ok(),
        "should store new key"
    );

    assert!(
        db.load_json::<PublicKey>(1, b"foo").await.is_err(),
        "should fail to unfreeze"
    );
}

async fn test_protect_unprotect(vcrypto: &AsyncCryptoSystemGuard<'_>, ts: &TableStore) {
    info!("test_protect_unprotect");

    let dek1 = SharedSecret::new(
        vcrypto.kind(),
        BareSharedSecret::new(&[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ]),
    );

    let dek2 = SharedSecret::new(
        vcrypto.kind(),
        BareSharedSecret::new(&[
            1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0xFF,
        ]),
    );

    let dek3 = SharedSecret::new(
        vcrypto.kind(),
        BareSharedSecret::new(&[0x80u8; VLD0_SHARED_SECRET_LENGTH]),
    );

    let deks = [dek1, dek2, dek3];
    let passwords = [
        "",
        " ",
        "  ",
        "12345678",
        "|/\\!@#$%^&*()_+",
        "Ⓜ️",
        "🔥🔥♾️",
    ];

    for dek in deks {
        for password in passwords {
            info!("testing dek {} with password {}", dek, password);
            let dek_bytes = ts
                .maybe_protect_device_encryption_key(dek.clone(), password)
                .await
                .unwrap_or_else(|_| panic!("protect: dek: '{}' pw: '{}'", dek, password));

            let unprotected = ts
                .maybe_unprotect_device_encryption_key(&dek_bytes, password)
                .await
                .unwrap_or_else(|_| panic!("unprotect: dek: '{}' pw: '{}'", dek, password));
            assert_eq!(unprotected, dek);
            let invalid_password = format!("{}x", password);
            let _ = ts
                .maybe_unprotect_device_encryption_key(&dek_bytes, &invalid_password)
                .await
                .expect_err(&format!(
                    "invalid_password: dek: '{}' pw: '{}'",
                    dek, &invalid_password
                ));
            if !password.is_empty() {
                let _ = ts
                    .maybe_unprotect_device_encryption_key(&dek_bytes, "")
                    .await
                    .expect_err(&format!("empty_password: dek: '{}' pw: ''", dek));
            }
        }
    }
}

async fn test_store_load_json_many(ts: &TableStore) {
    info!("test_json");

    let _ = ts.delete("test").await;
    let db = ts.open("test", 3).await.expect("should have opened");

    let rows = 16;
    let valuesize = 32768;
    let parallel = 10;

    let value = vec!["ABCD".to_string(); valuesize];

    let mut unord = FuturesUnordered::new();

    let mut r = 0;
    let start_ts = Timestamp::now();
    let mut keys = HashSet::new();
    loop {
        while r < rows && unord.len() < parallel {
            let key = format!("key_{}", r);
            r += 1;

            unord.push(Box::pin(async {
                let key = key;

                db.store_json(0, key.as_bytes(), &value)
                    .await
                    .expect("should store");
                let value2 = db
                    .load_json::<Vec<String>>(0, key.as_bytes())
                    .await
                    .expect("should load")
                    .expect("should exist");
                assert_eq!(value, value2);

                key.as_bytes().to_vec()
            }));
        }
        if let Some(res) = unord.next().await {
            keys.insert(res);
        } else {
            break;
        }
    }

    let stored_keys = db.get_keys(0).await.expect("should get keys");
    let stored_keys_set = stored_keys.into_iter().collect::<HashSet<_>>();
    assert_eq!(stored_keys_set, keys, "should have same keys");

    let end_ts = Timestamp::now();
    trace!(
        "test_store_load_json_many duration={}",
        end_ts.duration_since(start_ts)
    );
}

async fn test_open_zero_columns(ts: &TableStore) {
    info!("test_open_zero_columns");

    let _ = ts.delete("zero_cols").await;

    assert!(
        ts.open("zero_cols", 0).await.is_err(),
        "zero columns should be rejected"
    );

    let db = ts.open("zero_cols", 3).await.expect("should have opened");
    assert_eq!(db.get_column_count().unwrap(), 3);
    assert!(db.store(2, b"k2", b"v2").await.is_ok());
    drop(db);

    assert!(
        ts.open("zero_cols", 0).await.is_err(),
        "zero columns should be rejected even when table exists"
    );

    let _ = ts.delete("zero_cols").await;
}

async fn test_maybe_encrypt_decrypt(ts: &TableStore) {
    info!("test_maybe_encrypt_decrypt");

    let _ = ts.delete("test").await;
    let db = ts.open("test", 3).await.expect("should have opened");

    let max_value_size = VeilidConfigTableStore::default().max_value_size_mb as usize * 1024 * 1024;

    let datas: &[&[u8]] = &[b"", b" ", b"  ", b"12345678", b"|/\\!@#$%^&*()_+",
        b"\xE2\x93\x82\xEF\xB8\x8F\xEF\xB8\x8F", b"\xF0\xEF\xB8\x8F\x9F\x94\xA5\xF0\x9F\x94\xA5\xE2\x99\xBE\xEF\xB8\x8F",
        b"1234567890", b"ABCDEFGHIJKLMNOPQRSTUVWXYZ", b"ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMNOPQRSTUVWXYZ", 
        b"\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF",
        b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
        b"\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02",
        b"\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03\x03",
        b"\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04\x04",
        b"\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05\x05",
        b"\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06\x06",
        b"\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07",
        b"\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08",
        b"\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09\x09",
        b"\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\x0A\
\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\x0B\
\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\x0C\
\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\x0D\
\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\x0E\
\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\x0F\
\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\x10\
",
    ];

    for data in datas.iter().copied() {
        let encrypted = db.maybe_encrypt(&compress_prepend_size(data), true).await;
        let decrypted =
            decompress_size_prepended(&db.maybe_decrypt(&encrypted).await.unwrap(), max_value_size)
                .unwrap();
        assert_eq!(data, &decrypted);
        let encrypted = db.maybe_encrypt(&compress_prepend_size(data), false).await;
        let decrypted =
            decompress_size_prepended(&db.maybe_decrypt(&encrypted).await.unwrap(), max_value_size)
                .unwrap();
        assert_eq!(data, &decrypted);
    }
}

/// protected_store.delete=true with table_store.delete=false should still
/// wipe the tables, via the encryption-key-mismatch path that calls
/// delete_all_in_namespace.
async fn test_protected_store_delete_wipes_tables() {
    info!("test_protected_store_delete_wipes_tables");

    let (update_callback, config_a) = fixture_veilid_core();
    let api1 = api_startup(update_callback.clone(), config_a.clone())
        .await
        .expect("startup 1 failed");
    {
        let ts = api1.table_store().unwrap();
        let db = ts.open("wipe_test", 1).await.expect("open 1 failed");
        db.store(0, b"persist_me", b"hello")
            .await
            .expect("store failed");
        assert_eq!(
            db.load(0, b"persist_me").await.unwrap(),
            Some(b"hello".to_vec())
        );
        drop(db);
    }
    api1.shutdown().await;

    // Same paths; wipe protected store, keep table store files.
    let config_b = VeilidConfig {
        protected_store: VeilidConfigProtectedStore {
            delete: true,
            ..config_a.protected_store.clone()
        },
        table_store: VeilidConfigTableStore {
            delete: false,
            ..config_a.table_store.clone()
        },
        ..config_a
    };
    let api2 = api_startup(update_callback, config_b)
        .await
        .expect("startup 2 failed");
    {
        let ts = api2.table_store().unwrap();
        let db = ts.open("wipe_test", 1).await.expect("open 2 failed");
        assert_eq!(
            db.load(0, b"persist_me").await.unwrap(),
            None,
            "table should be wiped after protected_store.delete cascade"
        );
        drop(db);
    }
    api2.shutdown().await;
}

/// With wipe_on_invalid_device_encryption_key=false, api_startup must
/// surface NotInitialized when an internal table fails to decrypt, instead
/// of silently wiping the user's data.
async fn test_protected_store_delete_no_wipe_returns_not_initialized() {
    info!("test_protected_store_delete_no_wipe_returns_not_initialized");

    let (update_callback, config_a) = fixture_veilid_core();
    let api1 = api_startup(update_callback.clone(), config_a.clone())
        .await
        .expect("startup 1 failed");
    {
        let ts = api1.table_store().unwrap();
        let db = ts.open("nowipe_test", 1).await.expect("open 1 failed");
        db.store(0, b"persist_me", b"hello")
            .await
            .expect("store failed");
        drop(db);
    }
    api1.shutdown().await;

    let config_b = VeilidConfig {
        protected_store: VeilidConfigProtectedStore {
            delete: true,
            ..config_a.protected_store.clone()
        },
        table_store: VeilidConfigTableStore {
            delete: false,
            wipe_on_invalid_device_encryption_key: false,
            ..config_a.table_store.clone()
        },
        ..config_a
    };
    let err = api_startup(update_callback, config_b)
        .await
        .expect_err("startup 2 should fail with NotInitialized");
    assert!(
        matches!(err, VeilidAPIError::NotInitialized),
        "expected NotInitialized, got {:?}",
        err
    );
}

pub async fn test_all() {
    let api = startup().await;
    let crypto = api.crypto().unwrap();
    let ts = api.table_store().unwrap();

    test_store_load_json_many(&ts).await;
    test_maybe_encrypt_decrypt(&ts).await;

    for ck in VALID_CRYPTO_KINDS {
        let vcrypto = crypto.get_async(ck).unwrap();
        test_protect_unprotect(&vcrypto, &ts).await;
        test_delete_open_delete(&ts).await;
        test_open_zero_columns(&ts).await;
        test_store_delete_load(&ts).await;
        test_transaction(&ts).await;
        test_json(&vcrypto, &ts).await;
        let _ = ts.delete("test").await;
    }

    shutdown(api).await;

    test_protected_store_delete_wipes_tables().await;
    test_protected_store_delete_no_wipe_returns_not_initialized().await;
}
