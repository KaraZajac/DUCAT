use super::*;

pub async fn test_dh(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_dh");
    let (dht_key, dht_key_secret) = vcrypto.generate_keypair().await.into_split();
    assert!(vcrypto
        .validate_keypair(&dht_key, &dht_key_secret)
        .await
        .expect("should succeed"));
    let (dht_key2, dht_key_secret2) = vcrypto.generate_keypair().await.into_split();
    assert!(vcrypto
        .validate_keypair(&dht_key2, &dht_key_secret2)
        .await
        .expect("should succeed"));

    let r1 = vcrypto
        .compute_dh(&dht_key, &dht_key_secret2)
        .await
        .unwrap();
    let r2 = vcrypto
        .compute_dh(&dht_key2, &dht_key_secret)
        .await
        .unwrap();
    let r3 = vcrypto
        .compute_dh(&dht_key, &dht_key_secret2)
        .await
        .unwrap();
    let r4 = vcrypto
        .compute_dh(&dht_key2, &dht_key_secret)
        .await
        .unwrap();
    assert_eq!(r1, r2);
    assert_eq!(r3, r4);
    assert_eq!(r2, r3);
    trace!("dh: {:?}", r1);

    // test cache
    let r5 = vcrypto.cached_dh(&dht_key, &dht_key_secret2).await.unwrap();
    let r6 = vcrypto.cached_dh(&dht_key2, &dht_key_secret).await.unwrap();
    let r7 = vcrypto.cached_dh(&dht_key, &dht_key_secret2).await.unwrap();
    let r8 = vcrypto.cached_dh(&dht_key2, &dht_key_secret).await.unwrap();
    assert_eq!(r1, r5);
    assert_eq!(r2, r6);
    assert_eq!(r3, r7);
    assert_eq!(r4, r8);
    trace!("cached_dh: {:?}", r5);
}

pub async fn test_dh_rejects_all_zero_public_key(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    let zero_pk = PublicKey::new(
        vcrypto.kind(),
        BarePublicKey::new(&vec![0u8; vcrypto.public_key_length()]),
    );
    let (_, secret) = vcrypto.generate_keypair().await.into_split();
    let result = vcrypto.compute_dh(&zero_pk, &secret).await;
    assert!(
        result.is_err(),
        "compute_dh must reject all-zero public key"
    );
}

pub async fn test_dh_errors(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_dh_errors");

    let (key, secret) = vcrypto.generate_keypair().await.into_split();

    // cached_dh rejects wrong key kind
    let fake_key = PublicKey::new(CRYPTO_KIND_FAKE, key.ref_value().clone());
    let result = vcrypto.cached_dh(&fake_key, &secret).await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // cached_dh rejects wrong secret kind
    let fake_secret = SecretKey::new(CRYPTO_KIND_FAKE, secret.ref_value().clone());
    let result = vcrypto.cached_dh(&key, &fake_secret).await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // cached_dh rejects wrong key length
    let short_key = PublicKey::new(vcrypto.kind(), BarePublicKey::new(&[0u8; 5]));
    let result = vcrypto.cached_dh(&short_key, &secret).await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // vld0: y=2 is off-curve, so decompression fails
    #[cfg(feature = "enable-crypto-vld0")]
    if vcrypto.kind() == CRYPTO_KIND_VLD0 {
        let mut off_curve = [0u8; VLD0_PUBLIC_KEY_LENGTH];
        off_curve[0] = 2;
        let bad_point = PublicKey::new(vcrypto.kind(), BarePublicKey::new(&off_curve));
        let result = vcrypto.compute_dh(&bad_point, &secret).await;
        assert!(matches!(result, Err(VeilidAPIError::Internal { .. })));
    }
}

pub async fn test_generate_shared_secret(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_generate_shared_secret");

    let (key1, secret1) = vcrypto.generate_keypair().await.into_split();
    let (key2, secret2) = vcrypto.generate_keypair().await.into_split();

    // both sides derive the same secret; distinct domains derive distinct secrets
    let ss_a = vcrypto
        .generate_shared_secret(&key1, &secret2, Bytes::copy_from_slice(b"abc123"))
        .await
        .unwrap();
    let ss_b = vcrypto
        .generate_shared_secret(&key2, &secret1, Bytes::copy_from_slice(b"abc123"))
        .await
        .unwrap();
    assert_eq!(ss_a, ss_b);
    let ss_c = vcrypto
        .generate_shared_secret(&key2, &secret1, Bytes::copy_from_slice(b"abc1234"))
        .await
        .unwrap();
    assert_ne!(ss_a, ss_c);

    // domain size sweep
    for size in TEST_DATA_SIZES {
        let domain = vcrypto.random_bytes(size).await;
        let ss1 = vcrypto
            .generate_shared_secret(&key1, &secret2, domain.clone())
            .await
            .unwrap();
        let ss2 = vcrypto
            .generate_shared_secret(&key2, &secret1, domain)
            .await
            .unwrap();
        assert_eq!(ss1, ss2);
    }

    // key exchange failures propagate
    let zero_pk = PublicKey::new(
        vcrypto.kind(),
        BarePublicKey::new(&vec![0u8; vcrypto.public_key_length()]),
    );
    let result = vcrypto
        .generate_shared_secret(&zero_pk, &secret1, Bytes::copy_from_slice(b"abc123"))
        .await;
    assert!(result.is_err(), "must reject all-zero public key");
}

pub async fn test_all() {
    let api = crypto_tests_startup().await;
    let crypto = api.crypto().unwrap();

    for v in VALID_CRYPTO_KINDS {
        let vcrypto = crypto.get_async(v).unwrap();
        test_dh(&vcrypto).await;
        test_dh_rejects_all_zero_public_key(&vcrypto).await;
        test_dh_errors(&vcrypto).await;
        test_generate_shared_secret(&vcrypto).await;
    }

    crypto_tests_shutdown(api.clone()).await;
    assert!(api.is_shutdown());
}
