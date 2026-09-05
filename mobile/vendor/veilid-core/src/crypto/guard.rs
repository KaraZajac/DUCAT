use core::marker::PhantomData;
use std::ops::Range;

use super::*;

/// Guard to access a particular cryptosystem
///
/// Holds an `Arc` to the cryptosystem for its whole lifetime; the cryptosystem stays reachable as
/// long as the guard (or its derived [`AsyncCryptoSystemGuard`]) is alive.
#[must_use]
pub struct CryptoSystemGuard<'a> {
    crypto_system: Arc<dyn CryptoSystem + Send + Sync>,
    _phantom: core::marker::PhantomData<&'a (dyn CryptoSystem + Send + Sync)>,
}

impl<'a> CryptoSystemGuard<'a> {
    pub(super) fn new(crypto_system: Arc<dyn CryptoSystem + Send + Sync>) -> Self {
        Self {
            crypto_system,
            _phantom: PhantomData,
        }
    }
    /// Convert into an async guard whose operations yield to the executor between work units.
    ///
    /// Consumes this guard, moving its held cryptosystem `Arc` into the returned async guard.
    pub fn as_async(self) -> AsyncCryptoSystemGuard<'a> {
        AsyncCryptoSystemGuard { guard: self }
    }
    /// Get a clone of the inner Arc for use in blocking tasks
    pub(super) fn clone_arc(&self) -> Arc<dyn CryptoSystem + Send + Sync> {
        self.crypto_system.clone()
    }
}

impl core::ops::Deref for CryptoSystemGuard<'_> {
    type Target = dyn CryptoSystem + Send + Sync;

    fn deref(&self) -> &Self::Target {
        self.crypto_system.as_ref()
    }
}

/// Async cryptosystem guard to help break up heavy blocking operations
#[must_use]
pub struct AsyncCryptoSystemGuard<'a> {
    guard: CryptoSystemGuard<'a>,
}

impl AsyncCryptoSystemGuard<'_> {
    // Accessors

    /// The `CryptoKind` of the guarded cryptosystem.
    pub fn kind(&self) -> CryptoKind {
        self.guard.kind()
    }
    /// Get a guard on the `Crypto` component that owns this cryptosystem.
    #[must_use]
    pub fn crypto(&self) -> VeilidComponentGuard<'_, Crypto> {
        self.guard.crypto()
    }

    // Cached Operations

    /// Diffie-Hellman shared secret, served from the `Crypto` DH cache when present.
    ///
    /// Local CPU only; awaits a single runtime yield. On a cache miss runs the DH inline (does not
    /// offload to the rayon pool, unlike [`compute_dh`](Self::compute_dh)).
    ///
    /// Errors `VeilidAPIError::Generic` if `key` or `secret` carries the wrong kind or length, or
    /// (on a cache miss) `VeilidAPIError::Internal` if `key` is not a valid curve point and
    /// `VeilidAPIError::Generic` if the exchange is non-contributory.
    pub async fn cached_dh(
        &self,
        key: &PublicKey,
        secret: &SecretKey,
    ) -> VeilidAPIResult<SharedSecret> {
        yielding(|| self.guard.cached_dh(key, secret)).await
    }

    // Generation

    /// Generate `len` cryptographically random bytes.
    pub async fn random_bytes(&self, len: usize) -> Bytes {
        yielding(|| self.guard.random_bytes(len).into()).await
    }

    /// Hash a password with the given salt, producing a verifier string.
    ///
    /// CPU-heavy (Argon2); offloaded to the rayon thread pool off-WASM. No network or disk.
    ///
    /// Errors `VeilidAPIError::Generic` if `salt` length is outside the Argon2 bounds or the KDF
    /// fails, `VeilidAPIError::ParseError` if the salt fails base64 encoding.
    pub async fn hash_password(&self, password: Bytes, salt: Bytes) -> VeilidAPIResult<String> {
        let cs = self.guard.clone_arc();
        let salt = salt.to_vec();
        cpu_yielding(move || cs.hash_password(&password, &salt)).await
    }
    /// Verify a password against a hash produced by `hash_password`.
    ///
    /// CPU-heavy (Argon2); offloaded to the rayon thread pool off-WASM. No network or disk.
    ///
    /// Returns `Ok(false)` on mismatch. Errors `VeilidAPIError::ParseError` if `password_hash` is
    /// not a valid PHC string.
    pub async fn verify_password(
        &self,
        password: Bytes,
        password_hash: &str,
    ) -> VeilidAPIResult<bool> {
        let cs = self.guard.clone_arc();
        let password_hash = password_hash.to_string();
        cpu_yielding(move || cs.verify_password(&password, &password_hash)).await
    }
    /// Derive a shared secret deterministically from a password and salt.
    ///
    /// CPU-heavy (Argon2) but run inline before a single yield (not offloaded), so it holds the
    /// thread for the full KDF. No network or disk.
    ///
    /// Errors `VeilidAPIError::Generic` if `salt` length is outside the Argon2 bounds or the KDF fails.
    pub async fn derive_shared_secret(
        &self,
        password: Bytes,
        salt: Bytes,
    ) -> VeilidAPIResult<SharedSecret> {
        yielding(|| self.guard.derive_shared_secret(&password, &salt)).await
    }
    /// Generate a random nonce.
    pub async fn random_nonce(&self) -> Nonce {
        yielding(|| self.guard.random_nonce()).await
    }
    /// Generate a random shared secret.
    pub async fn random_shared_secret(&self) -> SharedSecret {
        yielding(|| self.guard.random_shared_secret()).await
    }
    /// Compute the Diffie-Hellman shared secret for a public key and secret key.
    ///
    /// Local CPU only, offloaded to the rayon thread pool off-WASM; uncached, recomputes every call.
    /// Use [`cached_dh`](Self::cached_dh) to memoize.
    ///
    /// Errors `VeilidAPIError::Internal` if `key` is not a valid curve point, `VeilidAPIError::Generic`
    /// if the exchange is non-contributory (low-order public key).
    pub async fn compute_dh(
        &self,
        key: &PublicKey,
        secret: &SecretKey,
    ) -> VeilidAPIResult<SharedSecret> {
        let cs = self.guard.clone_arc();
        let key = key.clone();
        let secret = secret.clone();
        cpu_yielding(move || cs.compute_dh(&key, &secret)).await
    }
    /// Derive a domain-separated shared secret by hashing the DH result together with `domain` and the Veilid API domain.
    ///
    /// Local CPU only; the DH step is offloaded to the rayon thread pool off-WASM (see
    /// [`compute_dh`](Self::compute_dh)).
    ///
    /// Errors with the [`compute_dh`](Self::compute_dh) errors if the key exchange fails.
    pub async fn generate_shared_secret(
        &self,
        key: &PublicKey,
        secret: &SecretKey,
        domain: Bytes,
    ) -> VeilidAPIResult<SharedSecret> {
        let dh = self.compute_dh(key, secret).await?;
        let data = [
            dh.ref_value().bytes().as_ref(),
            domain.as_ref(),
            VEILID_DOMAIN_API,
        ]
        .concat()
        .into();
        let hash = self.generate_hash(data).await;
        Ok(SharedSecret::new(
            hash.kind(),
            BareSharedSecret::new(&hash.into_value()),
        ))
    }

    /// Seal a plaintext to a recipient KEM encapsulation key with HPKE base mode (RFC 9180),
    /// single-shot. `aad` is authenticated but not encrypted. Returns a self-describing sealed blob.
    ///
    /// Sealing is one-way: only the recipient can open the blob, and the sealer cannot decrypt
    /// what it just sealed, unlike the DH shared-secret pattern. Callers who already share a
    /// symmetric key want [`encrypt_aead`](Self::encrypt_aead) instead.
    ///
    /// Local CPU only, offloaded to the rayon thread pool off-WASM (a KEM encapsulation always runs).
    ///
    /// Errors `VeilidAPIError::InvalidArgument` if `recipient` is not a valid key,
    /// `VeilidAPIError::Generic` if encapsulation fails (including a low-order key).
    pub async fn hpke_seal(
        &self,
        recipient: &EncapsulationKey,
        aad: Bytes,
        plaintext: Bytes,
    ) -> VeilidAPIResult<Bytes> {
        let cs = self.guard.clone_arc();
        let recipient = recipient.clone();
        cpu_yielding(move || Ok(cs.hpke_seal(&recipient, &aad, &plaintext)?.into())).await
    }

    /// Open a sealed blob produced by [`hpke_seal`](Self::hpke_seal) with the recipient KEM
    /// decapsulation key. `aad` must match what was supplied at seal. Only the recipient can
    /// open a sealed blob; the sealer cannot.
    ///
    /// Local CPU only, offloaded to the rayon thread pool off-WASM (a KEM decapsulation always runs).
    ///
    /// Errors `VeilidAPIError::ParseError` if the blob is truncated or its version is unknown,
    /// `VeilidAPIError::InvalidArgument` if the blob's kind is not this cryptosystem's kind or
    /// `secret` is not a valid key, `VeilidAPIError::Generic` if decryption fails (tampered blob,
    /// wrong recipient, or mismatched `aad`).
    pub async fn hpke_open(
        &self,
        secret: &DecapsulationKey,
        aad: Bytes,
        sealed: Bytes,
    ) -> VeilidAPIResult<Bytes> {
        let cs = self.guard.clone_arc();
        let secret = secret.clone();
        cpu_yielding(move || Ok(cs.hpke_open(&secret, &aad, &sealed)?.into())).await
    }

    /// Generate a new keypair.
    pub async fn generate_keypair(&self) -> KeyPair {
        yielding(|| self.guard.generate_keypair()).await
    }

    /// Generate a new KEM key pair.
    pub async fn generate_kem_keypair(&self) -> KemKeyPair {
        yielding(|| self.guard.generate_kem_keypair()).await
    }

    /// Derive the KEM encapsulation key corresponding to a signing public key.
    ///
    /// VLD0-only bridge (ed25519 to x25519); kinds whose signing and KEM keys are unrelated error
    /// `VeilidAPIError::Unimplemented`.
    ///
    /// Errors `VeilidAPIError::InvalidArgument` if `key` is not a valid signing public key.
    pub async fn encapsulation_key_from_signing_key(
        &self,
        key: &PublicKey,
    ) -> VeilidAPIResult<EncapsulationKey> {
        yielding(|| self.guard.encapsulation_key_from_signing_key(key)).await
    }

    /// Derive the KEM decapsulation key corresponding to a signing secret key.
    ///
    /// VLD0-only bridge (ed25519 to x25519); kinds whose signing and KEM keys are unrelated error
    /// `VeilidAPIError::Unimplemented`.
    ///
    /// Errors `VeilidAPIError::InvalidArgument` if `secret` is not a valid signing secret key.
    pub async fn decapsulation_key_from_signing_secret(
        &self,
        secret: &SecretKey,
    ) -> VeilidAPIResult<DecapsulationKey> {
        yielding(|| self.guard.decapsulation_key_from_signing_secret(secret)).await
    }

    /// Hash a byte buffer.
    pub async fn generate_hash(&self, data: Bytes) -> HashDigest {
        yielding(|| self.guard.generate_hash(&data)).await
    }

    /// Hash the entire contents of a reader.
    ///
    /// Errors `VeilidAPIError::Generic` if reading from `reader` fails.
    pub async fn generate_hash_reader(
        &self,
        reader: &mut dyn std::io::Read,
    ) -> VeilidAPIResult<PublicKey> {
        yielding(|| self.guard.generate_hash_reader(reader)).await
    }

    // Validation

    /// Length in bytes of a shared secret.
    #[must_use]
    pub fn shared_secret_length(&self) -> usize {
        self.guard.shared_secret_length()
    }
    /// Length in bytes of a nonce.
    #[must_use]
    pub fn nonce_length(&self) -> usize {
        self.guard.nonce_length()
    }
    /// Length in bytes of a hash digest.
    #[must_use]
    pub fn hash_digest_length(&self) -> usize {
        self.guard.hash_digest_length()
    }
    /// Length in bytes of a public key.
    #[must_use]
    pub fn public_key_length(&self) -> usize {
        self.guard.public_key_length()
    }
    /// Length in bytes of a secret key.
    #[must_use]
    pub fn secret_key_length(&self) -> usize {
        self.guard.secret_key_length()
    }
    /// Length in bytes of a KEM encapsulation key.
    #[must_use]
    pub fn encapsulation_key_length(&self) -> usize {
        self.guard.encapsulation_key_length()
    }
    /// Length in bytes of a KEM decapsulation key.
    #[must_use]
    pub fn decapsulation_key_length(&self) -> usize {
        self.guard.decapsulation_key_length()
    }
    /// Length in bytes of a signature.
    #[must_use]
    pub fn signature_length(&self) -> usize {
        self.guard.signature_length()
    }
    /// Number of extra bytes an AEAD operation adds to the ciphertext.
    #[must_use]
    pub fn aead_overhead(&self) -> usize {
        self.guard.aead_overhead()
    }
    /// Default salt length in bytes for password hashing.
    #[must_use]
    pub fn default_salt_length(&self) -> usize {
        self.guard.default_salt_length()
    }
    /// Validate that a shared secret is well-formed for this cryptosystem.
    ///
    /// Errors `VeilidAPIError::Generic` if `secret` has the wrong kind or length.
    pub fn check_shared_secret(&self, secret: &SharedSecret) -> VeilidAPIResult<()> {
        self.guard.check_shared_secret(secret)
    }
    /// Validate that a nonce is well-formed for this cryptosystem.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` has the wrong length.
    pub fn check_nonce(&self, nonce: &Nonce) -> VeilidAPIResult<()> {
        self.guard.check_nonce(nonce)
    }
    /// Validate that a hash digest is well-formed for this cryptosystem.
    ///
    /// Errors `VeilidAPIError::Generic` if `hash` has the wrong kind or length.
    pub fn check_hash_digest(&self, hash: &HashDigest) -> VeilidAPIResult<()> {
        self.guard.check_hash_digest(hash)
    }
    /// Validate that a public key is well-formed for this cryptosystem.
    ///
    /// Errors `VeilidAPIError::Generic` if `key` has the wrong kind or length.
    pub fn check_public_key(&self, key: &PublicKey) -> VeilidAPIResult<()> {
        self.guard.check_public_key(key)
    }
    /// Validate that a secret key is well-formed for this cryptosystem.
    ///
    /// Errors `VeilidAPIError::Generic` if `key` has the wrong kind or length.
    pub fn check_secret_key(&self, key: &SecretKey) -> VeilidAPIResult<()> {
        self.guard.check_secret_key(key)
    }
    /// Validate that a signature is well-formed for this cryptosystem.
    ///
    /// Errors `VeilidAPIError::Generic` if `signature` has the wrong kind or length.
    pub fn check_signature(&self, signature: &Signature) -> VeilidAPIResult<()> {
        self.guard.check_signature(signature)
    }
    /// Validate that a keypair is well-formed for this cryptosystem. Structural check only; see
    /// [`validate_keypair`](Self::validate_keypair).
    ///
    /// Errors `VeilidAPIError::Generic` if the pair or either key has the wrong kind or length.
    pub fn check_keypair(&self, keypair: &KeyPair) -> VeilidAPIResult<()> {
        self.guard.check_keypair(keypair)
    }
    /// Check that a public key and secret key form a valid keypair.
    ///
    /// Returns `Ok(false)` if they do not match. Errors `VeilidAPIError::Generic` if `key` or
    /// `secret` has the wrong kind or length.
    pub async fn validate_keypair(
        &self,
        key: &PublicKey,
        secret: &SecretKey,
    ) -> VeilidAPIResult<bool> {
        yielding(|| self.guard.validate_keypair(key, secret)).await
    }

    /// Check that a buffer hashes to the given digest.
    ///
    /// Errors `VeilidAPIError::Generic` if `hash` has the wrong kind or length.
    pub async fn validate_hash(&self, data: Bytes, hash: &HashDigest) -> VeilidAPIResult<bool> {
        yielding(|| self.guard.validate_hash(&data, hash)).await
    }

    /// Check that a reader's contents hash to the given digest.
    ///
    /// Errors `VeilidAPIError::Generic` if `hash` has the wrong kind or length, or if reading from
    /// `reader` fails.
    pub async fn validate_hash_reader(
        &self,
        reader: &mut dyn std::io::Read,
        hash: &HashDigest,
    ) -> VeilidAPIResult<bool> {
        yielding(|| self.guard.validate_hash_reader(reader, hash)).await
    }

    // Authentication

    /// Sign a buffer with a keypair, returning a detached signature.
    ///
    /// Local CPU only, offloaded to the rayon thread pool off-WASM.
    ///
    /// Errors `VeilidAPIError::Generic` if `public_key` or `secret` has the wrong kind or length,
    /// `VeilidAPIError::ParseError` if they do not form a valid ed25519 keypair,
    /// `VeilidAPIError::Internal` if signing fails.
    pub async fn sign(
        &self,
        public_key: &PublicKey,
        secret: &SecretKey,
        data: Bytes,
    ) -> VeilidAPIResult<Signature> {
        let cs = self.guard.clone_arc();
        let public_key = public_key.clone();
        let secret = secret.clone();
        cpu_yielding(move || cs.sign(&public_key, &secret, &data)).await
    }

    /// Sign the bytes in `range` and write the signature into `data` at `sig_idx`, returning the buffer.
    ///
    /// Local CPU only, offloaded to the rayon thread pool off-WASM.
    ///
    /// Errors `VeilidAPIError::Generic` if `public_key` or `secret` has the wrong kind or length,
    /// `VeilidAPIError::ParseError` if they do not form a valid ed25519 keypair or `sig_idx` is out
    /// of bounds, `VeilidAPIError::InvalidArgument` if `range` is out of bounds,
    /// `VeilidAPIError::Internal` if signing fails.
    pub async fn sign_in_place(
        &self,
        public_key: &PublicKey,
        secret: &SecretKey,
        mut data: BytesMut,
        range: Range<usize>,
        sig_idx: usize,
    ) -> VeilidAPIResult<BytesMut> {
        let cs = self.guard.clone_arc();
        let public_key = public_key.clone();
        let secret = secret.clone();
        cpu_yielding(move || {
            cs.sign_in_place(&public_key, &secret, &mut data, range, sig_idx)?;
            Ok(data)
        })
        .await
    }

    /// Verify a detached signature over a buffer against a public key.
    ///
    /// Local CPU only, offloaded to the rayon thread pool off-WASM.
    ///
    /// Returns `Ok(false)` if the signature does not match. Errors `VeilidAPIError::Generic` if
    /// `public_key` or `signature` has the wrong kind or length, `VeilidAPIError::ParseError` if
    /// `public_key` is not a valid ed25519 point.
    pub async fn verify(
        &self,
        public_key: &PublicKey,
        data: Bytes,
        signature: &Signature,
    ) -> VeilidAPIResult<bool> {
        let cs = self.guard.clone_arc();
        let public_key = public_key.clone();
        let signature = signature.clone();
        cpu_yielding(move || cs.verify(&public_key, &data, &signature)).await
    }

    /// Verify the signature at `sig_idx` over the bytes in `range` of `data` against a public key.
    ///
    /// Local CPU only, offloaded to the rayon thread pool off-WASM.
    ///
    /// Returns `Ok(false)` if the signature does not match. Errors `VeilidAPIError::Generic` if
    /// `public_key` has the wrong kind or length, `VeilidAPIError::ParseError` if `public_key` is
    /// not a valid ed25519 point, `VeilidAPIError::Internal` if `range` or `sig_idx` is out of bounds.
    pub async fn verify_in_place(
        &self,
        public_key: &PublicKey,
        data: Bytes,
        range: Range<usize>,
        sig_idx: usize,
    ) -> VeilidAPIResult<bool> {
        let cs = self.guard.clone_arc();
        let public_key = public_key.clone();
        cpu_yielding(move || cs.verify_in_place(&public_key, &data, range, sig_idx)).await
    }

    // AEAD Encrypt/Decrypt

    /// Decrypt and authenticate an AEAD ciphertext into a new buffer.
    ///
    /// Local CPU only; offloaded to the rayon thread pool off-WASM once the buffer exceeds the scaling
    /// threshold, else run inline.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length,
    /// or if authentication fails (tampered ciphertext, wrong key/nonce, or mismatched
    /// `associated_data`); `VeilidAPIError::Internal` on an internal length conversion failure.
    pub async fn decrypt_aead(
        &self,
        body: Bytes,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<Bytes>,
    ) -> VeilidAPIResult<Bytes> {
        let cs = self.guard.clone_arc();
        let nonce = nonce.clone();
        let shared_secret = shared_secret.clone();
        scaled_yielding(body.len(), 1024, 8192, move || {
            Ok(cs
                .decrypt_aead(&body, &nonce, &shared_secret, associated_data.as_deref())?
                .into())
        })
        .await
    }
    /// Decrypt and authenticate an AEAD ciphertext in place, returning the truncated plaintext buffer.
    ///
    /// Local CPU only; offloaded to the rayon thread pool off-WASM for large buffers, else run inline.
    ///
    /// Errors `VeilidAPIError::Generic` if `shared_secret` has the wrong kind or length, or if
    /// authentication fails (tampered ciphertext, wrong key/nonce, or mismatched `associated_data`);
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    pub async fn decrypt_in_place_aead(
        &self,
        mut body: BytesMut,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<Bytes>,
    ) -> VeilidAPIResult<BytesMut> {
        let cs = self.guard.clone_arc();
        let nonce = nonce.clone();
        let shared_secret = shared_secret.clone();
        scaled_yielding(body.len(), 1024, 8192, move || {
            cs.decrypt_in_place_aead(
                &mut body,
                &nonce,
                &shared_secret,
                associated_data.as_deref(),
            )?;

            Ok(body)
        })
        .await
    }

    /// Encrypt and authenticate a buffer with AEAD into a new ciphertext.
    ///
    /// Local CPU only; offloaded to the rayon thread pool off-WASM for large buffers, else run inline.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length;
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    pub async fn encrypt_aead(
        &self,
        body: Bytes,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<Bytes>,
    ) -> VeilidAPIResult<Bytes> {
        let cs = self.guard.clone_arc();
        let nonce = nonce.clone();
        let shared_secret = shared_secret.clone();
        scaled_yielding(body.len(), 1024, 8192, move || {
            Ok(cs
                .encrypt_aead(&body, &nonce, &shared_secret, associated_data.as_deref())?
                .into())
        })
        .await
    }

    /// Encrypt and authenticate a buffer with AEAD in place, appending the authentication tag.
    ///
    /// Local CPU only; offloaded to the rayon thread pool off-WASM for large buffers, else run inline.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length;
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    pub async fn encrypt_in_place_aead(
        &self,
        mut body: BytesMut,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<Bytes>,
    ) -> VeilidAPIResult<BytesMut> {
        let cs = self.guard.clone_arc();
        let nonce = nonce.clone();
        let shared_secret = shared_secret.clone();
        scaled_yielding(body.len(), 1024, 8192, move || {
            cs.encrypt_in_place_aead(
                &mut body,
                &nonce,
                &shared_secret,
                associated_data.as_deref(),
            )?;

            Ok(body)
        })
        .await
    }

    // NoAuth Encrypt/Decrypt

    /// Unauthenticated buffer-to-buffer crypt: transform `in_buf` into `out_buf` starting at `out_idx`.
    ///
    /// Local CPU only; offloaded to the rayon thread pool off-WASM for large buffers, else run inline.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length;
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    pub async fn crypt_b2b_no_auth(
        &self,
        in_buf: Bytes,
        mut out_buf: BytesMut,
        out_idx: usize,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<BytesMut> {
        let cs = self.guard.clone_arc();
        let nonce = nonce.clone();
        let shared_secret = shared_secret.clone();
        scaled_yielding(in_buf.len(), 1024, 8192, move || {
            cs.crypt_b2b_no_auth(
                &in_buf,
                &mut out_buf[out_idx..out_idx + in_buf.len()],
                &nonce,
                &shared_secret,
            )?;
            Ok(out_buf)
        })
        .await
    }

    /// Unauthenticated in-place crypt of the bytes in `range`.
    ///
    /// Local CPU only; offloaded to the rayon thread pool off-WASM for large buffers, else run inline.
    ///
    /// Errors `VeilidAPIError::Internal` if `range` is out of bounds, `VeilidAPIError::Generic` if
    /// `nonce` or `shared_secret` has the wrong kind or length.
    pub async fn crypt_in_place_no_auth(
        &self,
        mut body: BytesMut,
        range: Range<usize>,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<BytesMut> {
        let cs = self.guard.clone_arc();
        let nonce = nonce.clone();
        let shared_secret = shared_secret.clone();
        scaled_yielding(body.len(), 1024, 8192, move || {
            cs.crypt_in_place_no_auth(
                body.as_mut()
                    .get_mut(range)
                    .ok_or_else(|| VeilidAPIError::internal("range is out of bounds"))?,
                &nonce,
                &shared_secret,
            )?;
            Ok(body)
        })
        .await
    }

    /// Unauthenticated crypt into a fresh 8-byte-aligned output buffer.
    ///
    /// Local CPU only; offloaded to the rayon thread pool off-WASM for large buffers, else run inline.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length;
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    pub async fn crypt_no_auth_aligned_8(
        &self,
        body: Bytes,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<Vec<u8>> {
        let cs = self.guard.clone_arc();
        let nonce = nonce.clone();
        let shared_secret = shared_secret.clone();
        scaled_yielding(body.len(), 1024, 8192, move || {
            cs.crypt_no_auth_aligned_8(&body, &nonce, &shared_secret)
        })
        .await
    }

    /// Unauthenticated crypt into a fresh unaligned output buffer.
    ///
    /// Local CPU only; offloaded to the rayon thread pool off-WASM for large buffers, else run inline.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length;
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    pub async fn crypt_no_auth_unaligned(
        &self,
        body: Bytes,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<Vec<u8>> {
        let cs = self.guard.clone_arc();
        let nonce = nonce.clone();
        let shared_secret = shared_secret.clone();
        scaled_yielding(body.len(), 1024, 8192, move || {
            cs.crypt_no_auth_unaligned(&body, &nonce, &shared_secret)
        })
        .await
    }
}
