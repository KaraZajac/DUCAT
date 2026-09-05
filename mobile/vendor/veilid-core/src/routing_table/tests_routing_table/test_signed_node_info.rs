use super::*;
use crate::routing_table::*;
use crate::tests::*;

pub async fn test_signed_node_info() {
    info!("--- test_signed_node_info ---");

    let (update_callback, config) = fixture_veilid_core();
    let api = api_startup(update_callback, config)
        .await
        .expect("startup failed");

    let registry = api.core_context().unwrap().registry();
    let routing_table = registry.routing_table();
    let crypto = api.crypto().unwrap();
    for ck in VALID_CRYPTO_KINDS {
        let vcrypto = crypto.get(ck).unwrap();
        let keypair = vcrypto.generate_keypair();
        let secret_key_group = SecretKeyGroup::from(keypair.secret());

        // Build test node info
        let node_info = NodeInfo::new(
            Timestamp::now(),
            VALID_ENVELOPE_VERSIONS.to_vec(),
            vec![CryptoInfo::VLD0 {
                public_key: keypair.key().value(),
            }],
            PUBLIC_INTERNET_CAPABILITIES.to_vec(),
            ProtocolTypeSet::all(),
            AddressTypeSet::all(),
            vec![DialInfoDetail {
                class: DialInfoClass::Mapped,
                dial_info: DialInfo::udp(fix_socket_address()),
            }],
            vec![RelayInfo::new(
                Timestamp::now(),
                NodeIdGroup::from(NodeId::new(CRYPTO_KIND_VLD0, BareNodeId::default())),
                ProtocolTypeSet::all(),
                AddressTypeSet::all(),
                vec![DialInfoDetail {
                    class: DialInfoClass::Mapped,
                    dial_info: DialInfo::udp(fix_socket_address()),
                }],
                RelayKind::Inbound,
            )],
        );

        // Make peerinfo from nodeinfo
        let pi = PeerInfo::new_from_node_info(
            &routing_table,
            RoutingDomain::PublicInternet,
            &secret_key_group,
            node_info.clone(),
        )
        .unwrap();

        // Test correct validation and decoding
        let opt_pi2 = PeerInfo::new_from_wire(
            &routing_table,
            RoutingDomain::PublicInternet,
            pi.node_info_message(),
            pi.signatures().clone(),
        )
        .expect("should succeed");
        assert!(opt_pi2.is_some(), "should validate");
        let pi2 = opt_pi2.unwrap();

        assert!(
            pi2.equivalent(&pi),
            "should be equivalent:\npi={pi:?}\npi2={pi2:?}"
        );

        // Test with invalid signatures and invalid crypto kind
        let invalid_signatures =
            SignatureGroup::from(Signature::new(CRYPTO_KIND_VLD0, BareSignature::default()));

        let _ = PeerInfo::new_from_wire(
            &routing_table,
            RoutingDomain::PublicInternet,
            pi.node_info_message(),
            invalid_signatures,
        )
        .expect_err("should not validate");

        let invalid_crypto_kind = SignatureGroup::from(Signature::new(
            CryptoKind::new(*b"FOOO"),
            BareSignature::default(),
        ));

        let opt_pi_invalid_crypto_kind = PeerInfo::new_from_wire(
            &routing_table,
            RoutingDomain::PublicInternet,
            pi.node_info_message(),
            invalid_crypto_kind,
        )
        .expect("should validate");

        assert!(
            opt_pi_invalid_crypto_kind.is_none(),
            "should have no valid crypto kinds"
        );
    }

    api.shutdown().await;
}
