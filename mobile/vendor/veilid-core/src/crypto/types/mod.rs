use super::*;

use core::cmp::{Eq, Ord, PartialEq, PartialOrd};
use core::convert::TryInto;
use core::fmt;
use core::hash::Hash;

fourcc_type!(CryptoKind);

/// Sort best crypto kinds first
/// Better crypto kinds are 'less', ordered toward the front of a list
#[must_use]
pub fn compare_crypto_kind(a: &CryptoKind, b: &CryptoKind) -> cmp::Ordering {
    let a_idx = VALID_CRYPTO_KINDS.iter().position(|k| k == a);
    let b_idx = VALID_CRYPTO_KINDS.iter().position(|k| k == b);
    if let Some(a_idx) = a_idx {
        if let Some(b_idx) = b_idx {
            // Both are valid, prefer better crypto kind
            a_idx.cmp(&b_idx)
        } else {
            // A is valid, B is not
            cmp::Ordering::Less
        }
    } else if b_idx.is_some() {
        // B is valid, A is not
        cmp::Ordering::Greater
    } else {
        // Both are invalid, so use lex comparison
        a.cmp(b)
    }
}

/// Intersection of crypto kind vectors
#[must_use]
pub fn common_crypto_kinds(a: &[CryptoKind], b: &[CryptoKind]) -> Vec<CryptoKind> {
    a.iter().filter(|ack| b.contains(ack)).copied().collect()
}
mod byte_array_types;
mod crypto_typed;
mod crypto_typed_group;
mod kem_keypair;
mod keypair;
mod nonce;
mod record_key;

pub use byte_array_types::*;
pub(crate) use crypto_typed::*;
pub(crate) use crypto_typed_group::*;
pub use kem_keypair::*;
pub use keypair::*;
pub use record_key::*;

macro_rules! impl_crypto_typed_and_group {
    ($(#[$attr:meta])* $visibility:vis $name:ident) => {
        impl_crypto_typed!($(#[$attr])* $visibility $name);
        impl_crypto_typed_group!($visibility $name);
    };
}

macro_rules! impl_crypto_typed_and_group_and_vec {
    ($(#[$attr:meta])* $visibility:vis $name:ident) => {
        impl_crypto_typed!($(#[$attr])* $visibility $name);
        impl_crypto_typed_group!($visibility $name);
        impl_crypto_typed_vec!($visibility $name);
    };
}

// CryptoKind typed, with group and vector conversions
impl_crypto_typed_and_group_and_vec!(
    /// A KEM public key, sealed to with [`CryptoSystem::hpke_seal`](crate::CryptoSystem::hpke_seal)
    /// (DHKEM-X25519 under VLD0, ML-KEM at VLD1). Signing and identity use [`PublicKey`]; under
    /// VLD0 this key is derivable from one via
    /// [`CryptoSystem::encapsulation_key_from_signing_key`](crate::CryptoSystem::encapsulation_key_from_signing_key).
    pub EncapsulationKey
);
impl_crypto_typed_and_group_and_vec!(
    /// A KEM secret key, opening blobs with [`CryptoSystem::hpke_open`](crate::CryptoSystem::hpke_open)
    /// (DHKEM-X25519 under VLD0, ML-KEM at VLD1). Signing uses [`SecretKey`]; under VLD0 this key
    /// is derivable from one via
    /// [`CryptoSystem::decapsulation_key_from_signing_secret`](crate::CryptoSystem::decapsulation_key_from_signing_secret).
    pub DecapsulationKey
);
impl_crypto_typed_and_group_and_vec!(
    /// A signing public key: verification, identity, and node ids (Ed25519 under VLD0, ML-DSA at
    /// VLD1). KEM encryption to a key uses [`EncapsulationKey`].
    pub PublicKey
);
impl_crypto_typed_and_group_and_vec!(
    /// A signing secret key (Ed25519 under VLD0, ML-DSA at VLD1). KEM decryption uses
    /// [`DecapsulationKey`].
    pub SecretKey
);
impl_crypto_typed_and_group_and_vec!(pub Signature);
impl_crypto_typed_and_group_and_vec!(pub SharedSecret);
impl_crypto_typed_and_group_and_vec!(pub HashDigest);
impl_crypto_typed_and_group_and_vec!(pub OpaqueRecordKey);
impl_crypto_typed_and_group_and_vec!(pub NodeId);
impl_crypto_typed_and_group_and_vec!(pub RouteId);
impl_crypto_typed_and_group_and_vec!(pub MemberId);

// No vector representation
impl_crypto_typed_and_group!(
    /// A signing key pair. KEM key pairs use [`KemKeyPair`].
    pub KeyPair
);
impl_crypto_typed_and_group!(
    /// A KEM key pair for HPKE seal/open. Signing key pairs use [`KeyPair`].
    pub KemKeyPair
);
impl_crypto_typed_and_group!(pub RecordKey);

// Internal types
impl_crypto_typed!(pub(crate) HashCoordinate);
