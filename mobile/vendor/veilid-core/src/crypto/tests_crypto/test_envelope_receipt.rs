use super::*;

async fn test_envelope_round_trip(
    envelope_version: EnvelopeVersion,
    vcrypto: &AsyncCryptoSystemGuard<'_>,
    network_key: Option<BareSharedSecret>,
) {
    let crypto = vcrypto.crypto();
    if network_key.is_some() {
        info!(
            "--- test envelope round trip {} w/network key ---",
            vcrypto.kind()
        );
    } else {
        info!("--- test envelope round trip {} ---", vcrypto.kind());
    }

    // Create envelope
    let ts = Timestamp::from(0x12345678ABCDEF69u64);
    let nonce = vcrypto.random_nonce().await;
    let (sender_public_key, sender_secret) = vcrypto.generate_keypair().await.into_split();
    let sender_id = crypto
        .routing_table()
        .generate_node_id(&sender_public_key)
        .expect("should generate node id");
    let (recipient_public_key, recipient_secret) = vcrypto.generate_keypair().await.into_split();
    let recipient_id = crypto
        .routing_table()
        .generate_node_id(&recipient_public_key)
        .expect("should generate node id");
    let envelope = match envelope_version {
        ENVELOPE_VERSION_ENV0 => {
            Envelope::try_new_env0(&crypto, vcrypto.kind(), ts, nonce, sender_id, recipient_id)
                .expect("should create envelope")
        }
        _ => {
            panic!("unsupported envelope version");
        }
    };

    // Create arbitrary body
    let body = Bytes::copy_from_slice(b"This is an arbitrary body");

    // Serialize to bytes
    let enc_data = envelope
        .to_encrypted_data(&crypto, body.clone(), &sender_secret, &network_key)
        .await
        .expect("failed to encrypt data");

    // Deserialize from bytes
    let envelope2 = Envelope::try_from_signed_data(&crypto, enc_data.clone(), &network_key)
        .await
        .expect("failed to deserialize envelope from data");

    let body2 = envelope2
        .decrypt_body(&crypto, enc_data.clone(), &recipient_secret, &network_key)
        .await
        .expect("failed to decrypt envelope body");

    // Compare envelope and body
    assert_eq!(envelope, envelope2);
    assert_eq!(body.to_vec(), body2);

    // Deserialize from modified bytes
    let enc_data_len = enc_data.len();
    let mut mod_enc_data: BytesMut = enc_data.clone().into();
    mod_enc_data[enc_data_len - 1] ^= 0x80u8;
    let mod_enc_data = mod_enc_data.freeze();
    assert!(
        Envelope::try_from_signed_data(&crypto, mod_enc_data, &network_key)
            .await
            .is_err(),
        "should have failed to decode envelope with modified signature"
    );
    let mut mod_enc_data2: BytesMut = enc_data.clone().into();
    mod_enc_data2[enc_data_len - 65] ^= 0x80u8;
    let mod_enc_data2 = mod_enc_data2.freeze();
    assert!(
        Envelope::try_from_signed_data(&crypto, mod_enc_data2, &network_key)
            .await
            .is_err(),
        "should have failed to decode envelope with modified data"
    );
}

async fn test_receipt_round_trip(
    receipt_version: ReceiptVersion,
    vcrypto: &AsyncCryptoSystemGuard<'_>,
) {
    let crypto = vcrypto.crypto();
    info!("--- test receipt round trip ---");
    // Create arbitrary body
    let body = Bytes::copy_from_slice(b"This is an arbitrary body");

    // Create receipt
    let nonce = vcrypto.random_nonce().await;
    let (sender_public_key, sender_secret) = vcrypto.generate_keypair().await.into_split();
    let sender_id = crypto
        .routing_table()
        .generate_node_id(&sender_public_key)
        .expect("should generate node id");
    let receipt = match receipt_version {
        RECEIPT_VERSION_RCP0 => {
            Receipt::try_new_rcp0(&crypto, vcrypto.kind(), nonce, sender_id, body)
                .expect("should not fail")
        }
        _ => {
            panic!("unsupported receipt version");
        }
    };

    // Serialize to bytes
    let enc_data = receipt
        .to_signed_data(&crypto, &sender_secret)
        .await
        .expect("failed to make signed data");

    // Deserialize from bytes
    let receipt2 = Receipt::try_from_signed_data(&crypto, enc_data.clone())
        .await
        .expect("failed to deserialize envelope from data");

    // Should not validate even when a single bit is changed
    let mut mod_enc_data: BytesMut = enc_data.clone().into();
    mod_enc_data[5] = 0x01;
    let mod_enc_data = mod_enc_data.freeze();
    let _ = Receipt::try_from_signed_data(&crypto, mod_enc_data)
        .await
        .expect_err("should have failed to decrypt using wrong secret");

    // Compare receipts
    assert_eq!(receipt, receipt2);
}

pub async fn test_all() {
    let api = crypto_tests_startup().await;
    let crypto = api.crypto().unwrap();

    // Test versions
    for ev in VALID_ENVELOPE_VERSIONS {
        for v in VALID_CRYPTO_KINDS {
            let vcrypto = crypto.get_async(v).unwrap();

            test_envelope_round_trip(ev, &vcrypto, None).await;
            test_envelope_round_trip(
                ev,
                &vcrypto,
                Some(vcrypto.random_shared_secret().await.into_value()),
            )
            .await;
        }
    }

    // Test versions
    for rv in VALID_RECEIPT_VERSIONS {
        for v in VALID_CRYPTO_KINDS {
            let vcrypto = crypto.get_async(v).unwrap();

            test_receipt_round_trip(rv, &vcrypto).await;
        }
    }

    crypto_tests_shutdown(api.clone()).await;
    assert!(api.is_shutdown());
}
