use super::*;

pub async fn test_aead(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_aead");

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

    assert!(
        vcrypto
            .decrypt_aead(lorem_ipsum.clone(), &n1, &ss1, None)
            .await
            .is_err(),
        "should fail authentication"
    );

    let body5 = vcrypto
        .encrypt_aead(lorem_ipsum.clone(), &n1, &ss1, None)
        .await
        .unwrap();
    let body6 = vcrypto
        .decrypt_aead(body5.clone(), &n1, &ss1, None)
        .await
        .unwrap();
    let body7 = vcrypto
        .encrypt_aead(lorem_ipsum.clone(), &n1, &ss1, None)
        .await
        .unwrap();
    assert_eq!(body6, lorem_ipsum.clone());
    assert_eq!(body5, body7);
}

pub async fn test_aead_sizes(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_aead_sizes");

    let nonce = vcrypto.random_nonce().await;
    let ss = vcrypto.random_shared_secret().await;
    let aad = Bytes::copy_from_slice(b"some associated data");

    for size in TEST_DATA_SIZES {
        let body = vcrypto.random_bytes(size).await;

        for ad in [None, Some(aad.clone())] {
            let ciphertext = vcrypto
                .encrypt_aead(body.clone(), &nonce, &ss, ad.clone())
                .await
                .unwrap();
            assert_eq!(ciphertext.len(), size + vcrypto.aead_overhead());
            let plaintext = vcrypto
                .decrypt_aead(ciphertext, &nonce, &ss, ad)
                .await
                .unwrap();
            assert_eq!(plaintext, body);
        }

        let ciphertext = vcrypto
            .encrypt_in_place_aead(BytesMut::from(&body[..]), &nonce, &ss, None)
            .await
            .unwrap();
        assert_eq!(ciphertext.len(), size + vcrypto.aead_overhead());
        let plaintext = vcrypto
            .decrypt_in_place_aead(ciphertext, &nonce, &ss, None)
            .await
            .unwrap();
        assert_eq!(plaintext, body);
    }
}

pub async fn test_aead_errors(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_aead_errors");

    let body = Bytes::copy_from_slice(LOREM_IPSUM);
    let nonce = vcrypto.random_nonce().await;
    let ss = vcrypto.random_shared_secret().await;

    // wrong nonce length
    let bad_nonce = Nonce::new(&[0u8; 1]);
    let result = vcrypto
        .encrypt_aead(body.clone(), &bad_nonce, &ss, None)
        .await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));
    let result = vcrypto
        .decrypt_aead(body.clone(), &bad_nonce, &ss, None)
        .await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // wrong shared secret kind
    let fake_ss = SharedSecret::new(
        CRYPTO_KIND_FAKE,
        BareSharedSecret::new(&vec![0u8; vcrypto.shared_secret_length()]),
    );
    let result = vcrypto
        .encrypt_aead(body.clone(), &nonce, &fake_ss, None)
        .await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // wrong shared secret length
    let short_ss = SharedSecret::new(vcrypto.kind(), BareSharedSecret::new(&[0u8; 5]));
    let result = vcrypto
        .encrypt_aead(body.clone(), &nonce, &short_ss, None)
        .await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // no associated data is the same as empty associated data
    let ciphertext = vcrypto
        .encrypt_aead(body.clone(), &nonce, &ss, None)
        .await
        .unwrap();
    let plaintext = vcrypto
        .decrypt_aead(
            ciphertext.clone(),
            &nonce,
            &ss,
            Some(Bytes::copy_from_slice(b"")),
        )
        .await
        .unwrap();
    assert_eq!(plaintext, body);

    // mismatched associated data
    let aad = Bytes::copy_from_slice(b"aad");
    let ciphertext = vcrypto
        .encrypt_aead(body.clone(), &nonce, &ss, Some(aad.clone()))
        .await
        .unwrap();
    for bad_ad in [None, Some(Bytes::copy_from_slice(b"dab"))] {
        let result = vcrypto
            .decrypt_aead(ciphertext.clone(), &nonce, &ss, bad_ad)
            .await;
        assert!(result.is_err(), "must reject mismatched associated data");
    }

    // tampered ciphertext
    let mut tampered = ciphertext.to_vec();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x80;
    let result = vcrypto
        .decrypt_aead(tampered.into(), &nonce, &ss, Some(aad.clone()))
        .await;
    assert!(result.is_err(), "must reject tampered ciphertext");

    // ciphertext shorter than the tag
    let result = vcrypto
        .decrypt_aead(
            ciphertext.slice(0..vcrypto.aead_overhead() - 1),
            &nonce,
            &ss,
            Some(aad),
        )
        .await;
    assert!(result.is_err(), "must reject truncated ciphertext");
}

pub async fn test_all() {
    let api = crypto_tests_startup().await;
    let crypto = api.crypto().unwrap();

    for v in VALID_CRYPTO_KINDS {
        let vcrypto = crypto.get_async(v).unwrap();
        test_aead(&vcrypto).await;
        test_aead_sizes(&vcrypto).await;
        test_aead_errors(&vcrypto).await;
    }

    crypto_tests_shutdown(api.clone()).await;
    assert!(api.is_shutdown());
}
