use super::*;

use ::hpke::{Deserializable, Kem as KemTrait, OpModeR, OpModeS, Serializable};

// Wire and info constants, isolated because the HPKE RFC is still in review
pub(crate) const HPKE_VERSION_1: u8 = 1;
pub(crate) const HPKE_INFO_LABEL: &[u8] = b"veilid-hpke/1";
pub(crate) const HPKE_HEADER_LENGTH: usize = 5;

// One suite per kind; kinds supply only the Kem type parameter
type HpkeKdf = ::hpke::kdf::HkdfSha256;
type HpkeAead = ::hpke::aead::ChaCha20Poly1305;

pub(crate) fn hpke_info(kind: CryptoKind) -> Vec<u8> {
    let mut info = Vec::with_capacity(HPKE_INFO_LABEL.len() + 4);
    info.extend_from_slice(HPKE_INFO_LABEL);
    info.extend_from_slice(kind.bytes());
    info
}

// `enc` is RFC 9180's per-seal "encapsulated key" (a KEM ciphertext), NOT FIPS 203's
// "encapsulation key" (the KEM public key, our EncapsulationKey); keep it raw bytes
pub(crate) fn build_sealed(kind: CryptoKind, enc: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HPKE_HEADER_LENGTH + enc.len() + ciphertext.len());
    out.push(HPKE_VERSION_1);
    out.extend_from_slice(kind.bytes());
    out.extend_from_slice(enc);
    out.extend_from_slice(ciphertext);
    out
}

pub(crate) fn parse_sealed(
    kind: CryptoKind,
    enc_length: usize,
    sealed: &[u8],
) -> VeilidAPIResult<(&[u8], &[u8])> {
    if sealed.len() < HPKE_HEADER_LENGTH {
        apibail_parse_error!("sealed blob is truncated", sealed.len());
    }
    if sealed[0] != HPKE_VERSION_1 {
        apibail_parse_error!("unsupported hpke version", sealed[0]);
    }
    let blob_kind = CryptoKind::try_from(&sealed[1..HPKE_HEADER_LENGTH])?;
    if blob_kind != kind {
        apibail_invalid_argument!(
            "sealed blob crypto kind does not match",
            "sealed",
            blob_kind
        );
    }
    if sealed.len() < HPKE_HEADER_LENGTH + enc_length {
        apibail_parse_error!("sealed blob is truncated", sealed.len());
    }
    Ok(sealed[HPKE_HEADER_LENGTH..].split_at(enc_length))
}

pub(crate) fn hpke_seal_core<Kem: KemTrait>(
    pk_bytes: &[u8],
    info: &[u8],
    aad: &[u8],
    plaintext: &[u8],
    csprng: &mut impl ::hpke::rand_core::CryptoRng,
) -> VeilidAPIResult<(Vec<u8>, Vec<u8>)> {
    let pk = Kem::PublicKey::from_bytes(pk_bytes).map_err(|e| {
        VeilidAPIError::invalid_argument("hpke_seal", "recipient", map_to_string(e))
    })?;
    let (enc, ciphertext) = ::hpke::single_shot_seal_with_rng::<HpkeAead, HpkeKdf, Kem>(
        &OpModeS::Base,
        &pk,
        info,
        plaintext,
        aad,
        csprng,
    )
    .map_err(map_to_string)
    .map_err(VeilidAPIError::generic)?;
    Ok((enc.to_bytes().to_vec(), ciphertext))
}

pub(crate) fn hpke_open_core<Kem: KemTrait>(
    sk_bytes: &[u8],
    enc: &[u8],
    info: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> VeilidAPIResult<Vec<u8>> {
    let sk = Kem::PrivateKey::from_bytes(sk_bytes)
        .map_err(|e| VeilidAPIError::invalid_argument("hpke_open", "secret", map_to_string(e)))?;
    // enc comes from the blob; failures here stay indistinguishable from decrypt failure
    let enc = Kem::EncappedKey::from_bytes(enc)
        .map_err(map_to_string)
        .map_err(VeilidAPIError::generic)?;
    ::hpke::single_shot_open::<HpkeAead, HpkeKdf, Kem>(
        &OpModeR::Base,
        &sk,
        &enc,
        info,
        ciphertext,
        aad,
    )
    .map_err(map_to_string)
    .map_err(VeilidAPIError::generic)
}
