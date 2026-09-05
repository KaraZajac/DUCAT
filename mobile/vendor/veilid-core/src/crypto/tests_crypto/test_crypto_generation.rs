use super::*;

pub async fn test_generation(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    let b1 = vcrypto.random_bytes(32).await;
    let b2 = vcrypto.random_bytes(32).await;
    assert_ne!(b1, b2);
    assert_eq!(b1.len(), 32);
    assert_eq!(b2.len(), 32);
    let b3 = vcrypto.random_bytes(0).await;
    let b4 = vcrypto.random_bytes(0).await;
    assert_eq!(b3, b4);
    assert_eq!(b3.len(), 0);

    assert_ne!(vcrypto.default_salt_length(), 0);

    let password = Bytes::copy_from_slice(b"abc123".as_ref());
    let password2 = Bytes::copy_from_slice(b"abc124".as_ref());

    let salt = Bytes::copy_from_slice(b"qwerasdf".as_ref());
    let salt2 = Bytes::copy_from_slice(b"qwerasdg".as_ref());

    let pstr1 = vcrypto
        .hash_password(password.clone(), salt.clone())
        .await
        .unwrap();
    let pstr2 = vcrypto
        .hash_password(password.clone(), salt.clone())
        .await
        .unwrap();
    assert_eq!(pstr1, pstr2);
    let pstr3 = vcrypto
        .hash_password(password.clone(), salt2.clone())
        .await
        .unwrap();
    assert_ne!(pstr1, pstr3);
    let pstr4 = vcrypto
        .hash_password(password2.clone(), salt.clone())
        .await
        .unwrap();
    assert_ne!(pstr1, pstr4);
    let pstr5 = vcrypto
        .hash_password(password2.clone(), salt2.clone())
        .await
        .unwrap();
    assert_ne!(pstr3, pstr5);

    let short_salt = Bytes::copy_from_slice(b"qwe");
    let long_salt = Bytes::copy_from_slice(
        b"qwerqwerqwerqwerqwerqwerqwerqwerqwerqwerqwerqwerqwerqwerqwerqwerz",
    );

    let _ = vcrypto
        .hash_password(password.clone(), short_salt.clone())
        .await
        .expect_err("should reject short salt");
    let _ = vcrypto
        .hash_password(password.clone(), long_salt.clone())
        .await
        .expect_err("should reject long salt");

    assert!(vcrypto
        .verify_password(password.clone(), &pstr1)
        .await
        .unwrap());
    assert!(vcrypto
        .verify_password(password.clone(), &pstr2)
        .await
        .unwrap());
    assert!(vcrypto
        .verify_password(password.clone(), &pstr3)
        .await
        .unwrap());
    assert!(!vcrypto
        .verify_password(password.clone(), &pstr4)
        .await
        .unwrap());
    assert!(!vcrypto
        .verify_password(password.clone(), &pstr5)
        .await
        .unwrap());

    let ss1 = vcrypto
        .derive_shared_secret(password.clone(), salt.clone())
        .await;
    let ss2 = vcrypto
        .derive_shared_secret(password.clone(), salt.clone())
        .await;
    assert_eq!(ss1, ss2);
    let ss3 = vcrypto
        .derive_shared_secret(password.clone(), salt2.clone())
        .await;
    assert_ne!(ss1, ss3);
    let ss4 = vcrypto
        .derive_shared_secret(password2.clone(), salt.clone())
        .await;
    assert_ne!(ss1, ss4);
    let ss5 = vcrypto
        .derive_shared_secret(password2.clone(), salt2.clone())
        .await;
    assert_ne!(ss3, ss5);

    let _ = vcrypto
        .derive_shared_secret(password.clone(), short_salt.clone())
        .await
        .expect_err("should reject short salt");
    let _ = vcrypto
        .derive_shared_secret(password.clone(), long_salt.clone())
        .await
        .expect_err("should reject long salt");
}

pub async fn test_password_errors(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_password_errors");

    let password = Bytes::copy_from_slice(b"abc123");
    let result = vcrypto.verify_password(password, "not a valid hash").await;
    assert!(matches!(result, Err(VeilidAPIError::ParseError { .. })));
}

pub async fn test_hashing(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_hashing");

    for size in TEST_DATA_SIZES {
        let data = vcrypto.random_bytes(size).await;

        let digest = vcrypto.generate_hash(data.clone()).await;
        assert_eq!(digest.ref_value().len(), vcrypto.hash_digest_length());
        assert!(vcrypto.validate_hash(data.clone(), &digest).await.unwrap());

        // reader form hashes identically
        let mut reader = std::io::Cursor::new(data.to_vec());
        let reader_hash = vcrypto.generate_hash_reader(&mut reader).await.unwrap();
        assert_eq!(reader_hash.ref_value().bytes(), digest.ref_value().bytes());
        let mut reader = std::io::Cursor::new(data.to_vec());
        assert!(vcrypto
            .validate_hash_reader(&mut reader, &digest)
            .await
            .unwrap());

        // tampered data does not validate
        if size > 0 {
            let mut bad_data = data.to_vec();
            bad_data[0] ^= 0x80;
            assert!(!vcrypto
                .validate_hash(bad_data.into(), &digest)
                .await
                .unwrap());
        }
    }

    // wrong digest kind and length
    let data = Bytes::copy_from_slice(LOREM_IPSUM);
    let digest = vcrypto.generate_hash(data.clone()).await;
    let fake_digest = HashDigest::new(CRYPTO_KIND_FAKE, digest.ref_value().clone());
    let result = vcrypto.validate_hash(data.clone(), &fake_digest).await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));
    let short_digest = HashDigest::new(vcrypto.kind(), BareHashDigest::new(&[0u8; 5]));
    let result = vcrypto.validate_hash(data.clone(), &short_digest).await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));

    // reader failures propagate
    struct FailingReader;
    impl std::io::Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("read failed"))
        }
    }
    let result = vcrypto.generate_hash_reader(&mut FailingReader).await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));
    let result = vcrypto
        .validate_hash_reader(&mut FailingReader, &digest)
        .await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));
}

pub async fn test_check_functions(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_check_functions");

    let keypair = vcrypto.generate_keypair().await;
    let (key, secret) = keypair.clone().into_split();
    let ss = vcrypto.random_shared_secret().await;
    let nonce = vcrypto.random_nonce().await;
    let digest = vcrypto.generate_hash(Bytes::copy_from_slice(b"x")).await;
    let sig = vcrypto
        .sign(&key, &secret, Bytes::copy_from_slice(b"x"))
        .await
        .unwrap();

    assert!(vcrypto.check_shared_secret(&ss).is_ok());
    assert!(vcrypto.check_nonce(&nonce).is_ok());
    assert!(vcrypto.check_hash_digest(&digest).is_ok());
    assert!(vcrypto.check_public_key(&key).is_ok());
    assert!(vcrypto.check_secret_key(&secret).is_ok());
    assert!(vcrypto.check_signature(&sig).is_ok());
    assert!(vcrypto.check_keypair(&keypair).is_ok());

    // wrong kind
    let fake_ss = SharedSecret::new(CRYPTO_KIND_FAKE, ss.ref_value().clone());
    assert!(matches!(
        vcrypto.check_shared_secret(&fake_ss),
        Err(VeilidAPIError::Generic { .. })
    ));
    let fake_digest = HashDigest::new(CRYPTO_KIND_FAKE, digest.ref_value().clone());
    assert!(matches!(
        vcrypto.check_hash_digest(&fake_digest),
        Err(VeilidAPIError::Generic { .. })
    ));
    let fake_key = PublicKey::new(CRYPTO_KIND_FAKE, key.ref_value().clone());
    assert!(matches!(
        vcrypto.check_public_key(&fake_key),
        Err(VeilidAPIError::Generic { .. })
    ));
    let fake_secret = SecretKey::new(CRYPTO_KIND_FAKE, secret.ref_value().clone());
    assert!(matches!(
        vcrypto.check_secret_key(&fake_secret),
        Err(VeilidAPIError::Generic { .. })
    ));
    let fake_sig = Signature::new(CRYPTO_KIND_FAKE, sig.ref_value().clone());
    assert!(matches!(
        vcrypto.check_signature(&fake_sig),
        Err(VeilidAPIError::Generic { .. })
    ));
    let fake_keypair = KeyPair::new(
        CRYPTO_KIND_FAKE,
        BareKeyPair::new(key.ref_value().clone(), secret.ref_value().clone()),
    );
    assert!(matches!(
        vcrypto.check_keypair(&fake_keypair),
        Err(VeilidAPIError::Generic { .. })
    ));

    // wrong length
    let short_ss = SharedSecret::new(vcrypto.kind(), BareSharedSecret::new(&[0u8; 5]));
    assert!(vcrypto.check_shared_secret(&short_ss).is_err());
    let short_nonce = Nonce::new(&[0u8; 5]);
    assert!(vcrypto.check_nonce(&short_nonce).is_err());
    let short_digest = HashDigest::new(vcrypto.kind(), BareHashDigest::new(&[0u8; 5]));
    assert!(vcrypto.check_hash_digest(&short_digest).is_err());
    let short_key = PublicKey::new(vcrypto.kind(), BarePublicKey::new(&[0u8; 5]));
    assert!(vcrypto.check_public_key(&short_key).is_err());
    let short_secret = SecretKey::new(vcrypto.kind(), BareSecretKey::new(&[0u8; 5]));
    assert!(vcrypto.check_secret_key(&short_secret).is_err());
    let short_sig = Signature::new(vcrypto.kind(), BareSignature::new(&[0u8; 5]));
    assert!(vcrypto.check_signature(&short_sig).is_err());
}

pub async fn test_validate_keypair(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_validate_keypair");

    let (key1, secret1) = vcrypto.generate_keypair().await.into_split();
    let (_key2, secret2) = vcrypto.generate_keypair().await.into_split();

    assert!(vcrypto.validate_keypair(&key1, &secret1).await.unwrap());
    assert!(!vcrypto.validate_keypair(&key1, &secret2).await.unwrap());

    // wrong kind
    let fake_key = PublicKey::new(CRYPTO_KIND_FAKE, key1.ref_value().clone());
    let result = vcrypto.validate_keypair(&fake_key, &secret1).await;
    assert!(matches!(result, Err(VeilidAPIError::Generic { .. })));
}

pub async fn test_random_bytes_sizes(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_random_bytes_sizes");

    for size in TEST_DATA_SIZES {
        let b1 = vcrypto.random_bytes(size).await;
        assert_eq!(b1.len(), size);
        if size >= 16 {
            let b2 = vcrypto.random_bytes(size).await;
            assert_ne!(b1, b2);
        }
    }
}

pub async fn test_all() {
    let api = crypto_tests_startup().await;
    let crypto = api.crypto().unwrap();

    for v in VALID_CRYPTO_KINDS {
        let vcrypto = crypto.get_async(v).unwrap();
        test_generation(&vcrypto).await;
        test_password_errors(&vcrypto).await;
        test_hashing(&vcrypto).await;
        test_check_functions(&vcrypto).await;
        test_validate_keypair(&vcrypto).await;
        test_random_bytes_sizes(&vcrypto).await;
    }

    crypto_tests_shutdown(api.clone()).await;
    assert!(api.is_shutdown());
}
