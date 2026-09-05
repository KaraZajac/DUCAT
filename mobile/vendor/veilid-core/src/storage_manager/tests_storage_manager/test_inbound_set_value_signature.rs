use super::*;
use crate::tests::*;
use crate::veilid_api::Target;

// Forged descriptor+value (garbage signatures) must be rejected and not persisted
pub async fn test_inbound_set_value_rejects_forged_signature() {
    let (update_callback, config) = fixture_veilid_core();
    let api = api_startup(update_callback, config)
        .await
        .expect("startup failed");

    let registry = api.core_context().unwrap().registry();
    let storage_manager = registry.storage_manager();
    let crypto = api.crypto().unwrap();

    let ck = CRYPTO_KIND_VLD0;
    let vcrypto = crypto.get(ck).expect("vld0 cryptosystem");
    let attacker_keypair = vcrypto.generate_keypair();
    let attacker_owner_pk = attacker_keypair.key();
    let nonce = vcrypto.random_nonce();
    let avcrypto = vcrypto.as_async();

    let schema = DHTSchema::dflt(1).expect("schema");
    let schema_data: Bytes = schema.compile().into();

    // All-zero is a syntactically valid 64-byte Ed25519 signature that verifies against nothing
    let garbage_sig = Signature::new(ck, BareSignature::new(&[0u8; 64]));

    let fake_descriptor = SignedValueDescriptor::new(
        attacker_owner_pk.clone(),
        schema_data.clone(),
        garbage_sig.clone(),
    );

    // Matching opaque record key, so only the signature check (not the key binding) can reject
    let opaque_record_key = StorageManager::make_opaque_record_key(
        &avcrypto,
        attacker_owner_pk.ref_value(),
        &schema_data,
    )
    .await;

    let value_data = EncryptedValueData::new(
        ValueSeqNum::from(1),
        Bytes::from_static(b"poisoned-payload-from-unauthenticated-attacker"),
        attacker_owner_pk.clone(),
        Some(nonce),
    )
    .expect("EncryptedValueData::new should succeed");
    let fake_signed_value = SignedValueData::new(value_data, garbage_sig.clone());

    let attacker_target = Target::NodeId(NodeId::new(ck, BareNodeId::new(&[0u8; 32])));

    let result = storage_manager
        .inbound_set_value(
            &opaque_record_key,
            0,
            Arc::new(fake_signed_value),
            Some(Arc::new(fake_descriptor)),
            attacker_target,
        )
        .await
        .expect("inbound_set_value should not bubble an internal error");

    // Must be rejected, not accepted
    if let NetworkResult::Value(InboundSetValueResult::Success) = result {
        panic!("forged SetValueQ was accepted — signature bypass is NOT fixed");
    }

    // And nothing must have been persisted
    let nr = storage_manager
        .inbound_get_value(&opaque_record_key, 0, true)
        .await
        .expect("inbound_get_value should not error");
    if let NetworkResult::Value(InboundGetValueResult::Success(get_result)) = nr {
        assert!(
            get_result.opt_value.is_none(),
            "forged value must not be stored in the remote record store"
        );
    }

    api.shutdown().await;
}

pub async fn test_all() {
    test_inbound_set_value_rejects_forged_signature().await;
}
