mod sizes;

use super::*;

use super::hpke::{build_sealed, hpke_info, hpke_open_core, hpke_seal_core, parse_sealed};

use argon2::{
    password_hash::{
        phc::{PasswordHash, Salt},
        PasswordHasher, PasswordVerifier,
    },
    Argon2,
};
use chacha20::cipher::{KeyIvInit, StreamCipher};
use chacha20::XChaCha20;
use chacha20poly1305 as ch;
use chacha20poly1305::aead::AeadInOut as _;
use chacha20poly1305::KeyInit as _;
use curve25519_dalek::digest::Digest as _;
use ed25519_dalek as ed;
use x25519_dalek as xd;

pub use chacha20poly1305::aead::Buffer;

const VLD0_DOMAIN_SIGN: &[u8] = b"VLD0_SIGN";
const VLD0_DOMAIN_CRYPT: &[u8] = b"VLD0_CRYPT";

const VLD0_AEAD_OVERHEAD: usize = 16;
/// [`CryptoKind`] fourcc identifying the VLD0 cryptosystem.
pub const CRYPTO_KIND_VLD0: CryptoKind = CryptoKind::new(*b"VLD0");
/// The VLD0 fourcc as a big-endian `u32`.
pub const CRYPTO_KIND_VLD0_FOURCC: u32 = u32::from_be_bytes(*b"VLD0");
pub use sizes::*;

fn public_to_x25519_pk(public: &PublicKey) -> VeilidAPIResult<xd::PublicKey> {
    let pk_ed = ed::VerifyingKey::from_bytes(
        public
            .ref_value()
            .as_ref()
            .try_into()
            .map_err(VeilidAPIError::internal)?,
    )
    .map_err(VeilidAPIError::internal)?;
    Ok(xd::PublicKey::from(*pk_ed.to_montgomery().as_bytes()))
}
fn secret_to_x25519_sk(secret: &SecretKey) -> VeilidAPIResult<xd::StaticSecret> {
    // NOTE: ed::SigningKey.to_scalar() does not produce an unreduced scalar, we want the raw bytes here
    // See https://github.com/dalek-cryptography/curve25519-dalek/issues/565
    let hash: [u8; VLD0_SIGNATURE_LENGTH] = ed::Sha512::default()
        .chain_update(secret.ref_value().bytes())
        .finalize()
        .into();
    let mut output = [0u8; VLD0_SECRET_KEY_LENGTH];
    output.copy_from_slice(&hash[..VLD0_SECRET_KEY_LENGTH]);

    Ok(xd::StaticSecret::from(output))
}

pub(crate) fn vld0_generate_keypair() -> KeyPair {
    let mut csprng = VeilidRng {};
    let signing_key = ed::SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();
    let public_key = BarePublicKey::new(&verifying_key.to_bytes());
    let secret_key = BareSecretKey::new(&signing_key.to_bytes());

    KeyPair::new(CRYPTO_KIND_VLD0, BareKeyPair::new(public_key, secret_key))
}

pub(crate) fn vld0_generate_kem_keypair() -> KemKeyPair {
    use ::hpke::{Kem as _, Serializable as _};
    // RFC 9180 DeriveKeyPair(random ikm), the same path seal/open use
    let (sk, pk) = ::hpke::kem::X25519HkdfSha256::gen_keypair_with_rng(&mut VeilidRng {});

    KemKeyPair::new(
        CRYPTO_KIND_VLD0,
        BareKemKeyPair::new(
            BareEncapsulationKey::new(&pk.to_bytes()),
            BareDecapsulationKey::new(&sk.to_bytes()),
        ),
    )
}

/// V0 CryptoSystem
pub(crate) struct CryptoSystemVLD0 {
    registry: VeilidComponentRegistry,
}

impl CryptoSystemVLD0 {
    #[must_use]
    pub(crate) fn new(registry: VeilidComponentRegistry) -> Self {
        Self { registry }
    }
}

impl CryptoSystem for CryptoSystemVLD0 {
    // Accessors
    fn kind(&self) -> CryptoKind {
        CRYPTO_KIND_VLD0
    }

    fn crypto(&self) -> VeilidComponentGuard<'_, Crypto> {
        self.registry.lookup::<Crypto>().unwrap_or_log()
    }

    // Cached Operations
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key())))]
    fn cached_dh(&self, key: &PublicKey, secret: &SecretKey) -> VeilidAPIResult<SharedSecret> {
        self.crypto()
            .cached_dh_internal::<CryptoSystemVLD0>(self, key, secret)
    }

    // Generation
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = unsafe { unaligned_u8_vec_uninit(len) };
        random_bytes(bytes.as_mut());
        bytes
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn hash_password(&self, password: &[u8], salt: &[u8]) -> VeilidAPIResult<String> {
        if salt.len() < Salt::MIN_LENGTH || salt.len() > Salt::MAX_LENGTH {
            apibail_generic!("invalid salt length");
        }

        // Argon2 with default params (Argon2id v19)
        let argon2 = Argon2::default();

        // Hash password to PHC string ($argon2id$v=19$...)
        let password_hash = argon2
            .hash_password_with_salt(password, salt)
            .map_err(|e| VeilidAPIError::generic(e.to_string()))?
            .to_string();
        Ok(password_hash)
    }
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn verify_password(&self, password: &[u8], password_hash: &str) -> VeilidAPIResult<bool> {
        let parsed_hash = PasswordHash::new(password_hash)
            .map_err(|e| VeilidAPIError::parse_error(e.to_string(), password_hash))?;
        // Argon2 with default params (Argon2id v19)
        let argon2 = Argon2::default();

        Ok(argon2.verify_password(password, &parsed_hash).is_ok())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn derive_shared_secret(&self, password: &[u8], salt: &[u8]) -> VeilidAPIResult<SharedSecret> {
        if salt.len() < Salt::MIN_LENGTH || salt.len() > Salt::MAX_LENGTH {
            apibail_generic!("invalid salt length");
        }

        // Argon2 with default params (Argon2id v19)
        let argon2 = Argon2::default();

        let mut output_key_material = [0u8; VLD0_SHARED_SECRET_LENGTH];
        argon2
            .hash_password_into(password, salt, &mut output_key_material)
            .map_err(|e| VeilidAPIError::generic(e.to_string()))?;
        Ok(SharedSecret::new(
            CRYPTO_KIND_VLD0,
            BareSharedSecret::new(&output_key_material),
        ))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn random_nonce(&self) -> Nonce {
        let mut nonce = [0u8; VLD0_NONCE_LENGTH];
        random_bytes(&mut nonce);
        Nonce::new(&nonce)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn random_shared_secret(&self) -> SharedSecret {
        let mut s = [0u8; VLD0_SHARED_SECRET_LENGTH];
        random_bytes(&mut s);
        SharedSecret::new(CRYPTO_KIND_VLD0, BareSharedSecret::new(&s))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn compute_dh(&self, key: &PublicKey, secret: &SecretKey) -> VeilidAPIResult<SharedSecret> {
        let pk_xd = public_to_x25519_pk(key)?;
        let sk_xd = secret_to_x25519_sk(secret)?;

        let dh = sk_xd.diffie_hellman(&pk_xd);
        if !dh.was_contributory() {
            apibail_generic!("dh was not contributory");
        }
        let dh_bytes = dh.to_bytes();

        let mut hasher = blake3::Hasher::new();
        hasher.update(VLD0_DOMAIN_CRYPT);
        hasher.update(&dh_bytes);
        let output = hasher.finalize();

        Ok(SharedSecret::new(
            CRYPTO_KIND_VLD0,
            BareSharedSecret::new(output.as_bytes()),
        ))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn hpke_seal(
        &self,
        recipient: &EncapsulationKey,
        aad: &[u8],
        plaintext: &[u8],
    ) -> VeilidAPIResult<Vec<u8>> {
        let (enc, ciphertext) = hpke_seal_core::<::hpke::kem::X25519HkdfSha256>(
            recipient.ref_value(),
            &hpke_info(CRYPTO_KIND_VLD0),
            aad,
            plaintext,
            &mut VeilidRng {},
        )?;
        Ok(build_sealed(CRYPTO_KIND_VLD0, &enc, &ciphertext))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn hpke_open(
        &self,
        secret: &DecapsulationKey,
        aad: &[u8],
        sealed: &[u8],
    ) -> VeilidAPIResult<Vec<u8>> {
        let (enc, ciphertext) = parse_sealed(CRYPTO_KIND_VLD0, VLD0_HPKE_ENC_LENGTH, sealed)?;
        hpke_open_core::<::hpke::kem::X25519HkdfSha256>(
            secret.ref_value(),
            enc,
            &hpke_info(CRYPTO_KIND_VLD0),
            aad,
            ciphertext,
        )
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn generate_keypair(&self) -> KeyPair {
        vld0_generate_keypair()
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn generate_kem_keypair(&self) -> KemKeyPair {
        vld0_generate_kem_keypair()
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn encapsulation_key_from_signing_key(
        &self,
        key: &PublicKey,
    ) -> VeilidAPIResult<EncapsulationKey> {
        let pk_xd = public_to_x25519_pk(key).map_err(|e| {
            VeilidAPIError::invalid_argument(
                "encapsulation_key_from_signing_key",
                "key",
                map_to_string(e),
            )
        })?;
        Ok(EncapsulationKey::new(
            CRYPTO_KIND_VLD0,
            BareEncapsulationKey::new(pk_xd.as_bytes()),
        ))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn decapsulation_key_from_signing_secret(
        &self,
        secret: &SecretKey,
    ) -> VeilidAPIResult<DecapsulationKey> {
        // The sha512 conversion accepts any input length; enforce the ed25519 one
        if secret.ref_value().len() != VLD0_SECRET_KEY_LENGTH {
            apibail_invalid_argument!(
                "decapsulation_key_from_signing_secret",
                "secret",
                secret.ref_value().len()
            );
        }
        let sk_xd = secret_to_x25519_sk(secret).map_err(|e| {
            VeilidAPIError::invalid_argument(
                "decapsulation_key_from_signing_secret",
                "secret",
                map_to_string(e),
            )
        })?;
        Ok(DecapsulationKey::new(
            CRYPTO_KIND_VLD0,
            BareDecapsulationKey::new(&sk_xd.to_bytes()),
        ))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn generate_hash(&self, data: &[u8]) -> HashDigest {
        HashDigest::new(
            CRYPTO_KIND_VLD0,
            BareHashDigest::new(blake3::hash(data).as_bytes()),
        )
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn generate_hash_reader(&self, reader: &mut dyn std::io::Read) -> VeilidAPIResult<PublicKey> {
        let mut hasher = blake3::Hasher::new();
        std::io::copy(reader, &mut hasher).map_err(|e| VeilidAPIError::generic(e.to_string()))?;
        Ok(PublicKey::new(
            CRYPTO_KIND_VLD0,
            BarePublicKey::new(hasher.finalize().as_bytes()),
        ))
    }

    // Validation
    fn shared_secret_length(&self) -> usize {
        VLD0_SHARED_SECRET_LENGTH
    }
    fn nonce_length(&self) -> usize {
        VLD0_NONCE_LENGTH
    }
    fn hash_digest_length(&self) -> usize {
        VLD0_HASH_DIGEST_LENGTH
    }
    fn public_key_length(&self) -> usize {
        VLD0_PUBLIC_KEY_LENGTH
    }
    fn secret_key_length(&self) -> usize {
        VLD0_SECRET_KEY_LENGTH
    }
    fn encapsulation_key_length(&self) -> usize {
        VLD0_ENCAPSULATION_KEY_LENGTH
    }
    fn decapsulation_key_length(&self) -> usize {
        VLD0_DECAPSULATION_KEY_LENGTH
    }
    fn signature_length(&self) -> usize {
        VLD0_SIGNATURE_LENGTH
    }
    fn default_salt_length(&self) -> usize {
        16
    }
    fn aead_overhead(&self) -> usize {
        VLD0_AEAD_OVERHEAD
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn validate_keypair(
        &self,
        public_key: &PublicKey,
        secret_key: &SecretKey,
    ) -> VeilidAPIResult<bool> {
        self.check_public_key(public_key)?;
        self.check_secret_key(secret_key)?;

        let data = vec![0u8; 512];
        let Ok(sig) = self.sign(public_key, secret_key, &data) else {
            return Ok(false);
        };
        let Ok(v) = self.verify(public_key, &data, &sig) else {
            return Ok(false);
        };
        Ok(v)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn validate_hash(&self, data: &[u8], hash_digest: &HashDigest) -> VeilidAPIResult<bool> {
        self.check_hash_digest(hash_digest)?;

        let bytes = *blake3::hash(data).as_bytes();

        Ok(bytes == *hash_digest.ref_value().bytes())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn validate_hash_reader(
        &self,
        reader: &mut dyn std::io::Read,
        hash_digest: &HashDigest,
    ) -> VeilidAPIResult<bool> {
        self.check_hash_digest(hash_digest)?;

        let mut hasher = blake3::Hasher::new();
        std::io::copy(reader, &mut hasher).map_err(|e| VeilidAPIError::generic(e.to_string()))?;
        let bytes = *hasher.finalize().as_bytes();
        Ok(bytes == *hash_digest.ref_value().bytes())
    }

    // Authentication
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn sign(
        &self,
        public_key: &PublicKey,
        secret_key: &SecretKey,
        data: &[u8],
    ) -> VeilidAPIResult<Signature> {
        self.check_public_key(public_key)?;
        self.check_secret_key(secret_key)?;

        let mut kpb = BytesMut::with_capacity(VLD0_SECRET_KEY_LENGTH + VLD0_PUBLIC_KEY_LENGTH);

        kpb.extend_from_slice(secret_key.ref_value().as_ref());
        kpb.extend_from_slice(public_key.ref_value().as_ref());
        let keypair =
            ed::SigningKey::from_keypair_bytes(kpb.as_ref().try_into().unwrap_or_log())
                .map_err(|e| VeilidAPIError::parse_error("Keypair is invalid", e.to_string()))?;

        let mut dig: ed::Sha512 = ed::Sha512::default();
        dig.update(data);

        let sig_bytes = keypair
            .sign_prehashed(dig, Some(VLD0_DOMAIN_SIGN))
            .map_err(VeilidAPIError::internal)?;

        let sig = Signature::new(CRYPTO_KIND_VLD0, BareSignature::new(&sig_bytes.to_bytes()));

        Ok(sig)
    }

    fn sign_in_place(
        &self,
        public_key: &PublicKey,
        secret_key: &SecretKey,
        data: &mut [u8],
        range: Range<usize>,
        sig_idx: usize,
    ) -> VeilidAPIResult<()> {
        self.check_public_key(public_key)?;
        self.check_secret_key(secret_key)?;

        let mut kpb = BytesMut::with_capacity(VLD0_SECRET_KEY_LENGTH + VLD0_PUBLIC_KEY_LENGTH);

        kpb.extend_from_slice(secret_key.ref_value().as_ref());
        kpb.extend_from_slice(public_key.ref_value().as_ref());
        let keypair =
            ed::SigningKey::from_keypair_bytes(kpb.as_ref().try_into().unwrap_or_log())
                .map_err(|e| VeilidAPIError::parse_error("Keypair is invalid", e.to_string()))?;

        let mut dig: ed::Sha512 = ed::Sha512::default();

        dig.update(data.get(range.clone()).ok_or_else(|| {
            VeilidAPIError::invalid_argument("range is out of bounds", "range", range)
        })?);

        let sig_bytes = keypair
            .sign_prehashed(dig, Some(VLD0_DOMAIN_SIGN))
            .map_err(VeilidAPIError::internal)?;

        data.get_mut(sig_idx..sig_idx + VLD0_SIGNATURE_LENGTH)
            .ok_or_else(|| {
                VeilidAPIError::parse_error("signature index is out of bounds", sig_idx)
            })?
            .copy_from_slice(&sig_bytes.to_bytes());

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn verify(
        &self,
        public_key: &PublicKey,
        data: &[u8],
        signature: &Signature,
    ) -> VeilidAPIResult<bool> {
        self.check_public_key(public_key)?;
        self.check_signature(signature)?;

        let pk = ed::VerifyingKey::from_bytes(
            public_key
                .ref_value()
                .as_ref()
                .try_into()
                .map_err(VeilidAPIError::internal)?,
        )
        .map_err(|_| VeilidAPIError::parse_error("Public key is invalid", public_key.clone()))?;
        let sig = ed::Signature::from_bytes(
            signature
                .ref_value()
                .as_ref()
                .try_into()
                .map_err(VeilidAPIError::internal)?,
        );

        let mut dig: ed::Sha512 = ed::Sha512::default();
        dig.update(data);

        if pk
            .verify_prehashed_strict(dig, Some(VLD0_DOMAIN_SIGN), &sig)
            .is_err()
        {
            return Ok(false);
        }
        Ok(true)
    }

    fn verify_in_place(
        &self,
        public_key: &PublicKey,
        data: &[u8],
        range: Range<usize>,
        sig_idx: usize,
    ) -> VeilidAPIResult<bool> {
        self.check_public_key(public_key)?;

        let pk = ed::VerifyingKey::from_bytes(
            public_key
                .ref_value()
                .as_ref()
                .try_into()
                .map_err(VeilidAPIError::internal)?,
        )
        .map_err(|_| VeilidAPIError::parse_error("Public key is invalid", public_key.clone()))?;

        let mut dig: ed::Sha512 = ed::Sha512::default();
        dig.update(
            data.get(range)
                .ok_or_else(|| VeilidAPIError::internal("range is out of bounds"))?,
        );

        let sig = data
            .get(sig_idx..sig_idx + VLD0_SIGNATURE_LENGTH)
            .ok_or_else(|| VeilidAPIError::internal("signature index is out of bounds"))?;
        let sig = ed::Signature::from_bytes(sig.try_into().unwrap_or_log());
        if pk
            .verify_prehashed_strict(dig, Some(VLD0_DOMAIN_SIGN), &sig)
            .is_err()
        {
            return Ok(false);
        }
        Ok(true)
    }

    // AEAD Encrypt/Decrypt
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn decrypt_in_place_aead(
        &self,
        body: &mut dyn CryptoSystemBuffer,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<&[u8]>,
    ) -> VeilidAPIResult<()> {
        self.check_shared_secret(shared_secret)?;

        let shared_secret_bytes: [u8; VLD0_SHARED_SECRET_LENGTH] = shared_secret
            .ref_value()
            .as_ref()
            .try_into()
            .map_err(VeilidAPIError::internal)?;
        let nonce_bytes: [u8; VLD0_NONCE_LENGTH] = nonce
            .as_ref()
            .try_into()
            .map_err(VeilidAPIError::internal)?;

        let key = ch::Key::from(shared_secret_bytes);
        let xnonce = ch::XNonce::from(nonce_bytes);
        let aead = ch::XChaCha20Poly1305::new(&key);
        let mut buffer = BufferWrapper::new(body);
        aead.decrypt_in_place(
            &xnonce,
            associated_data.unwrap_or(b""),
            &mut buffer as &mut dyn ch::aead::Buffer,
        )
        .map_err(map_to_string)
        .map_err(VeilidAPIError::generic)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn decrypt_aead(
        &self,
        body: &[u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<&[u8]>,
    ) -> VeilidAPIResult<Vec<u8>> {
        self.check_nonce(nonce)?;
        self.check_shared_secret(shared_secret)?;

        let mut out = body.to_vec();
        self.decrypt_in_place_aead(&mut out, nonce, shared_secret, associated_data)
            .map_err(map_to_string)
            .map_err(VeilidAPIError::generic)?;
        Ok(out)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn encrypt_in_place_aead(
        &self,
        body: &mut dyn CryptoSystemBuffer,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<&[u8]>,
    ) -> VeilidAPIResult<()> {
        self.check_nonce(nonce)?;
        self.check_shared_secret(shared_secret)?;

        let shared_secret_bytes: [u8; VLD0_SHARED_SECRET_LENGTH] = shared_secret
            .ref_value()
            .as_ref()
            .try_into()
            .map_err(VeilidAPIError::internal)?;
        let nonce_bytes: [u8; VLD0_NONCE_LENGTH] = nonce
            .as_ref()
            .try_into()
            .map_err(VeilidAPIError::internal)?;

        let key = ch::Key::from(shared_secret_bytes);
        let xnonce = ch::XNonce::from(nonce_bytes);
        let aead = ch::XChaCha20Poly1305::new(&key);

        let mut buffer = BufferWrapper::new(body);
        aead.encrypt_in_place(&xnonce, associated_data.unwrap_or(b""), &mut buffer)
            .map_err(map_to_string)
            .map_err(VeilidAPIError::generic)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn encrypt_aead(
        &self,
        body: &[u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<&[u8]>,
    ) -> VeilidAPIResult<Vec<u8>> {
        self.check_nonce(nonce)?;
        self.check_shared_secret(shared_secret)?;

        let mut out = body.to_vec();
        self.encrypt_in_place_aead(&mut out, nonce, shared_secret, associated_data)
            .map_err(map_to_string)
            .map_err(VeilidAPIError::generic)?;
        Ok(out)
    }

    // NoAuth Encrypt/Decrypt
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn crypt_in_place_no_auth(
        &self,
        body: &mut [u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<()> {
        self.check_nonce(nonce)?;
        self.check_shared_secret(shared_secret)?;

        let shared_secret_bytes: [u8; VLD0_SHARED_SECRET_LENGTH] = shared_secret
            .ref_value()
            .as_ref()
            .try_into()
            .map_err(VeilidAPIError::internal)?;
        let nonce_bytes: [u8; VLD0_NONCE_LENGTH] = nonce
            .as_ref()
            .try_into()
            .map_err(VeilidAPIError::internal)?;
        let key = ch::Key::from(shared_secret_bytes);
        let xnonce = ch::XNonce::from(nonce_bytes);

        let mut cipher = <XChaCha20 as KeyIvInit>::new(&key, &xnonce);
        cipher.apply_keystream(body);
        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn crypt_b2b_no_auth(
        &self,
        in_buf: &[u8],
        out_buf: &mut [u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<()> {
        self.check_nonce(nonce)?;
        self.check_shared_secret(shared_secret)?;

        let shared_secret_bytes: [u8; VLD0_SHARED_SECRET_LENGTH] = shared_secret
            .ref_value()
            .as_ref()
            .try_into()
            .map_err(VeilidAPIError::internal)?;
        let nonce_bytes: [u8; VLD0_NONCE_LENGTH] = nonce
            .as_ref()
            .try_into()
            .map_err(VeilidAPIError::internal)?;
        let key = ch::Key::from(shared_secret_bytes);
        let xnonce = ch::XNonce::from(nonce_bytes);

        let mut cipher = <XChaCha20 as KeyIvInit>::new(&key, &xnonce);
        cipher.apply_keystream_b2b(in_buf, out_buf);
        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn crypt_no_auth_aligned_8(
        &self,
        in_buf: &[u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<Vec<u8>> {
        self.check_nonce(nonce)?;
        self.check_shared_secret(shared_secret)?;

        let mut out_buf = unsafe { aligned_8_u8_vec_uninit(in_buf.len()) };
        self.crypt_b2b_no_auth(in_buf, &mut out_buf, nonce, shared_secret)?;
        Ok(out_buf)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "crypto", skip_all, fields(__VEILID_LOG_KEY = self.registry.log_key()))
    )]
    fn crypt_no_auth_unaligned(
        &self,
        in_buf: &[u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<Vec<u8>> {
        self.check_nonce(nonce)?;
        self.check_shared_secret(shared_secret)?;

        let mut out_buf = unsafe { unaligned_u8_vec_uninit(in_buf.len()) };
        self.crypt_b2b_no_auth(in_buf, &mut out_buf, nonce, shared_secret)?;
        Ok(out_buf)
    }
}
