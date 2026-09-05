use super::*;

pub async fn test_sign_verify(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_sign_verify");

    let (key, secret) = vcrypto.generate_keypair().await.into_split();
    let (key2, _secret2) = vcrypto.generate_keypair().await.into_split();

    for size in TEST_DATA_SIZES {
        let data = vcrypto.random_bytes(size).await;

        let sig = vcrypto.sign(&key, &secret, data.clone()).await.unwrap();
        assert_eq!(sig.ref_value().len(), vcrypto.signature_length());
        assert!(vcrypto.verify(&key, data.clone(), &sig).await.unwrap());

        // wrong signer
        assert!(!vcrypto.verify(&key2, data.clone(), &sig).await.unwrap());

        // corrupted signature
        let mut bad_sig_bytes = sig.ref_value().to_vec();
        bad_sig_bytes[0] ^= 0x80;
        let bad_sig = Signature::new(vcrypto.kind(), BareSignature::new(&bad_sig_bytes));
        assert!(!vcrypto.verify(&key, data.clone(), &bad_sig).await.unwrap());

        // tampered data
        if size > 0 {
            let mut bad_data = data.to_vec();
            bad_data[0] ^= 0x80;
            assert!(!vcrypto.verify(&key, bad_data.into(), &sig).await.unwrap());
        }
    }
}

pub async fn test_sign_verify_errors(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_sign_verify_errors");

    let (key, secret) = vcrypto.generate_keypair().await.into_split();
    let (_key2, secret2) = vcrypto.generate_keypair().await.into_split();
    let data = Bytes::copy_from_slice(LOREM_IPSUM);
    let sig = vcrypto.sign(&key, &secret, data.clone()).await.unwrap();

    // wrong key kinds
    let fake_key = PublicKey::new(CRYPTO_KIND_FAKE, key.ref_value().clone());
    let result = vcrypto.sign(&fake_key, &secret, data.clone()).await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));
    let fake_secret = SecretKey::new(CRYPTO_KIND_FAKE, secret.ref_value().clone());
    let result = vcrypto.sign(&key, &fake_secret, data.clone()).await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));
    let result = vcrypto.verify(&fake_key, data.clone(), &sig).await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // wrong signature length
    let short_sig = Signature::new(vcrypto.kind(), BareSignature::new(&[0u8; 5]));
    let result = vcrypto.verify(&key, data.clone(), &short_sig).await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // mismatched keypair
    let result = vcrypto.sign(&key, &secret2, data.clone()).await;
    assert!(matches!(result, Err(VeilidAPIError::ParseError { .. })));
}

pub async fn test_sign_verify_in_place(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_sign_verify_in_place");

    let (key, secret) = vcrypto.generate_keypair().await.into_split();
    let sig_length = vcrypto.signature_length();

    for size in TEST_DATA_SIZES {
        let data = vcrypto.random_bytes(size).await;
        let mut buf = BytesMut::from(&data[..]);
        buf.resize(size + sig_length, 0u8);

        let signed = vcrypto
            .sign_in_place(&key, &secret, buf, 0..size, size)
            .await
            .unwrap();
        assert!(vcrypto
            .verify_in_place(&key, signed.clone().freeze(), 0..size, size)
            .await
            .unwrap());

        // corrupted embedded signature
        let mut corrupted = signed.clone();
        corrupted[size] ^= 0x80;
        assert!(!vcrypto
            .verify_in_place(&key, corrupted.freeze(), 0..size, size)
            .await
            .unwrap());
    }

    // out-of-bounds range and signature index
    let data = vcrypto.random_bytes(64).await;
    let buf = BytesMut::from(&data[..]);
    let result = vcrypto
        .sign_in_place(&key, &secret, buf.clone(), 0..65, 0)
        .await;
    assert!(matches!(
        result,
        Err(VeilidAPIError::InvalidArgument { .. })
    ));
    let result = vcrypto
        .sign_in_place(&key, &secret, buf.clone(), 0..8, 32)
        .await;
    assert!(matches!(result, Err(VeilidAPIError::ParseError { .. })));
    let result = vcrypto.verify_in_place(&key, data.clone(), 0..65, 0).await;
    assert!(matches!(result, Err(VeilidAPIError::Internal { .. })));
    let result = vcrypto.verify_in_place(&key, data, 0..8, 32).await;
    assert!(matches!(result, Err(VeilidAPIError::Internal { .. })));
}

pub async fn test_all() {
    let api = crypto_tests_startup().await;
    let crypto = api.crypto().unwrap();

    for v in VALID_CRYPTO_KINDS {
        let vcrypto = crypto.get_async(v).unwrap();
        test_sign_verify(&vcrypto).await;
        test_sign_verify_errors(&vcrypto).await;
        test_sign_verify_in_place(&vcrypto).await;
    }

    crypto_tests_shutdown(api.clone()).await;
    assert!(api.is_shutdown());
}
