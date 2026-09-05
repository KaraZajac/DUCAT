use crate::network_manager::*;
use crate::routing_table::*;
use crate::*;

pub fn fix_typed_node_id(kind: CryptoKind, idx: u8) -> NodeId {
    NodeId::new(
        kind,
        BareNodeId::new(&[
            idx, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ]),
    )
}

#[expect(dead_code)]
pub fn fix_typed_node_id_group(valid_kinds: bool, unknown: bool) -> NodeIdGroup {
    let mut tks = NodeIdGroup::new();
    if valid_kinds {
        VALID_CRYPTO_KINDS.iter().for_each(|k| {
            tks.add(fix_typed_node_id(*k, 0));
        });
    }
    if unknown {
        tks.add(fix_typed_node_id(CryptoKind::new([1, 2, 3, 4]), 0));
    }
    tks
}

pub fn fix_crypto_info_list(valid: bool) -> Vec<CryptoInfo> {
    let mut cil: Vec<CryptoInfo> = vec![];
    VALID_CRYPTO_KINDS.into_iter().for_each(|ck| {
        let ci = match ck {
            #[cfg(feature = "enable-crypto-none")]
            CRYPTO_KIND_NONE => CryptoInfo::NONE {
                public_key: BarePublicKey::from_str(if !valid {
                    "xuYisL8R7-qoUQiJtHVpvemzd1x3mH246cMJSkMp6BQ"
                } else {
                    "xMzvYmY1C0B-pUrB9V1pnUf6A1hSqNTOju39UaFxQoU"
                })
                .expect("should parse bare public key"),
            },
            #[cfg(feature = "enable-crypto-vld0")]
            CRYPTO_KIND_VLD0 => CryptoInfo::VLD0 {
                public_key: BarePublicKey::from_str(if !valid {
                    "v6XPfyOoCP_ZP-CWFNrf_pF_dpxsq74p2LW_Q5Q4yPQ"
                } else {
                    "W7ENB-SUWpPA7usY8ORVQf_si5QmFbD1Uqa89Jg2Uc0"
                })
                .expect("should parse bare public key"),
            },
            // #[cfg(feature = "enable-crypto-vld1")]
            // CRYPTO_KIND_VLD1 => CryptoInfo::VLD1 {
            //     encapsulation_key: todo!(),
            //     signing_key: todo!(),
            // },
            _ => {
                unreachable!("missing match arm");
            }
        };
        cil.push(ci);
    });
    cil
}

pub fn fix_crypto_info_list_secrets() -> SecretKeyGroup {
    let mut skg = SecretKeyGroup::new();
    VALID_CRYPTO_KINDS.into_iter().for_each(|ck| {
        let sk = match ck {
            #[cfg(feature = "enable-crypto-none")]
            CRYPTO_KIND_NONE => {
                SecretKey::from_str("NONE:OzMQnZnK9L-BWrU-CqKWYrgF_KetVysxcRICrl6OvXo")
                    .expect("should parse secret key")
            }
            #[cfg(feature = "enable-crypto-vld0")]
            CRYPTO_KIND_VLD0 => {
                SecretKey::from_str("VLD0:hbdjau5sr3rBNwN68XeWLg3rfXnXLaLqfbbqhELqV1E")
                    .expect("should parse secret key")
            }
            // #[cfg(feature = "enable-crypto-vld1")]
            // CRYPTO_KIND_VLD1 => CryptoInfo::VLD1 {
            //     encapsulation_key: todo!(),
            //     signing_key: todo!(),
            // },
            _ => {
                unreachable!("missing match arm");
            }
        };
        skg.add(sk);
    });
    skg
}

pub fn fix_socket_address() -> SocketAddress {
    SocketAddress::new(Address::IPV4(Ipv4Addr::new(18, 218, 0, 160)), 53)
}

pub fn fix_dial_info_detail() -> DialInfoDetail {
    DialInfoDetail {
        class: DialInfoClass::Direct,
        dial_info: DialInfo::TCP(DialInfoTCP {
            socket_address: fix_socket_address(),
        }),
    }
}

pub fn fix_peer_info(
    routing_table: &RoutingTable,
    crypto_info_list: Vec<CryptoInfo>,
    secret_keys: SecretKeyGroup,
) -> VeilidAPIResult<PeerInfo> {
    let node_info = NodeInfo::new(
        Timestamp::new(0),
        vec![ENVELOPE_VERSION_ENV0],
        crypto_info_list,
        PUBLIC_INTERNET_CAPABILITIES.to_vec(),
        ProtocolTypeSet::new(),
        AddressTypeSet::new(),
        vec![fix_dial_info_detail()],
        vec![],
    );

    PeerInfo::new_from_node_info(
        routing_table,
        RoutingDomain::PublicInternet,
        &secret_keys,
        node_info,
    )
}

pub fn fix_unsigned_peer_info(
    routing_table: &RoutingTable,
    crypto_info_list: Vec<CryptoInfo>,
) -> VeilidAPIResult<PeerInfo> {
    let node_info = NodeInfo::new(
        Timestamp::new(0),
        vec![ENVELOPE_VERSION_ENV0],
        crypto_info_list,
        PUBLIC_INTERNET_CAPABILITIES.to_vec(),
        ProtocolTypeSet::new(),
        AddressTypeSet::new(),
        vec![fix_dial_info_detail()],
        vec![],
    );

    PeerInfo::new_from_unsigned(routing_table, RoutingDomain::PublicInternet, node_info)
}
