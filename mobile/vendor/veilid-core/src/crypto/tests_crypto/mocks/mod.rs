use super::*;

pub const LOREM_IPSUM:&[u8] = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. ";
pub const CHEEZBURGER: &[u8] = b"I can has cheezburger";
pub const EMPTY_KEY: [u8; VLD0_PUBLIC_KEY_LENGTH] = [0u8; VLD0_PUBLIC_KEY_LENGTH];
pub const EMPTY_KEY_SECRET: [u8; VLD0_SECRET_KEY_LENGTH] = [0u8; VLD0_SECRET_KEY_LENGTH];
pub const TEST_CRYPTO_KIND: CryptoKind = VALID_CRYPTO_KINDS[0];

// Mocks: these do actually work in real functions

#[must_use]
pub fn mock_keypairs() -> Vec<KeyPair> {
    vec![
    #[cfg(feature = "enable-crypto-vld0")]
    KeyPair::from_str("VLD0:W7ENB-SUWpPA7usY8ORVQf_si5QmFbD1Uqa89Jg2Uc0:hbdjau5sr3rBNwN68XeWLg3rfXnXLaLqfbbqhELqV1E").expect("should parse keypair"),
    #[cfg(feature = "enable-crypto-vld0")]
    KeyPair::from_str("VLD0:v6XPfyOoCP_ZP-CWFNrf_pF_dpxsq74p2LW_Q5Q4yPQ:n-DhHtOU7KWQkdp5to8cpBa_u0RFt2IDZzXPqMTq4O0").expect("should parse keypair"),
    #[cfg(feature = "enable-crypto-none")]
    KeyPair::from_str("NONE:xMzvYmY1C0B-pUrB9V1pnUf6A1hSqNTOju39UaFxQoU:OzMQnZnK9L-BWrU-CqKWYrgF_KetVysxcRICrl6OvXo").expect("should parse keypair"),
    #[cfg(feature = "enable-crypto-none")]
    KeyPair::from_str("NONE:xuYisL8R7-qoUQiJtHVpvemzd1x3mH246cMJSkMp6BQ:ORndT0DuEBVXrvd2S4qWQhZMiKOIZ4JHFjz2tbzWF-s").expect("should parse keypair"),
    ]
}

#[allow(dead_code)]
pub fn mock_keypair() -> KeyPair {
    mock_keypairs()[0].clone()
}

#[must_use]
pub fn mock_public_keys() -> Vec<PublicKey> {
    vec![
        #[cfg(feature = "enable-crypto-vld0")]
        PublicKey::from_str("VLD0:W7ENB-SUWpPA7usY8ORVQf_si5QmFbD1Uqa89Jg2Uc0")
            .expect("should parse public key"),
        #[cfg(feature = "enable-crypto-vld0")]
        PublicKey::from_str("VLD0:v6XPfyOoCP_ZP-CWFNrf_pF_dpxsq74p2LW_Q5Q4yPQ")
            .expect("should parse public key"),
        #[cfg(feature = "enable-crypto-none")]
        PublicKey::from_str("NONE:xMzvYmY1C0B-pUrB9V1pnUf6A1hSqNTOju39UaFxQoU")
            .expect("should parse public key"),
        #[cfg(feature = "enable-crypto-none")]
        PublicKey::from_str("NONE:xuYisL8R7-qoUQiJtHVpvemzd1x3mH246cMJSkMp6BQ")
            .expect("should parse public key"),
    ]
}

pub fn mock_public_key() -> PublicKey {
    mock_public_keys()[0].clone()
}

#[must_use]
pub fn mock_secret_keys() -> Vec<SecretKey> {
    vec![
        #[cfg(feature = "enable-crypto-vld0")]
        SecretKey::from_str("VLD0:hbdjau5sr3rBNwN68XeWLg3rfXnXLaLqfbbqhELqV1E")
            .expect("should parse secret key"),
        #[cfg(feature = "enable-crypto-vld0")]
        SecretKey::from_str("VLD0:n-DhHtOU7KWQkdp5to8cpBa_u0RFt2IDZzXPqMTq4O0")
            .expect("should parse secret key"),
        #[cfg(feature = "enable-crypto-none")]
        SecretKey::from_str("NONE:OzMQnZnK9L-BWrU-CqKWYrgF_KetVysxcRICrl6OvXo")
            .expect("should parse secret key"),
        #[cfg(feature = "enable-crypto-none")]
        SecretKey::from_str("NONE:ORndT0DuEBVXrvd2S4qWQhZMiKOIZ4JHFjz2tbzWF-s")
            .expect("should parse secret key"),
    ]
}

pub fn mock_secret_key() -> SecretKey {
    mock_secret_keys()[0].clone()
}

// Fakes: these don't actually work in real functions

pub fn fake_bare_public_key() -> BarePublicKey {
    let mut fake_key = [0u8; VLD0_PUBLIC_KEY_LENGTH];
    random_bytes(&mut fake_key);
    BarePublicKey::new(&fake_key)
}

pub fn fake_bare_record_key() -> BareRecordKey {
    let mut fake_key = [0u8; VLD0_HASH_DIGEST_LENGTH];
    random_bytes(&mut fake_key);
    let mut fake_encryption_key = [0u8; VLD0_HASH_DIGEST_LENGTH];
    random_bytes(&mut fake_encryption_key);
    BareRecordKey::new(
        BareOpaqueRecordKey::new(&fake_key),
        Some(BareSharedSecret::new(&fake_encryption_key)),
    )
}

pub fn fake_bare_secret_key() -> BareSecretKey {
    let mut fake_key = [0u8; VLD0_SECRET_KEY_LENGTH];
    random_bytes(&mut fake_key);
    BareSecretKey::new(&fake_key)
}

pub fn fake_bare_signature() -> BareSignature {
    let mut fake_key = [0u8; VLD0_SIGNATURE_LENGTH];
    random_bytes(&mut fake_key);
    BareSignature::new(&fake_key)
}

pub fn fake_bare_route_id() -> BareRouteId {
    let mut fake_key = [0u8; VLD0_HASH_DIGEST_LENGTH];
    random_bytes(&mut fake_key);
    BareRouteId::new(&fake_key)
}

pub fn fake_bare_node_id() -> BareNodeId {
    let mut fake_key = [0u8; VLD0_HASH_DIGEST_LENGTH];
    random_bytes(&mut fake_key);
    BareNodeId::new(&fake_key)
}

pub fn fake_bare_member_id() -> BareMemberId {
    let mut fake_key = [0u8; VLD0_HASH_DIGEST_LENGTH];
    random_bytes(&mut fake_key);
    BareMemberId::new(&fake_key)
}

pub fn fake_nonce() -> Nonce {
    let mut fake_nonce = [0u8; VLD0_NONCE_LENGTH];
    random_bytes(&mut fake_nonce);
    Nonce::new(&fake_nonce)
}

pub fn fake_bare_hash_digest() -> BareHashDigest {
    let mut fake_key = [0u8; VLD0_HASH_DIGEST_LENGTH];
    random_bytes(&mut fake_key);
    BareHashDigest::new(&fake_key)
}

pub fn fake_node_id() -> NodeId {
    NodeId::new(CryptoKind::from_str("FAKE").unwrap(), fake_bare_node_id())
}

pub fn fake_route_id() -> RouteId {
    RouteId::new(CryptoKind::from_str("FAKE").unwrap(), fake_bare_route_id())
}

pub fn fake_record_key() -> RecordKey {
    RecordKey::new(
        CryptoKind::from_str("FAKE").unwrap(),
        fake_bare_record_key(),
    )
}

pub fn fake_public_key() -> PublicKey {
    PublicKey::new(
        CryptoKind::from_str("FAKE").unwrap(),
        fake_bare_public_key(),
    )
}

pub fn fake_secret_key() -> SecretKey {
    SecretKey::new(
        CryptoKind::from_str("FAKE").unwrap(),
        fake_bare_secret_key(),
    )
}

pub fn fake_signature() -> Signature {
    Signature::new(CryptoKind::from_str("FAKE").unwrap(), fake_bare_signature())
}
