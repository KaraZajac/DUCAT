use super::*;
use crate::routing_table::*;

async fn test_generate_secret(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    // Verify keys generate
    let (public_key, secret_key) = vcrypto.generate_keypair().await.into_split();
    let (public_key2, secret_key2) = vcrypto.generate_keypair().await.into_split();

    // Verify byte patterns are different between public and secret
    assert_ne!(
        public_key.ref_value().bytes(),
        secret_key.ref_value().bytes()
    );
    assert_ne!(
        public_key2.ref_value().bytes(),
        secret_key2.ref_value().bytes()
    );

    // Verify the keys and secrets are different across keypairs
    assert_ne!(public_key, public_key2);
    assert_ne!(secret_key, secret_key2);
}

async fn test_sign_and_verify(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    // Make two keys
    let (public_key, secret_key) = vcrypto.generate_keypair().await.into_split();
    let (public_key2, secret_key2) = vcrypto.generate_keypair().await.into_split();

    let lorem_ipsum = Bytes::copy_from_slice(LOREM_IPSUM);
    let cheezburger = Bytes::copy_from_slice(CHEEZBURGER);
    // Sign the same message twice
    let sig = vcrypto
        .sign(&public_key, &secret_key, lorem_ipsum.clone())
        .await
        .unwrap();
    trace!("sig: {:?}", sig);
    let sig_b = vcrypto
        .sign(&public_key, &secret_key, lorem_ipsum.clone())
        .await
        .unwrap();
    // Sign a second message
    let sig_c = vcrypto
        .sign(&public_key, &secret_key, cheezburger.clone())
        .await
        .unwrap();
    trace!("sig_c: {:?}", sig_c);
    // Verify they are the same signature
    assert_eq!(sig, sig_b);
    // Sign the same message with a different key
    let sig2 = vcrypto
        .sign(&public_key2, &secret_key2, lorem_ipsum.clone())
        .await
        .unwrap();
    // Verify a different key gives a different signature
    assert_ne!(sig2, sig_b);

    // Try using the wrong secret to sign
    let a1 = vcrypto
        .sign(&public_key, &secret_key, lorem_ipsum.clone())
        .await
        .unwrap();
    let a2 = vcrypto
        .sign(&public_key2, &secret_key2, lorem_ipsum.clone())
        .await
        .unwrap();
    let _b1 = vcrypto
        .sign(&public_key, &secret_key2, lorem_ipsum.clone())
        .await
        .unwrap_err();
    let _b2 = vcrypto
        .sign(&public_key2, &secret_key, lorem_ipsum.clone())
        .await
        .unwrap_err();

    assert_ne!(a1, a2);

    assert_eq!(
        vcrypto.verify(&public_key, lorem_ipsum.clone(), &a1).await,
        Ok(true)
    );
    assert_eq!(
        vcrypto.verify(&public_key2, lorem_ipsum.clone(), &a2).await,
        Ok(true)
    );
    assert_eq!(
        vcrypto.verify(&public_key, lorem_ipsum.clone(), &a2).await,
        Ok(false)
    );
    assert_eq!(
        vcrypto.verify(&public_key2, lorem_ipsum.clone(), &a1).await,
        Ok(false)
    );

    // Try verifications that should work
    assert_eq!(
        vcrypto.verify(&public_key, lorem_ipsum.clone(), &sig).await,
        Ok(true)
    );
    assert_eq!(
        vcrypto
            .verify(&public_key, lorem_ipsum.clone(), &sig_b)
            .await,
        Ok(true)
    );
    assert_eq!(
        vcrypto
            .verify(&public_key2, lorem_ipsum.clone(), &sig2)
            .await,
        Ok(true)
    );
    assert_eq!(
        vcrypto
            .verify(&public_key, cheezburger.clone(), &sig_c)
            .await,
        Ok(true)
    );
    // Try verifications that shouldn't work
    assert_eq!(
        vcrypto
            .verify(&public_key2, lorem_ipsum.clone(), &sig)
            .await,
        Ok(false)
    );
    assert_eq!(
        vcrypto
            .verify(&public_key, lorem_ipsum.clone(), &sig2)
            .await,
        Ok(false)
    );
    assert_eq!(
        vcrypto
            .verify(&public_key2, cheezburger.clone(), &sig_c)
            .await,
        Ok(false)
    );
    assert_eq!(
        vcrypto.verify(&public_key, cheezburger.clone(), &sig).await,
        Ok(false)
    );
}

async fn test_key_conversions(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    // Test default key
    let (public_key, secret_key) = (
        PublicKey::new(
            vcrypto.kind(),
            BarePublicKey::new(&vec![0u8; vcrypto.public_key_length()]),
        ),
        SecretKey::new(
            vcrypto.kind(),
            BareSecretKey::new(&vec![0u8; vcrypto.secret_key_length()]),
        ),
    );
    let public_key_string = public_key.to_string();
    trace!("public_key_string: {:?}", public_key_string);
    let public_key_string2 = public_key.to_string();
    trace!("public_key_string2: {:?}", public_key_string2);
    assert_eq!(public_key_string, public_key_string2);

    let secret_key_string = secret_key.to_string();
    trace!("secret_key_string: {:?}", secret_key_string);
    assert_eq!(secret_key_string, public_key_string);

    // Make different keys
    let (public_key2, secret_key2) = vcrypto.generate_keypair().await.into_split();
    trace!("public_key2: {:?}", public_key2);
    trace!("secret_key2: {:?}", secret_key2);
    let (public_key3, secret_key3) = vcrypto.generate_keypair().await.into_split();
    trace!("public_key3: {:?}", public_key3);
    trace!("secret_key3: {:?}", secret_key3);

    let public_key2_string = public_key2.to_string();
    let public_key2_string2 = public_key2.to_string();
    let public_key3_string = public_key3.to_string();
    assert_eq!(public_key2_string, public_key2_string2);
    assert_ne!(public_key3_string, public_key2_string);
    let secret_key2_string = secret_key2.to_string();
    assert_ne!(secret_key2_string, secret_key_string);
    assert_ne!(secret_key2_string, public_key2_string);

    // Assert they convert back correctly
    let public_key_back = PublicKey::try_from(public_key_string.as_str()).unwrap();
    let public_key_back2 = PublicKey::try_from(public_key_string2.as_str()).unwrap();
    assert_eq!(public_key_back, public_key_back2);
    assert_eq!(public_key_back, public_key);
    assert_eq!(public_key_back2, public_key);

    let secret_key_back = SecretKey::try_from(secret_key_string.as_str()).unwrap();
    assert_eq!(secret_key_back, secret_key);

    let public_key2_back = PublicKey::try_from(public_key2_string.as_str()).unwrap();
    let public_key2_back2 = PublicKey::try_from(public_key2_string2.as_str()).unwrap();
    assert_eq!(public_key2_back, public_key2_back2);
    assert_eq!(public_key2_back, public_key2);
    assert_eq!(public_key2_back2, public_key2);

    let secret_key2_back = SecretKey::try_from(secret_key2_string.as_str()).unwrap();
    assert_eq!(secret_key2_back, secret_key2);

    // Assert string roundtrip
    assert_eq!(secret_key2_back.to_string(), secret_key2_string);

    // These conversions should fail
    assert!(BarePublicKey::try_from("whatever!").is_err());
    assert!(BareSecretKey::try_from("whatever!").is_err());
    assert!(BarePublicKey::try_from(" ").is_err());
    assert!(BareSecretKey::try_from(" ").is_err());
}

async fn test_encode_decode(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    let public_key =
        BarePublicKey::try_decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap();
    let secret_key =
        BareSecretKey::try_decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap();
    let public_key_b = BarePublicKey::new(&EMPTY_KEY);
    let secret_key_b = BareSecretKey::new(&EMPTY_KEY_SECRET);
    assert_eq!(public_key, public_key_b);
    assert_eq!(secret_key, secret_key_b);

    let (public_key2, secret_key2) = vcrypto.generate_keypair().await.value().into_split();

    let e1 = public_key.encode();
    trace!("e1:  {:?}", e1);
    assert_eq!(e1, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned());
    let e1s = secret_key.encode();
    trace!("e1s: {:?}", e1s);
    let e2 = public_key2.encode();
    trace!("e2:  {:?}", e2);
    let e2s = secret_key2.encode();
    trace!("e2s: {:?}", e2s);

    let d1 = BarePublicKey::try_decode(e1.as_str()).unwrap();
    trace!("d1:  {:?}", d1);
    assert_eq!(public_key, d1);

    let d1s = BareSecretKey::try_decode(e1s.as_str()).unwrap();
    trace!("d1s: {:?}", d1s);
    assert_eq!(secret_key, d1s);

    let d2 = BarePublicKey::try_decode(e2.as_str()).unwrap();
    trace!("d2:  {:?}", d2);
    assert_eq!(public_key2, d2);

    let d2s = BareSecretKey::try_decode(e2s.as_str()).unwrap();
    trace!("d2s: {:?}", d2s);
    assert_eq!(secret_key2, d2s);

    // Failures
    let f1 = BareSecretKey::try_decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA!");
    assert!(f1.is_err());
    let f2 = BareSecretKey::try_decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&");
    assert!(f2.is_err());
}

pub fn test_typed_convert(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    let tks1 = format!(
        "{}:7lxDEabK_qgjbe38RtBa3IZLrud84P6NhGP-pRTZzdQ",
        vcrypto.kind()
    );
    let tk1 = PublicKey::from_str(&tks1).expect("failed");
    assert!(vcrypto.check_public_key(&tk1).is_ok());
    let tks1x = tk1.to_string();
    assert_eq!(tks1, tks1x);

    let tks2 = format!(
        "{}:7lxDEabK_qgjbe38RtBa3IZLrud84P6NhGP-pRTZzd",
        vcrypto.kind()
    );
    let _tk2 = PublicKey::from_str(&tks2).expect_err("should fail");

    let tks3 = format!(
        "{}:7lxDEabK_qgjbe38RtBa3IZLrud84P6NhGP-pRTZ",
        vcrypto.kind()
    );
    let tk3 = PublicKey::from_str(&tks3).expect("failed");
    assert!(vcrypto.check_public_key(&tk3).is_err());

    let tks4 = "XXXX:7lxDEabK_qgjbe38RtBa3IZLrud84P6NhGP-pRTZzdQ".to_string();
    let tk4 = PublicKey::from_str(&tks4).expect("failed");
    let tks4x = tk4.to_string();
    assert_eq!(tks4, tks4x);
    // Enable this when we switch crypto to using typed keys too
    //assert!(vcrypto.check_public_key(&tk4).is_err());

    let tks5 = "XXX:7lxDEabK_qgjbe38RtBa3IZLrud84P6NhGP-pRTZzdQ".to_string();
    let _tk5 = PublicKey::from_str(&tks5).expect_err("should fail");

    let tks6 = "7lxDEabK_qgjbe38RtBa3IZLrud84P6NhGP-pRTZzdQ".to_string();
    let tk6 = PublicKey::from_str(&tks6).expect("failed");
    let tks6x = tk6.to_string();
    assert!(tks6x.ends_with(&tks6));

    let b = Vec::from(tk6.clone());
    let tk7 = PublicKey::try_from(b).expect("should succeed");
    assert_eq!(tk7, tk6);

    let b = Vec::from(tk6.clone());
    let tk8 = PublicKey::try_from(b.as_slice()).expect("should succeed");
    assert_eq!(tk8, tk6);
}

async fn test_hash(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    let mut s = BTreeSet::<HashDigest>::new();

    let abc = Bytes::copy_from_slice(b"abc");
    let abcd = Bytes::copy_from_slice(b"abcd");
    let empty = Bytes::copy_from_slice(b"");
    let space = Bytes::copy_from_slice(b" ");
    let lorem_ipsum = Bytes::copy_from_slice(LOREM_IPSUM);
    let cheezburger = Bytes::copy_from_slice(CHEEZBURGER);

    let k1 = vcrypto.generate_hash(abc.clone()).await;
    let k2 = vcrypto.generate_hash(abcd.clone()).await;
    let k3 = vcrypto.generate_hash(empty.clone()).await;
    let k4 = vcrypto.generate_hash(space.clone()).await;
    let k5 = vcrypto.generate_hash(lorem_ipsum.clone()).await;
    let k6 = vcrypto.generate_hash(cheezburger.clone()).await;

    s.insert(k1.clone());
    s.insert(k2.clone());
    s.insert(k3.clone());
    s.insert(k4.clone());
    s.insert(k5.clone());
    s.insert(k6.clone());
    assert_eq!(s.len(), 6);

    let v1 = vcrypto.generate_hash(abc.clone()).await;
    let v2 = vcrypto.generate_hash(abcd.clone()).await;
    let v3 = vcrypto.generate_hash(empty.clone()).await;
    let v4 = vcrypto.generate_hash(space.clone()).await;
    let v5 = vcrypto.generate_hash(lorem_ipsum.clone()).await;
    let v6 = vcrypto.generate_hash(cheezburger.clone()).await;

    assert_eq!(k1, v1);
    assert_eq!(k2, v2);
    assert_eq!(k3, v3);
    assert_eq!(k4, v4);
    assert_eq!(k5, v5);
    assert_eq!(k6, v6);

    vcrypto
        .validate_hash(abc.clone(), &v1)
        .await
        .expect("should succeed");
    vcrypto
        .validate_hash(abcd.clone(), &v2)
        .await
        .expect("should succeed");
    vcrypto
        .validate_hash(empty.clone(), &v3)
        .await
        .expect("should succeed");
    vcrypto
        .validate_hash(space.clone(), &v4)
        .await
        .expect("should succeed");
    vcrypto
        .validate_hash(lorem_ipsum.clone(), &v5)
        .await
        .expect("should succeed");
    vcrypto
        .validate_hash(cheezburger.clone(), &v6)
        .await
        .expect("should succeed");
}

async fn test_operations(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    let lorem_ipsum = Bytes::copy_from_slice(LOREM_IPSUM);
    let cheezburger = Bytes::copy_from_slice(CHEEZBURGER);
    let abc = Bytes::copy_from_slice(b"abc");

    let k1 = vcrypto.generate_hash(lorem_ipsum.clone()).await;
    let k2 = vcrypto.generate_hash(cheezburger.clone()).await;
    let k3 = vcrypto.generate_hash(abc.clone()).await;

    // Get distance
    let d1 = k1.to_hash_coordinate().distance(&k2.to_hash_coordinate());
    let d2 = k2.to_hash_coordinate().distance(&k1.to_hash_coordinate());
    let d3 = k1.to_hash_coordinate().distance(&k3.to_hash_coordinate());
    let d4 = k2.to_hash_coordinate().distance(&k3.to_hash_coordinate());

    trace!("d1={:?}", d1);
    trace!("d2={:?}", d2);
    trace!("d3={:?}", d3);
    trace!("d4={:?}", d4);

    // Verify commutativity
    assert_eq!(d1, d2);
    assert!(d1 <= d2);
    assert!(d1 >= d2);
    assert!(d1 >= d2);
    assert!(d1 <= d2);
    assert_eq!(d2, d1);
    assert!(d2 <= d1);
    assert!(d2 >= d1);
    assert!(d2 >= d1);
    assert!(d2 <= d1);

    // Verify nibbles
    assert_eq!(d1.nibble(0), 0x9u8);
    assert_eq!(d1.nibble(1), 0x4u8);
    assert_eq!(d1.nibble(2), 0x3u8);
    assert_eq!(d1.nibble(3), 0x6u8);
    assert_eq!(d1.nibble(63), 0x6u8);

    assert_eq!(d1.first_nonzero_nibble(), Some((0, 0x9u8)));
    assert_eq!(d2.first_nonzero_nibble(), Some((0, 0x9u8)));
    assert_eq!(d3.first_nonzero_nibble(), Some((1, 0x4u8)));
    assert_eq!(d4.first_nonzero_nibble(), Some((0, 0x9u8)));

    // Verify bits
    assert!(d1.bit(0));
    assert!(!d1.bit(1));
    assert!(!d1.bit(7));
    assert!(!d1.bit(8));
    assert!(d1.bit(14));
    assert!(!d1.bit(15));
    assert!(d1.bit(254));
    assert!(!d1.bit(255));

    assert_eq!(d1.first_nonzero_bit(), Some(0));
    assert_eq!(d2.first_nonzero_bit(), Some(0));
    assert_eq!(d3.first_nonzero_bit(), Some(5));
    assert_eq!(d4.first_nonzero_bit(), Some(0));
}

pub fn test_public_key_ordering() {
    let k1 = BarePublicKey::new(&[
        128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ]);
    let k2 = BarePublicKey::new(&[
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ]);
    let k3 = BarePublicKey::new(&[
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 128,
    ]);
    let k4 = BarePublicKey::new(&[
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 1,
    ]);
    let k5 = BarePublicKey::new(&[
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ]);

    assert!(k2 < k1);
    assert!(k3 < k2);
    assert!(k4 < k3);
    assert!(k5 < k4);
}

pub async fn test_all() {
    let api = crypto_tests_startup().await;
    let crypto = api.crypto().unwrap();

    test_public_key_ordering();

    // Test versions
    for v in VALID_CRYPTO_KINDS {
        let vcrypto = crypto.get_async(v).unwrap();

        test_generate_secret(&vcrypto).await;
        test_sign_and_verify(&vcrypto).await;
        test_key_conversions(&vcrypto).await;
        test_encode_decode(&vcrypto).await;
        test_typed_convert(&vcrypto);
        test_hash(&vcrypto).await;
        test_operations(&vcrypto).await;
    }

    crypto_tests_shutdown(api.clone()).await;
    assert!(api.is_shutdown());
}
