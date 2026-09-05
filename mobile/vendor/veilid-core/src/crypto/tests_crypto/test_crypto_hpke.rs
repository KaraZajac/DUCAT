use super::*;

use crate::crypto::crypto_system::hpke::{
    hpke_open_core, hpke_seal_core, HPKE_HEADER_LENGTH, HPKE_VERSION_1,
};

const HPKE_TAG_LENGTH: usize = 16;
const HPKE_ENC_LENGTH: usize = 32;

fn hex(s: &str) -> Vec<u8> {
    data_encoding::HEXLOWER.decode(s.as_bytes()).unwrap()
}

// RFC 9180 A.2: DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20Poly1305, base mode
#[derive(serde::Deserialize)]
struct A2TestVector {
    info: String,
    #[serde(rename = "ikmE")]
    ikm_e: String,
    #[serde(rename = "ikmR")]
    ikm_r: String,
    #[serde(rename = "skRm")]
    sk_rm: String,
    #[serde(rename = "pkRm")]
    pk_rm: String,
    enc: String,
    encryption: A2Encryption,
}
#[derive(serde::Deserialize)]
struct A2Encryption {
    aad: String,
    ct: String,
    pt: String,
}

fn a2_test_vector() -> A2TestVector {
    serde_json::from_str(include_str!("rfc9180_a2_base.json")).unwrap()
}

// Replays fixed bytes so the vector's ikmE drives the ephemeral keypair
struct ReplayRng(Vec<u8>);
impl ::hpke::rand_core::TryRng for ReplayRng {
    type Error = core::convert::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        let mut b = [0u8; 4];
        self.try_fill_bytes(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut b = [0u8; 8];
        self.try_fill_bytes(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        let rest = self.0.split_off(dest.len().min(self.0.len()));
        dest[..self.0.len()].copy_from_slice(&self.0);
        self.0 = rest;
        Ok(())
    }
}
impl ::hpke::rand_core::TryCryptoRng for ReplayRng {}

// KATs run against the seal/open core because the public methods pin veilid's own
// info string while the vectors carry the RFC's
pub fn test_hpke_kat_seal() {
    info!("test_hpke_kat_seal");
    let v = a2_test_vector();

    let mut rng = ReplayRng(hex(&v.ikm_e));
    let (enc, ciphertext) = hpke_seal_core::<::hpke::kem::X25519HkdfSha256>(
        &hex(&v.pk_rm),
        &hex(&v.info),
        &hex(&v.encryption.aad),
        &hex(&v.encryption.pt),
        &mut rng,
    )
    .unwrap();
    assert_eq!(enc, hex(&v.enc));
    assert_eq!(ciphertext, hex(&v.encryption.ct));
}

pub fn test_hpke_kat_open() {
    info!("test_hpke_kat_open");
    let v = a2_test_vector();

    // skRm derives from ikmR per RFC 9180 DeriveKeyPair
    {
        use ::hpke::{Kem as _, Serializable as _};
        let (sk, _) = ::hpke::kem::X25519HkdfSha256::derive_keypair(&hex(&v.ikm_r));
        assert_eq!(sk.to_bytes().to_vec(), hex(&v.sk_rm));
    }

    let pt = hpke_open_core::<::hpke::kem::X25519HkdfSha256>(
        &hex(&v.sk_rm),
        &hex(&v.enc),
        &hex(&v.info),
        &hex(&v.encryption.aad),
        &hex(&v.encryption.ct),
    )
    .unwrap();
    assert_eq!(pt, hex(&v.encryption.pt));

    let wrong_aad = hpke_open_core::<::hpke::kem::X25519HkdfSha256>(
        &hex(&v.sk_rm),
        &hex(&v.enc),
        &hex(&v.info),
        b"wrong aad",
        &hex(&v.encryption.ct),
    );
    assert!(wrong_aad.is_err(), "must reject mismatched aad");
}

// The vector's raw x25519 keys are valid KEM keys for the public API
#[cfg(feature = "enable-crypto-vld0")]
pub async fn test_hpke_kat_keys_public_api(crypto: &Crypto) {
    info!("test_hpke_kat_keys_public_api");
    let v = a2_test_vector();
    let vcrypto = crypto.get_async(CRYPTO_KIND_VLD0).unwrap();

    let recipient =
        EncapsulationKey::new(CRYPTO_KIND_VLD0, BareEncapsulationKey::new(&hex(&v.pk_rm)));
    let secret = DecapsulationKey::new(CRYPTO_KIND_VLD0, BareDecapsulationKey::new(&hex(&v.sk_rm)));
    let sealed = vcrypto
        .hpke_seal(
            &recipient,
            Bytes::new(),
            Bytes::copy_from_slice(LOREM_IPSUM),
        )
        .await
        .unwrap();
    let opened = vcrypto
        .hpke_open(&secret, Bytes::new(), sealed)
        .await
        .unwrap();
    assert_eq!(opened, LOREM_IPSUM);
}

pub async fn test_hpke_round_trip(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_hpke_round_trip");
    let (key, secret) = vcrypto.generate_kem_keypair().await.into_split();

    for size in TEST_DATA_SIZES {
        let plaintext = vcrypto.random_bytes(size).await;
        for aad in [
            Bytes::new(),
            Bytes::copy_from_slice(b"some associated data"),
        ] {
            let sealed = vcrypto
                .hpke_seal(&key, aad.clone(), plaintext.clone())
                .await
                .unwrap();
            assert_eq!(
                sealed.len(),
                HPKE_HEADER_LENGTH + HPKE_ENC_LENGTH + plaintext.len() + HPKE_TAG_LENGTH
            );
            let opened = vcrypto.hpke_open(&secret, aad, sealed).await.unwrap();
            assert_eq!(opened, plaintext);
        }
    }

    // seal is randomized: same input yields distinct blobs
    let s1 = vcrypto
        .hpke_seal(&key, Bytes::new(), Bytes::copy_from_slice(LOREM_IPSUM))
        .await
        .unwrap();
    let s2 = vcrypto
        .hpke_seal(&key, Bytes::new(), Bytes::copy_from_slice(LOREM_IPSUM))
        .await
        .unwrap();
    assert_ne!(s1, s2);
}

pub async fn test_generate_kem_keypair(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_generate_kem_keypair");

    let kkp1 = vcrypto.generate_kem_keypair().await;
    let kkp2 = vcrypto.generate_kem_keypair().await;
    assert_ne!(kkp1, kkp2);
    assert_eq!(kkp1.kind(), vcrypto.kind());
    assert_eq!(
        kkp1.key().ref_value().len(),
        vcrypto.encapsulation_key_length()
    );
    assert_eq!(
        kkp1.secret().ref_value().len(),
        vcrypto.decapsulation_key_length()
    );

    // string and serde round-trips
    let s = kkp1.to_string();
    assert_eq!(KemKeyPair::from_str(&s).unwrap(), kkp1);
    let j = serde_json::to_string(&kkp1).unwrap();
    assert_eq!(serde_json::from_str::<KemKeyPair>(&j).unwrap(), kkp1);
}

pub async fn test_hpke_bridge(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_hpke_bridge");
    let (signing_key, signing_secret) = vcrypto.generate_keypair().await.into_split();

    let bridged_key = vcrypto
        .encapsulation_key_from_signing_key(&signing_key)
        .await
        .unwrap();
    let bridged_secret = vcrypto
        .decapsulation_key_from_signing_secret(&signing_secret)
        .await
        .unwrap();
    assert_eq!(bridged_key.kind(), vcrypto.kind());
    assert_eq!(
        bridged_key.ref_value().len(),
        vcrypto.encapsulation_key_length()
    );
    assert_eq!(
        bridged_secret.ref_value().len(),
        vcrypto.decapsulation_key_length()
    );

    // bridged halves form a working KEM pair
    let sealed = vcrypto
        .hpke_seal(
            &bridged_key,
            Bytes::new(),
            Bytes::copy_from_slice(LOREM_IPSUM),
        )
        .await
        .unwrap();
    let opened = vcrypto
        .hpke_open(&bridged_secret, Bytes::new(), sealed)
        .await
        .unwrap();
    assert_eq!(opened, LOREM_IPSUM);

    // derivation is deterministic
    let bridged_key2 = vcrypto
        .encapsulation_key_from_signing_key(&signing_key)
        .await
        .unwrap();
    assert_eq!(bridged_key, bridged_key2);

    // bridged and native pairs are the same kind of key: native seal/open still works alongside
    let (native_key, native_secret) = vcrypto.generate_kem_keypair().await.into_split();
    let sealed = vcrypto
        .hpke_seal(
            &native_key,
            Bytes::new(),
            Bytes::copy_from_slice(LOREM_IPSUM),
        )
        .await
        .unwrap();
    let opened = vcrypto
        .hpke_open(&native_secret, Bytes::new(), sealed)
        .await
        .unwrap();
    assert_eq!(opened, LOREM_IPSUM);

    // a bridged secret does not open blobs sealed to a native key
    let sealed = vcrypto
        .hpke_seal(
            &native_key,
            Bytes::new(),
            Bytes::copy_from_slice(LOREM_IPSUM),
        )
        .await
        .unwrap();
    let result = vcrypto
        .hpke_open(&bridged_secret, Bytes::new(), sealed)
        .await;
    assert!(result.is_err(), "must reject wrong recipient");

    // wrong-length signing keys are rejected
    let short_key = PublicKey::new(vcrypto.kind(), BarePublicKey::new(&[0u8; 5]));
    let result = vcrypto.encapsulation_key_from_signing_key(&short_key).await;
    assert!(matches!(
        result,
        Err(VeilidAPIError::InvalidArgument { .. })
    ));
    let short_secret = SecretKey::new(vcrypto.kind(), BareSecretKey::new(&[0u8; 5]));
    let result = vcrypto
        .decapsulation_key_from_signing_secret(&short_secret)
        .await;
    assert!(matches!(
        result,
        Err(VeilidAPIError::InvalidArgument { .. })
    ));
}

// The bridged x25519 keys match the conversion the KEM itself derives
#[cfg(feature = "enable-crypto-vld0")]
pub async fn test_hpke_bridge_matches_kem_derivation(crypto: &Crypto) {
    info!("test_hpke_bridge_matches_kem_derivation");
    use ::hpke::{Deserializable as _, Kem as _, Serializable as _};

    let vcrypto = crypto.get_async(CRYPTO_KIND_VLD0).unwrap();
    let (signing_key, signing_secret) = vcrypto.generate_keypair().await.into_split();
    let bridged_key = vcrypto
        .encapsulation_key_from_signing_key(&signing_key)
        .await
        .unwrap();
    let bridged_secret = vcrypto
        .decapsulation_key_from_signing_secret(&signing_secret)
        .await
        .unwrap();

    let sk = <::hpke::kem::X25519HkdfSha256 as ::hpke::Kem>::PrivateKey::from_bytes(
        bridged_secret.ref_value(),
    )
    .unwrap();
    let pk = ::hpke::kem::X25519HkdfSha256::sk_to_pk(&sk);
    assert_eq!(pk.to_bytes().to_vec(), bridged_key.ref_value().to_vec());
}

pub async fn test_hpke_wrong_recipient(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_hpke_wrong_recipient");
    let (key, _secret) = vcrypto.generate_kem_keypair().await.into_split();
    let (_key2, secret2) = vcrypto.generate_kem_keypair().await.into_split();

    let sealed = vcrypto
        .hpke_seal(&key, Bytes::new(), Bytes::copy_from_slice(LOREM_IPSUM))
        .await
        .unwrap();
    let result = vcrypto.hpke_open(&secret2, Bytes::new(), sealed).await;
    assert!(result.is_err(), "must reject wrong recipient");
}

pub async fn test_hpke_tamper(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_hpke_tamper");
    let (key, secret) = vcrypto.generate_kem_keypair().await.into_split();
    let aad = Bytes::copy_from_slice(b"aad");

    let sealed = vcrypto
        .hpke_seal(&key, aad.clone(), Bytes::copy_from_slice(LOREM_IPSUM))
        .await
        .unwrap();

    // flip a bit in each field: version, kind, enc, ct body, tag
    let tamper_indexes = [
        0,
        1,
        HPKE_HEADER_LENGTH,
        HPKE_HEADER_LENGTH + HPKE_ENC_LENGTH,
        sealed.len() - 1,
    ];
    for idx in tamper_indexes {
        let mut tampered = sealed.to_vec();
        tampered[idx] ^= 0x80;
        let result = vcrypto
            .hpke_open(&secret, aad.clone(), tampered.into())
            .await;
        assert!(result.is_err(), "must reject tampered byte at {}", idx);
    }

    let result = vcrypto
        .hpke_open(&secret, Bytes::copy_from_slice(b"dab"), sealed)
        .await;
    assert!(result.is_err(), "must reject mismatched aad");
}

pub async fn test_hpke_malformed(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_hpke_malformed");
    let (key, secret) = vcrypto.generate_kem_keypair().await.into_split();

    let sealed = vcrypto
        .hpke_seal(&key, Bytes::new(), Bytes::copy_from_slice(LOREM_IPSUM))
        .await
        .unwrap();

    // truncated below the header, then below the encapsulated key
    for len in [0, HPKE_HEADER_LENGTH - 1, HPKE_HEADER_LENGTH + 10] {
        let result = vcrypto
            .hpke_open(&secret, Bytes::new(), sealed.slice(0..len))
            .await;
        assert!(
            matches!(result, Err(VeilidAPIError::ParseError { .. })),
            "truncation to {} must be a parse error",
            len
        );
    }

    let mut bad_version = sealed.to_vec();
    bad_version[0] = HPKE_VERSION_1 + 1;
    let result = vcrypto
        .hpke_open(&secret, Bytes::new(), bad_version.into())
        .await;
    assert!(
        matches!(result, Err(VeilidAPIError::ParseError { .. })),
        "unknown version must be a parse error"
    );
}

pub async fn test_hpke_invalid_keys(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_hpke_invalid_keys");
    let (key, _secret) = vcrypto.generate_kem_keypair().await.into_split();

    // wrong-length recipient key
    let short_key = EncapsulationKey::new(vcrypto.kind(), BareEncapsulationKey::new(&[0u8; 5]));
    let result = vcrypto
        .hpke_seal(
            &short_key,
            Bytes::new(),
            Bytes::copy_from_slice(LOREM_IPSUM),
        )
        .await;
    assert!(matches!(
        result,
        Err(VeilidAPIError::InvalidArgument { .. })
    ));

    // wrong-length recipient secret
    let sealed = vcrypto
        .hpke_seal(&key, Bytes::new(), Bytes::copy_from_slice(LOREM_IPSUM))
        .await
        .unwrap();
    let short_secret = DecapsulationKey::new(vcrypto.kind(), BareDecapsulationKey::new(&[0u8; 5]));
    let result = vcrypto.hpke_open(&short_secret, Bytes::new(), sealed).await;
    assert!(matches!(
        result,
        Err(VeilidAPIError::InvalidArgument { .. })
    ));
}

pub async fn test_hpke_seal_rejects_all_zero_key(vcrypto: &AsyncCryptoSystemGuard<'_>) {
    info!("test_hpke_seal_rejects_all_zero_key");
    let zero_key = EncapsulationKey::new(
        vcrypto.kind(),
        BareEncapsulationKey::new(&vec![0u8; vcrypto.encapsulation_key_length()]),
    );
    let result = vcrypto
        .hpke_seal(&zero_key, Bytes::new(), Bytes::copy_from_slice(LOREM_IPSUM))
        .await;
    assert!(result.is_err(), "must reject all-zero encapsulation key");
}

async fn test_hpke_cross_kind(crypto: &Crypto) {
    info!("test_hpke_cross_kind");
    for sealer in VALID_CRYPTO_KINDS {
        for opener in VALID_CRYPTO_KINDS {
            if sealer == opener {
                continue;
            }
            let vsealer = crypto.get_async(sealer).unwrap();
            let vopener = crypto.get_async(opener).unwrap();
            let (key, _) = vsealer.generate_kem_keypair().await.into_split();
            let (_, secret) = vopener.generate_kem_keypair().await.into_split();
            let sealed = vsealer
                .hpke_seal(&key, Bytes::new(), Bytes::copy_from_slice(LOREM_IPSUM))
                .await
                .unwrap();
            let result = vopener.hpke_open(&secret, Bytes::new(), sealed).await;
            assert!(
                matches!(result, Err(VeilidAPIError::InvalidArgument { .. })),
                "kind mismatch must be an invalid argument error"
            );
        }
    }
}

pub async fn test_all() {
    test_hpke_kat_seal();
    test_hpke_kat_open();

    let api = crypto_tests_startup().await;
    let crypto = api.crypto().unwrap();

    for v in VALID_CRYPTO_KINDS {
        let vcrypto = crypto.get_async(v).unwrap();
        test_hpke_round_trip(&vcrypto).await;
        test_generate_kem_keypair(&vcrypto).await;
        test_hpke_bridge(&vcrypto).await;
        test_hpke_wrong_recipient(&vcrypto).await;
        test_hpke_tamper(&vcrypto).await;
        test_hpke_malformed(&vcrypto).await;
        test_hpke_invalid_keys(&vcrypto).await;
        test_hpke_seal_rejects_all_zero_key(&vcrypto).await;
    }
    test_hpke_cross_kind(&crypto).await;
    #[cfg(feature = "enable-crypto-vld0")]
    {
        test_hpke_kat_keys_public_api(&crypto).await;
        test_hpke_bridge_matches_kem_derivation(&crypto).await;
    }

    crypto_tests_shutdown(api.clone()).await;
    assert!(api.is_shutdown());
}
