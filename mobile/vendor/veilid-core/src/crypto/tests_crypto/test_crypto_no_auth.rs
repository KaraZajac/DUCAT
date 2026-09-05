use super::*;

pub async fn test_no_auth(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_no_auth");

    let lorem_ipsum = Bytes::copy_from_slice(LOREM_IPSUM);

    let n1 = vcrypto.random_nonce().await;
    let _n2 = loop {
        let n = vcrypto.random_nonce().await;
        if n != n1 {
            break n;
        }
    };

    let ss1 = vcrypto.random_shared_secret().await;
    let _ss2 = loop {
        let ss = vcrypto.random_shared_secret().await;
        if ss != ss1 {
            break ss;
        }
    };

    let body5 = Bytes::from(
        vcrypto
            .crypt_no_auth_unaligned(lorem_ipsum.clone(), &n1, &ss1)
            .await
            .unwrap(),
    );
    let body6 = vcrypto
        .crypt_no_auth_unaligned(body5.clone(), &n1, &ss1)
        .await
        .unwrap();
    let body7 = vcrypto
        .crypt_no_auth_unaligned(lorem_ipsum.clone(), &n1, &ss1)
        .await
        .unwrap();
    assert_eq!(body6, lorem_ipsum.clone());
    assert_eq!(body5, body7);

    let body5 = vcrypto
        .crypt_no_auth_aligned_8(lorem_ipsum.clone(), &n1, &ss1)
        .await
        .unwrap();
    let body6 = vcrypto
        .crypt_no_auth_aligned_8(Bytes::copy_from_slice(&body5), &n1, &ss1)
        .await
        .unwrap();
    let body7 = vcrypto
        .crypt_no_auth_aligned_8(lorem_ipsum.clone(), &n1, &ss1)
        .await
        .unwrap();
    assert_eq!(body6, lorem_ipsum.clone());
    assert_eq!(body5, body7);
}

pub async fn test_no_auth_sizes(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_no_auth_sizes");

    let nonce = vcrypto.random_nonce().await;
    let ss = vcrypto.random_shared_secret().await;

    for size in TEST_DATA_SIZES {
        let body = vcrypto.random_bytes(size).await;

        let crypted = vcrypto
            .crypt_no_auth_unaligned(body.clone(), &nonce, &ss)
            .await
            .unwrap();
        assert_eq!(crypted.len(), size);

        // every variant produces the same keystream
        let aligned = vcrypto
            .crypt_no_auth_aligned_8(body.clone(), &nonce, &ss)
            .await
            .unwrap();
        assert_eq!(aligned, crypted);

        let b2b = vcrypto
            .crypt_b2b_no_auth(body.clone(), BytesMut::zeroed(size), 0, &nonce, &ss)
            .await
            .unwrap();
        assert_eq!(b2b, crypted);

        let in_place = vcrypto
            .crypt_in_place_no_auth(BytesMut::from(&body[..]), 0..size, &nonce, &ss)
            .await
            .unwrap();
        assert_eq!(in_place, crypted);

        // re-applying reverses
        let plain = vcrypto
            .crypt_no_auth_unaligned(crypted.into(), &nonce, &ss)
            .await
            .unwrap();
        assert_eq!(plain, body);
    }
}

pub async fn test_no_auth_errors(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_no_auth_errors");

    let body = Bytes::copy_from_slice(LOREM_IPSUM);
    let nonce = vcrypto.random_nonce().await;
    let ss = vcrypto.random_shared_secret().await;

    // wrong nonce length
    let bad_nonce = Nonce::new(&[0u8; 1]);
    let result = vcrypto
        .crypt_no_auth_unaligned(body.clone(), &bad_nonce, &ss)
        .await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // wrong shared secret kind
    let fake_ss = SharedSecret::new(
        CRYPTO_KIND_FAKE,
        BareSharedSecret::new(&vec![0u8; vcrypto.shared_secret_length()]),
    );
    let result = vcrypto
        .crypt_no_auth_aligned_8(body.clone(), &nonce, &fake_ss)
        .await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // wrong shared secret length
    let short_ss = SharedSecret::new(vcrypto.kind(), BareSharedSecret::new(&[0u8; 5]));
    let result = vcrypto
        .crypt_no_auth_unaligned(body.clone(), &nonce, &short_ss)
        .await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // in-place range out of bounds
    let result = vcrypto
        .crypt_in_place_no_auth(BytesMut::from(&body[..]), 0..body.len() + 1, &nonce, &ss)
        .await;
    assert!(matches!(result, Err(VeilidAPIError::Internal { .. })));
}

pub async fn test_all() {
    let api = crypto_tests_startup().await;
    let crypto = api.crypto().unwrap();

    for v in VALID_CRYPTO_KINDS {
        let vcrypto = crypto.get_async(v).unwrap();
        test_no_auth(&vcrypto).await;
        test_no_auth_sizes(&vcrypto).await;
        test_no_auth_errors(&vcrypto).await;
    }

    crypto_tests_shutdown(api.clone()).await;
    assert!(api.is_shutdown());
}
