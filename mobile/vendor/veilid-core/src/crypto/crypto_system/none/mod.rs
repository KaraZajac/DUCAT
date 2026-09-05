mod blake3digest512;
mod none_kem;
mod sizes;

use super::*;

use super::hpke::{build_sealed, hpke_info, hpke_open_core, hpke_seal_core, parse_sealed};
use argon2::password_hash::phc::Salt;
use blake3digest512::Blake3Digest512;
use data_encoding::BASE64URL_NOPAD;
use digest::rand_core::Rng as _;
use digest::Digest;
use none_kem::NoneKem;
const NONE_AEAD_OVERHEAD: usize = NONE_PUBLIC_KEY_LENGTH;
/// [`CryptoKind`] fourcc identifying the NONE cryptosystem.
pub const CRYPTO_KIND_NONE: CryptoKind = CryptoKind::new(*b"NONE");
/// The NONE fourcc as a big-endian `u32`.
pub const CRYPTO_KIND_NONE_FOURCC: u32 = u32::from_be_bytes(*b"NONE");
pub use sizes::*;

pub(crate) fn none_generate_keypair() -> KeyPair {
    let mut csprng = VeilidRng {};
    let mut pub_bytes = [0u8; NONE_PUBLIC_KEY_LENGTH];
    let mut sec_bytes = [0u8; NONE_SECRET_KEY_LENGTH];
    csprng.fill_bytes(&mut pub_bytes);
    for n in 0..NONE_PUBLIC_KEY_LENGTH {
        sec_bytes[n] = !pub_bytes[n];
    }
    let dht_key = BarePublicKey::new(&pub_bytes);
    let dht_key_secret = BareSecretKey::new(&sec_bytes);
    KeyPair::new(CRYPTO_KIND_NONE, BareKeyPair::new(dht_key, dht_key_secret))
}

pub(crate) fn none_generate_kem_keypair() -> KemKeyPair {
    let mut csprng = VeilidRng {};
    let mut ek_bytes = [0u8; NONE_ENCAPSULATION_KEY_LENGTH];
    let mut dk_bytes = [0u8; NONE_DECAPSULATION_KEY_LENGTH];
    csprng.fill_bytes(&mut ek_bytes);
    for n in 0..NONE_ENCAPSULATION_KEY_LENGTH {
        dk_bytes[n] = !ek_bytes[n];
    }
    KemKeyPair::new(
        CRYPTO_KIND_NONE,
        BareKemKeyPair::new(
            BareEncapsulationKey::new(&ek_bytes),
            BareDecapsulationKey::new(&dk_bytes),
        ),
    )
}

fn do_xor_32(a: &[u8], b: &[u8]) -> VeilidAPIResult<[u8; 32]> {
    if a.len() != 32 || b.len() != 32 {
        apibail_generic!("wrong key length");
    }
    let mut out = [0u8; 32];
    for n in 0..32 {
        out[n] = a[n] ^ b[n];
    }
    Ok(out)
}

fn do_xor_inplace(a: &mut [u8], key: &[u8]) -> VeilidAPIResult<()> {
    if key.len() != 32 {
        apibail_generic!("wrong key length");
    }
    for n in 0..a.len() {
        a[n] ^= key[n % key.len()];
    }

    Ok(())
}

fn do_xor_b2b(a: &[u8], b: &mut [u8], key: &[u8]) -> VeilidAPIResult<()> {
    if key.len() != 32 {
        apibail_generic!("wrong key length");
    }
    if b.len() < a.len() {
        apibail_generic!("output buffer too small");
    }

    for n in 0..a.len() {
        b[n] = a[n] ^ key[n % key.len()];
    }

    Ok(())
}

// Fold associated data into the keyblob so a mismatched aad fails the tag check
fn do_aead_keyblob(
    nonce: &Nonce,
    shared_secret: &SharedSecret,
    associated_data: Option<&[u8]>,
) -> VeilidAPIResult<[u8; 32]> {
    let mut blob = nonce.to_vec();
    blob.extend_from_slice(&[0u8; 8]);
    let mut blob = do_xor_32(&blob, shared_secret.ref_value())?;
    let ad_hash = blake3::hash(associated_data.unwrap_or(b""));
    for n in 0..32 {
        blob[n] ^= ad_hash.as_bytes()[n];
    }
    Ok(blob)
}

fn is_bytes_eq_32(a: &[u8], v: u8) -> VeilidAPIResult<bool> {
    if a.len() != 32 {
        apibail_generic!("wrong key length");
    }

    for n in 0..32 {
        if a[n] != v {
            return Ok(false);
        }
    }
    Ok(true)
}

/// None CryptoSystem
pub(crate) struct CryptoSystemNONE {
    registry: VeilidComponentRegistry,
}

impl CryptoSystemNONE {
    #[must_use]
    pub(crate) fn new(registry: VeilidComponentRegistry) -> Self {
        Self { registry }
    }
}

impl CryptoSystem for CryptoSystemNONE {
    // Accessors
    fn kind(&self) -> CryptoKind {
        CRYPTO_KIND_NONE
    }

    fn crypto(&self) -> VeilidComponentGuard<'_, Crypto> {
        self.registry.lookup::<Crypto>().unwrap_or_log()
    }

    // Cached Operations
    fn cached_dh(&self, key: &PublicKey, secret: &SecretKey) -> VeilidAPIResult<SharedSecret> {
        self.crypto()
            .cached_dh_internal::<CryptoSystemNONE>(self, key, secret)
    }

    // Generation
    fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = unsafe { unaligned_u8_vec_uninit(len) };
        random_bytes(bytes.as_mut());
        bytes
    }
    fn hash_password(&self, password: &[u8], salt: &[u8]) -> VeilidAPIResult<String> {
        if salt.len() < Salt::MIN_LENGTH || salt.len() > Salt::MAX_LENGTH {
            apibail_generic!("invalid salt length");
        }
        Ok(format!(
            "{}:{}",
            BASE64URL_NOPAD.encode(salt),
            BASE64URL_NOPAD.encode(password)
        ))
    }
    fn verify_password(&self, password: &[u8], password_hash: &str) -> VeilidAPIResult<bool> {
        let Some((salt, _)) = password_hash.split_once(":") else {
            apibail_parse_error!("invalid format", password_hash);
        };
        let Ok(salt) = BASE64URL_NOPAD.decode(salt.as_bytes()) else {
            apibail_parse_error!("invalid salt", password_hash);
        };
        Ok(self.hash_password(password, &salt)? == password_hash)
    }

    fn derive_shared_secret(&self, password: &[u8], salt: &[u8]) -> VeilidAPIResult<SharedSecret> {
        if salt.len() < Salt::MIN_LENGTH || salt.len() > Salt::MAX_LENGTH {
            apibail_generic!("invalid salt length");
        }
        Ok(SharedSecret::new(
            CRYPTO_KIND_NONE,
            BareSharedSecret::new(
                blake3::hash(self.hash_password(password, salt)?.as_bytes()).as_bytes(),
            ),
        ))
    }

    fn random_nonce(&self) -> Nonce {
        let mut nonce = [0u8; NONE_NONCE_LENGTH];
        random_bytes(&mut nonce);
        Nonce::new(&nonce)
    }

    fn random_shared_secret(&self) -> SharedSecret {
        let mut s = [0u8; NONE_SHARED_SECRET_LENGTH];
        random_bytes(&mut s);
        SharedSecret::new(CRYPTO_KIND_NONE, BareSharedSecret::new(&s))
    }
    fn compute_dh(&self, key: &PublicKey, secret: &SecretKey) -> VeilidAPIResult<SharedSecret> {
        // Mirror VLD0's low-order-point rejection
        if is_bytes_eq_32(key.ref_value(), 0u8)? {
            apibail_generic!("dh was not contributory");
        }
        let s = do_xor_32(key.ref_value(), secret.ref_value())?;
        Ok(SharedSecret::new(
            CRYPTO_KIND_NONE,
            BareSharedSecret::new(&s),
        ))
    }
    fn hpke_seal(
        &self,
        recipient: &EncapsulationKey,
        aad: &[u8],
        plaintext: &[u8],
    ) -> VeilidAPIResult<Vec<u8>> {
        let (enc, ciphertext) = hpke_seal_core::<NoneKem>(
            recipient.ref_value(),
            &hpke_info(CRYPTO_KIND_NONE),
            aad,
            plaintext,
            &mut VeilidRng {},
        )?;
        Ok(build_sealed(CRYPTO_KIND_NONE, &enc, &ciphertext))
    }

    fn hpke_open(
        &self,
        secret: &DecapsulationKey,
        aad: &[u8],
        sealed: &[u8],
    ) -> VeilidAPIResult<Vec<u8>> {
        let (enc, ciphertext) = parse_sealed(CRYPTO_KIND_NONE, NONE_HPKE_ENC_LENGTH, sealed)?;
        hpke_open_core::<NoneKem>(
            secret.ref_value(),
            enc,
            &hpke_info(CRYPTO_KIND_NONE),
            aad,
            ciphertext,
        )
    }

    fn generate_keypair(&self) -> KeyPair {
        none_generate_keypair()
    }

    fn generate_kem_keypair(&self) -> KemKeyPair {
        none_generate_kem_keypair()
    }

    fn encapsulation_key_from_signing_key(
        &self,
        key: &PublicKey,
    ) -> VeilidAPIResult<EncapsulationKey> {
        // The pk = !sk convention is shared with NoneKem, so the bytes map directly
        if key.ref_value().len() != NONE_PUBLIC_KEY_LENGTH {
            apibail_invalid_argument!(
                "encapsulation_key_from_signing_key",
                "key",
                key.ref_value().len()
            );
        }
        Ok(EncapsulationKey::new(
            CRYPTO_KIND_NONE,
            BareEncapsulationKey::new(key.ref_value()),
        ))
    }

    fn decapsulation_key_from_signing_secret(
        &self,
        secret: &SecretKey,
    ) -> VeilidAPIResult<DecapsulationKey> {
        if secret.ref_value().len() != NONE_SECRET_KEY_LENGTH {
            apibail_invalid_argument!(
                "decapsulation_key_from_signing_secret",
                "secret",
                secret.ref_value().len()
            );
        }
        Ok(DecapsulationKey::new(
            CRYPTO_KIND_NONE,
            BareDecapsulationKey::new(secret.ref_value()),
        ))
    }
    fn generate_hash(&self, data: &[u8]) -> HashDigest {
        HashDigest::new(
            CRYPTO_KIND_NONE,
            BareHashDigest::new(blake3::hash(data).as_bytes()),
        )
    }
    fn generate_hash_reader(&self, reader: &mut dyn std::io::Read) -> VeilidAPIResult<PublicKey> {
        let mut hasher = blake3::Hasher::new();
        std::io::copy(reader, &mut hasher).map_err(|e| VeilidAPIError::generic(e.to_string()))?;
        Ok(PublicKey::new(
            CRYPTO_KIND_NONE,
            BarePublicKey::new(hasher.finalize().as_bytes()),
        ))
    }

    // Validation
    fn default_salt_length(&self) -> usize {
        4
    }
    fn shared_secret_length(&self) -> usize {
        NONE_SHARED_SECRET_LENGTH
    }
    fn nonce_length(&self) -> usize {
        NONE_NONCE_LENGTH
    }
    fn hash_digest_length(&self) -> usize {
        NONE_HASH_DIGEST_LENGTH
    }
    fn aead_overhead(&self) -> usize {
        NONE_AEAD_OVERHEAD
    }
    fn public_key_length(&self) -> usize {
        NONE_PUBLIC_KEY_LENGTH
    }
    fn secret_key_length(&self) -> usize {
        NONE_SECRET_KEY_LENGTH
    }
    fn encapsulation_key_length(&self) -> usize {
        NONE_ENCAPSULATION_KEY_LENGTH
    }
    fn decapsulation_key_length(&self) -> usize {
        NONE_DECAPSULATION_KEY_LENGTH
    }
    fn signature_length(&self) -> usize {
        NONE_SIGNATURE_LENGTH
    }

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
    fn validate_hash(&self, data: &[u8], hash_digest: &HashDigest) -> VeilidAPIResult<bool> {
        self.check_hash_digest(hash_digest)?;
        let out_hash = blake3::hash(data);
        let bytes = out_hash.as_bytes();
        Ok(*bytes == **hash_digest.ref_value())
    }
    fn validate_hash_reader(
        &self,
        reader: &mut dyn std::io::Read,
        hash_digest: &HashDigest,
    ) -> VeilidAPIResult<bool> {
        self.check_hash_digest(hash_digest)?;
        let mut hasher = blake3::Hasher::new();
        std::io::copy(reader, &mut hasher).map_err(|e| VeilidAPIError::generic(e.to_string()))?;
        let out_hash = hasher.finalize();
        let bytes = out_hash.as_bytes();
        Ok(*bytes == **hash_digest.ref_value())
    }

    // Authentication
    fn sign(
        &self,
        public_key: &PublicKey,
        secret_key: &SecretKey,
        data: &[u8],
    ) -> VeilidAPIResult<Signature> {
        self.check_public_key(public_key)?;
        self.check_secret_key(secret_key)?;

        if !is_bytes_eq_32(
            &do_xor_32(public_key.ref_value(), secret_key.ref_value())?,
            0xFFu8,
        )? {
            return Err(VeilidAPIError::parse_error(
                "Keypair is invalid",
                "invalid keys",
            ));
        }

        let mut dig = Blake3Digest512::new();
        dig.update(data);
        let sig = dig.finalize();
        let in_sig_bytes: [u8; NONE_SIGNATURE_LENGTH] = sig.into();
        let mut sig_bytes = [0u8; NONE_SIGNATURE_LENGTH];
        sig_bytes[0..32].copy_from_slice(&in_sig_bytes[0..32]);
        sig_bytes[32..64]
            .copy_from_slice(&do_xor_32(&in_sig_bytes[32..64], secret_key.ref_value())?);
        let none_sig = Signature::new(CRYPTO_KIND_NONE, BareSignature::new(&sig_bytes));
        Ok(none_sig)
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

        let mut dig = Blake3Digest512::new();
        dig.update(data.get(range.clone()).ok_or_else(|| {
            VeilidAPIError::invalid_argument("range is out of bounds", "range", range)
        })?);
        let sig = dig.finalize();
        let in_sig_bytes: [u8; NONE_SIGNATURE_LENGTH] = sig.into();

        let sig_out = data
            .get_mut(sig_idx..sig_idx + NONE_SIGNATURE_LENGTH)
            .ok_or_else(|| {
                VeilidAPIError::parse_error("signature index is out of bounds", sig_idx)
            })?;
        sig_out[0..32].copy_from_slice(&in_sig_bytes[0..32]);
        let xor_sig = do_xor_32(&in_sig_bytes[32..64], secret_key.ref_value())?;
        sig_out[32..64].copy_from_slice(&xor_sig);

        Ok(())
    }

    fn verify(
        &self,
        public_key: &PublicKey,
        data: &[u8],
        signature: &Signature,
    ) -> VeilidAPIResult<bool> {
        self.check_public_key(public_key)?;
        self.check_signature(signature)?;

        let mut dig = Blake3Digest512::new();
        dig.update(data);
        let sig = dig.finalize();
        let in_sig_bytes: [u8; NONE_SIGNATURE_LENGTH] = sig.into();
        let mut verify_bytes = [0u8; NONE_SIGNATURE_LENGTH];
        verify_bytes[0..32].copy_from_slice(&do_xor_32(
            &in_sig_bytes[0..32],
            &signature.ref_value()[0..32],
        )?);
        verify_bytes[32..64].copy_from_slice(&do_xor_32(
            &in_sig_bytes[32..64],
            &signature.ref_value()[32..64],
        )?);

        if !is_bytes_eq_32(&verify_bytes[0..32], 0u8)? {
            return Ok(false);
        }
        if !is_bytes_eq_32(
            &do_xor_32(&verify_bytes[32..64], public_key.ref_value())?,
            0xFFu8,
        )? {
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

        let mut dig = Blake3Digest512::new();
        dig.update(
            data.get(range)
                .ok_or_else(|| VeilidAPIError::internal("range is out of bounds"))?,
        );
        let sig = dig.finalize();
        let in_sig_bytes: [u8; NONE_SIGNATURE_LENGTH] = sig.into();
        let mut verify_bytes = [0u8; NONE_SIGNATURE_LENGTH];
        let signature = data
            .get(sig_idx..sig_idx + NONE_SIGNATURE_LENGTH)
            .ok_or_else(|| VeilidAPIError::internal("signature index is out of bounds"))?;
        verify_bytes[0..32].copy_from_slice(&do_xor_32(&in_sig_bytes[0..32], &signature[0..32])?);
        verify_bytes[32..64]
            .copy_from_slice(&do_xor_32(&in_sig_bytes[32..64], &signature[32..64])?);

        if !is_bytes_eq_32(&verify_bytes[0..32], 0u8)? {
            return Ok(false);
        }
        if !is_bytes_eq_32(
            &do_xor_32(&verify_bytes[32..64], public_key.ref_value())?,
            0xFFu8,
        )? {
            return Ok(false);
        }

        Ok(true)
    }

    // AEAD Encrypt/Decrypt
    fn decrypt_in_place_aead(
        &self,
        body: &mut dyn CryptoSystemBuffer,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<&[u8]>,
    ) -> VeilidAPIResult<()> {
        self.check_nonce(nonce)?;
        self.check_shared_secret(shared_secret)?;

        let blob = do_aead_keyblob(nonce, shared_secret, associated_data)?;

        if body.len() < NONE_AEAD_OVERHEAD {
            return Err(VeilidAPIError::generic("invalid length"));
        }
        if body.as_ref()[body.len() - NONE_AEAD_OVERHEAD..] != blob {
            return Err(VeilidAPIError::generic("invalid keyblob"));
        }
        body.truncate(body.len() - NONE_AEAD_OVERHEAD);
        do_xor_inplace(body.as_mut(), &blob)
    }

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

    fn encrypt_in_place_aead(
        &self,
        body: &mut dyn CryptoSystemBuffer,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<&[u8]>,
    ) -> VeilidAPIResult<()> {
        self.check_nonce(nonce)?;
        self.check_shared_secret(shared_secret)?;

        let blob = do_aead_keyblob(nonce, shared_secret, associated_data)?;
        do_xor_inplace(body.as_mut(), &blob)?;
        body.extend_from_slice(&blob);
        Ok(())
    }

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
    fn crypt_in_place_no_auth(
        &self,
        body: &mut [u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<()> {
        self.check_nonce(nonce)?;
        self.check_shared_secret(shared_secret)?;

        let mut blob = nonce.to_vec();
        blob.extend_from_slice(&[0u8; 8]);
        let blob = do_xor_32(&blob, shared_secret.ref_value())?;
        do_xor_inplace(body, &blob)
    }

    fn crypt_b2b_no_auth(
        &self,
        in_buf: &[u8],
        out_buf: &mut [u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<()> {
        self.check_nonce(nonce)?;
        self.check_shared_secret(shared_secret)?;

        let mut blob = nonce.to_vec();
        blob.extend_from_slice(&[0u8; 8]);
        let blob = do_xor_32(&blob, shared_secret.ref_value())?;
        do_xor_b2b(in_buf, out_buf, &blob)
    }

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
