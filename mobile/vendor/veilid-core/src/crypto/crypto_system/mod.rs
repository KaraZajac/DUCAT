use std::ops::Range;

use super::*;

mod buffer;

pub(crate) use buffer::*;

#[cfg(any(feature = "enable-crypto-vld0", feature = "enable-crypto-none"))]
pub(crate) mod hpke;
#[cfg(feature = "enable-crypto-none")]
pub(crate) mod none;
#[cfg(feature = "enable-crypto-vld0")]
pub(crate) mod vld0;
// #[cfg(feature = "enable-crypto-vld1")]
// pub(crate) mod vld1;

pub(crate) const VEILID_DOMAIN_API: &[u8] = b"VEILID_API";

#[cfg(feature = "enable-crypto-none")]
pub use none::*;
#[cfg(feature = "enable-crypto-vld0")]
pub use vld0::*;
// #[cfg(feature = "enable-crypto-vld1")]
// pub use vld1::*;

/// The set of cryptographic primitives a single cryptosystem provides: key generation, signing
/// and verification, AEAD and unauthenticated encryption, Diffie-Hellman key exchange and shared
/// secret derivation, hashing, password hashing, and random byte generation.
///
/// Each implementation is tagged by a [`CryptoKind`] fourcc; keys, signatures, nonces, and digests
/// carry that kind and are only accepted by the matching cryptosystem. VLD0 is the current
/// implementation.
pub trait CryptoSystem {
    // Accessors
    /// The [`CryptoKind`] fourcc identifying this cryptosystem.
    fn kind(&self) -> CryptoKind;
    /// Component guard for the parent [`Crypto`] registry, used to reach cross-cryptosystem caches.
    fn crypto(&self) -> VeilidComponentGuard<'_, Crypto>;

    // Cached Operations
    /// Diffie-Hellman shared secret for the given public/secret key pair, memoized in the
    /// [`Crypto`] DH cache to avoid recomputing the same exchange. See [`compute_dh`](Self::compute_dh).
    ///
    /// Local CPU only; on a cache miss runs the same scalar multiplication as `compute_dh`. Takes the
    /// `Crypto` inner lock to read/update the cache.
    ///
    /// Errors `VeilidAPIError::Generic` if `key` or `secret` carries the wrong kind or length, plus
    /// the [`compute_dh`](Self::compute_dh) errors on a cache miss.
    fn cached_dh(&self, key: &PublicKey, secret: &SecretKey) -> VeilidAPIResult<SharedSecret>;

    // Generation
    /// Fill a new `Vec` of `len` bytes from the cryptographic RNG.
    fn random_bytes(&self, len: usize) -> Vec<u8>;
    /// Hash a password with the given salt, returning a self-describing PHC hash string suitable
    /// for storage and later [`verify_password`](Self::verify_password).
    ///
    /// CPU-heavy (Argon2); blocks the calling thread for the full KDF. No network or disk.
    ///
    /// Errors `VeilidAPIError::Generic` if `salt` length is outside the Argon2 bounds or the KDF
    /// itself fails, `VeilidAPIError::ParseError` if the salt fails base64 encoding.
    fn hash_password(&self, password: &[u8], salt: &[u8]) -> VeilidAPIResult<String>;
    /// Check a password against a PHC hash string produced by [`hash_password`](Self::hash_password).
    /// Returns `Ok(false)` on mismatch; errors only on a malformed hash string.
    ///
    /// CPU-heavy (Argon2); blocks the calling thread for the full KDF. No network or disk.
    ///
    /// Errors `VeilidAPIError::ParseError` if `password_hash` is not a valid PHC string.
    fn verify_password(&self, password: &[u8], password_hash: &str) -> VeilidAPIResult<bool>;
    /// Derive a shared secret from a password and salt via a password-hashing KDF. Deterministic:
    /// the same password and salt always yield the same secret. Distinct from
    /// [`generate_shared_secret`](Self::generate_shared_secret), which uses key exchange.
    ///
    /// CPU-heavy (Argon2); blocks the calling thread for the full KDF. No network or disk.
    ///
    /// Errors `VeilidAPIError::Generic` if `salt` length is outside the Argon2 bounds or the KDF fails.
    fn derive_shared_secret(&self, password: &[u8], salt: &[u8]) -> VeilidAPIResult<SharedSecret>;
    /// A fresh random nonce of [`nonce_length`](Self::nonce_length) bytes.
    fn random_nonce(&self) -> Nonce;
    /// A fresh random shared secret of [`shared_secret_length`](Self::shared_secret_length) bytes.
    fn random_shared_secret(&self) -> SharedSecret;
    /// Raw Diffie-Hellman shared secret for the given public/secret key pair, with no caching.
    ///
    /// Local CPU only (scalar multiplication); recomputes every call. Use
    /// [`cached_dh`](Self::cached_dh) to memoize repeated exchanges.
    ///
    /// Errors `VeilidAPIError::Internal` if `key` is not a valid curve point, `VeilidAPIError::Generic`
    /// if the exchange is non-contributory (low-order public key).
    fn compute_dh(&self, key: &PublicKey, secret: &SecretKey) -> VeilidAPIResult<SharedSecret>;
    /// Derive a domain-separated shared secret from a key exchange: computes the DH secret, then
    /// hashes it together with `domain` and the Veilid API domain tag. Distinct `domain` values
    /// yield independent secrets from the same key pair.
    ///
    /// Errors with the [`compute_dh`](Self::compute_dh) errors if the key exchange fails.
    fn generate_shared_secret(
        &self,
        key: &PublicKey,
        secret: &SecretKey,
        domain: &[u8],
    ) -> VeilidAPIResult<SharedSecret> {
        let dh = self.compute_dh(key, secret)?;
        let hash = self.generate_hash(&[&dh.into_value(), domain, VEILID_DOMAIN_API].concat());
        Ok(SharedSecret::new(
            hash.kind(),
            BareSharedSecret::new(&hash.into_value()),
        ))
    }
    /// Seal `plaintext` to `recipient` with HPKE base mode (RFC 9180), single-shot. `aad` is
    /// authenticated but not encrypted, and must be supplied again to open. Returns a
    /// self-describing sealed blob: a version byte, this cryptosystem's kind fourcc, the
    /// encapsulated KEM key, and the ciphertext with appended tag.
    ///
    /// Sealing is one-way: only the holder of the recipient's [`DecapsulationKey`] can open the
    /// blob; the sealer cannot decrypt what it just sealed. This differs from the DH pattern,
    /// where the shared secret let the encrypting party decrypt its own blobs. A sealer that
    /// needs to re-read stored blobs must also seal them to its own key. Callers who already
    /// share a symmetric key want [`encrypt_aead`](Self::encrypt_aead) instead; HPKE is for
    /// encrypting to a recipient's key when no shared secret exists.
    ///
    /// Errors `VeilidAPIError::InvalidArgument` if `recipient` is not a valid key,
    /// `VeilidAPIError::Generic` if encapsulation fails (including a low-order key).
    fn hpke_seal(
        &self,
        recipient: &EncapsulationKey,
        aad: &[u8],
        plaintext: &[u8],
    ) -> VeilidAPIResult<Vec<u8>>;
    /// Open a sealed blob produced by [`hpke_seal`](Self::hpke_seal) with the recipient `secret`,
    /// returning the plaintext. `aad` must match what was supplied at seal. Only the recipient
    /// can open a sealed blob; the sealer cannot.
    ///
    /// Errors `VeilidAPIError::ParseError` if the blob is truncated or its version is unknown,
    /// `VeilidAPIError::InvalidArgument` if the blob's kind is not this cryptosystem's kind or
    /// `secret` is not a valid key, `VeilidAPIError::Generic` if decryption fails (tampered blob,
    /// wrong recipient, or mismatched `aad`).
    fn hpke_open(
        &self,
        secret: &DecapsulationKey,
        aad: &[u8],
        sealed: &[u8],
    ) -> VeilidAPIResult<Vec<u8>>;
    /// Generate a fresh random signing key pair for this cryptosystem.
    fn generate_keypair(&self) -> KeyPair;
    /// Generate a fresh random KEM key pair for this cryptosystem.
    fn generate_kem_keypair(&self) -> KemKeyPair;
    /// Derive the KEM encapsulation key corresponding to a signing public key.
    ///
    /// VLD0-only bridge (ed25519 to x25519): kinds whose signing and KEM keys are unrelated
    /// (VLD1 ML-DSA/ML-KEM) error `VeilidAPIError::Unimplemented`.
    ///
    /// Errors `VeilidAPIError::InvalidArgument` if `key` is not a valid signing public key.
    fn encapsulation_key_from_signing_key(
        &self,
        key: &PublicKey,
    ) -> VeilidAPIResult<EncapsulationKey>;
    /// Derive the KEM decapsulation key corresponding to a signing secret key.
    ///
    /// VLD0-only bridge (ed25519 to x25519): kinds whose signing and KEM keys are unrelated
    /// (VLD1 ML-DSA/ML-KEM) error `VeilidAPIError::Unimplemented`.
    ///
    /// Errors `VeilidAPIError::InvalidArgument` if `secret` is not a valid signing secret key.
    fn decapsulation_key_from_signing_secret(
        &self,
        secret: &SecretKey,
    ) -> VeilidAPIResult<DecapsulationKey>;
    /// Hash a byte slice, returning a digest tagged with this cryptosystem's kind.
    fn generate_hash(&self, data: &[u8]) -> HashDigest;
    /// Hash a stream by reading it to end, returning the digest as a `PublicKey` (the digest and
    /// public key share a byte length in this cryptosystem). Errors on read failure.
    ///
    /// Errors `VeilidAPIError::Generic` if reading from `reader` fails.
    fn generate_hash_reader(&self, reader: &mut dyn std::io::Read) -> VeilidAPIResult<PublicKey>;

    // Validation
    /// Byte length of a shared secret.
    fn shared_secret_length(&self) -> usize;
    /// Byte length of a nonce.
    fn nonce_length(&self) -> usize;
    /// Byte length of a hash digest.
    fn hash_digest_length(&self) -> usize;
    /// Byte length of a public key.
    fn public_key_length(&self) -> usize;
    /// Byte length of a secret key.
    fn secret_key_length(&self) -> usize;
    /// Byte length of a KEM encapsulation key.
    fn encapsulation_key_length(&self) -> usize;
    /// Byte length of a KEM decapsulation key.
    fn decapsulation_key_length(&self) -> usize;
    /// Byte length of a signature.
    fn signature_length(&self) -> usize;
    /// Default salt length in bytes for password hashing and KDF operations.
    fn default_salt_length(&self) -> usize;
    /// Bytes an AEAD operation adds to the ciphertext (the authentication tag length).
    fn aead_overhead(&self) -> usize;

    /// Verify a shared secret carries this cryptosystem's kind and the correct length.
    ///
    /// Errors `VeilidAPIError::Generic` if `secret` has the wrong kind or length.
    fn check_shared_secret(&self, secret: &SharedSecret) -> VeilidAPIResult<()> {
        if secret.kind() != self.kind() {
            apibail_generic!("incorrect shared secret kind");
        }
        if secret.value().len() != self.shared_secret_length() {
            apibail_generic!(
                "invalid shared secret length: {} != {}",
                secret.value().len(),
                self.shared_secret_length()
            );
        }
        Ok(())
    }
    /// Verify a nonce has the correct length.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` has the wrong length.
    fn check_nonce(&self, nonce: &Nonce) -> VeilidAPIResult<()> {
        if nonce.len() != self.nonce_length() {
            apibail_generic!(
                "invalid nonce length: {} != {}",
                nonce.len(),
                self.nonce_length()
            );
        }
        Ok(())
    }
    /// Verify a hash digest carries this cryptosystem's kind and the correct length.
    ///
    /// Errors `VeilidAPIError::Generic` if `hash` has the wrong kind or length.
    fn check_hash_digest(&self, hash: &HashDigest) -> VeilidAPIResult<()> {
        if hash.kind() != self.kind() {
            apibail_generic!("incorrect hash digest kind");
        }
        if hash.value().len() != self.hash_digest_length() {
            apibail_generic!(
                "invalid hash digest length: {} != {}",
                hash.value().len(),
                self.hash_digest_length()
            );
        }
        Ok(())
    }
    /// Verify a public key carries this cryptosystem's kind and the correct length.
    ///
    /// Errors `VeilidAPIError::Generic` if `key` has the wrong kind or length.
    fn check_public_key(&self, key: &PublicKey) -> VeilidAPIResult<()> {
        if key.kind() != self.kind() {
            apibail_generic!("incorrect public key kind");
        }
        if key.value().len() != self.public_key_length() {
            apibail_generic!(
                "invalid public key length: {} != {}",
                key.value().len(),
                self.public_key_length()
            );
        }
        Ok(())
    }
    /// Verify a secret key carries this cryptosystem's kind and the correct length.
    ///
    /// Errors `VeilidAPIError::Generic` if `key` has the wrong kind or length.
    fn check_secret_key(&self, key: &SecretKey) -> VeilidAPIResult<()> {
        if key.kind() != self.kind() {
            apibail_generic!("incorrect secret key kind");
        }
        if key.value().len() != self.secret_key_length() {
            apibail_generic!(
                "invalid secret key length: {} != {}",
                key.value().len(),
                self.secret_key_length()
            );
        }
        Ok(())
    }
    /// Verify a signature carries this cryptosystem's kind and the correct length.
    ///
    /// Errors `VeilidAPIError::Generic` if `signature` has the wrong kind or length.
    fn check_signature(&self, signature: &Signature) -> VeilidAPIResult<()> {
        if signature.kind() != self.kind() {
            apibail_generic!("incorrect signature kind");
        }
        if signature.value().len() != self.signature_length() {
            apibail_generic!(
                "invalid signature length: {} != {}",
                signature.value().len(),
                self.signature_length()
            );
        }
        Ok(())
    }
    /// Verify a key pair's kind and that both its public and secret keys have the correct length.
    /// This is a structural check only; it does not verify the keys form a valid pair (see
    /// [`validate_keypair`](Self::validate_keypair)).
    ///
    /// Errors `VeilidAPIError::Generic` if the pair or either key has the wrong kind or length.
    fn check_keypair(&self, keypair: &KeyPair) -> VeilidAPIResult<()> {
        if keypair.kind() != self.kind() {
            apibail_generic!("incorrect keypair kind");
        }
        self.check_public_key(&keypair.key())?;
        self.check_secret_key(&keypair.secret())?;
        Ok(())
    }

    /// Check that a public and secret key form a usable signing pair by signing test data and
    /// verifying it. Returns `Ok(false)` if they do not match; errors only on a malformed key.
    ///
    /// Errors `VeilidAPIError::Generic` if `key` or `secret` has the wrong kind or length.
    fn validate_keypair(&self, key: &PublicKey, secret: &SecretKey) -> VeilidAPIResult<bool>;
    /// Recompute the hash of `data` and compare it against `hash`. Returns `Ok(true)` on match.
    ///
    /// Errors `VeilidAPIError::Generic` if `hash` has the wrong kind or length.
    fn validate_hash(&self, data: &[u8], hash: &HashDigest) -> VeilidAPIResult<bool>;
    /// Hash a stream by reading it to end and compare against `hash`. Returns `Ok(true)` on match;
    /// errors on read failure.
    ///
    /// Errors `VeilidAPIError::Generic` if `hash` has the wrong kind or length, or if reading from
    /// `reader` fails.
    fn validate_hash_reader(
        &self,
        reader: &mut dyn std::io::Read,
        hash: &HashDigest,
    ) -> VeilidAPIResult<bool>;

    // Authentication
    /// Sign `data` with the given key pair, returning a detached signature.
    ///
    /// Errors `VeilidAPIError::Generic` if `public_key` or `secret` has the wrong kind or length,
    /// `VeilidAPIError::ParseError` if they do not form a valid ed25519 keypair,
    /// `VeilidAPIError::Internal` if signing fails.
    fn sign(
        &self,
        public_key: &PublicKey,
        secret: &SecretKey,
        data: &[u8],
    ) -> VeilidAPIResult<Signature>;
    /// Sign the bytes of `data[range]` and write the signature into `data` at `sig_idx`, in place.
    /// Used to sign a buffer and embed its own signature. Errors if `range` or the signature slot
    /// is out of bounds.
    ///
    /// Errors `VeilidAPIError::Generic` if `public_key` or `secret` has the wrong kind or length,
    /// `VeilidAPIError::ParseError` if they do not form a valid ed25519 keypair or `sig_idx` is out
    /// of bounds, `VeilidAPIError::InvalidArgument` if `range` is out of bounds,
    /// `VeilidAPIError::Internal` if signing fails.
    fn sign_in_place(
        &self,
        public_key: &PublicKey,
        secret: &SecretKey,
        data: &mut [u8],
        range: Range<usize>,
        sig_idx: usize,
    ) -> VeilidAPIResult<()>;
    /// Verify a detached `signature` over `data` for `public_key`. Returns `Ok(true)` if valid,
    /// `Ok(false)` if not; errors only on a malformed key or signature.
    ///
    /// Errors `VeilidAPIError::Generic` if `public_key` or `signature` has the wrong kind or length,
    /// `VeilidAPIError::ParseError` if `public_key` is not a valid ed25519 point. A signature that
    /// does not match returns `Ok(false)`, not an error.
    fn verify(
        &self,
        public_key: &PublicKey,
        data: &[u8],
        signature: &Signature,
    ) -> VeilidAPIResult<bool>;
    /// Verify a signature embedded in `data` at `sig_idx` against the bytes of `data[range]`.
    /// The inverse of [`sign_in_place`](Self::sign_in_place). Returns `Ok(true)` if valid.
    ///
    /// Errors `VeilidAPIError::Generic` if `public_key` has the wrong kind or length,
    /// `VeilidAPIError::ParseError` if `public_key` is not a valid ed25519 point,
    /// `VeilidAPIError::Internal` if `range` or `sig_idx` is out of bounds. A signature that does
    /// not match returns `Ok(false)`, not an error.
    fn verify_in_place(
        &self,
        public_key: &PublicKey,
        data: &[u8],
        range: Range<usize>,
        sig_idx: usize,
    ) -> VeilidAPIResult<bool>;

    // AEAD Encrypt/Decrypt
    /// Decrypt and authenticate `body` in place, removing the authentication tag on success.
    /// `associated_data` must match what was supplied at encryption. Errors if authentication
    /// fails (tampered ciphertext, wrong key/nonce, or mismatched associated data).
    ///
    /// Errors `VeilidAPIError::Generic` if `shared_secret` has the wrong kind or length, or if
    /// authentication fails; `VeilidAPIError::Internal` on an internal length conversion failure.
    fn decrypt_in_place_aead(
        &self,
        body: &mut dyn CryptoSystemBuffer,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<&[u8]>,
    ) -> VeilidAPIResult<()>;
    /// Decrypt and authenticate `body`, returning the plaintext. Allocating form of
    /// [`decrypt_in_place_aead`](Self::decrypt_in_place_aead).
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length,
    /// or if authentication fails; `VeilidAPIError::Internal` on an internal length conversion failure.
    fn decrypt_aead(
        &self,
        body: &[u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<&[u8]>,
    ) -> VeilidAPIResult<Vec<u8>>;
    /// Encrypt and authenticate `body` in place, appending the authentication tag. `associated_data`
    /// is authenticated but not encrypted, and must be supplied again at decryption. The same nonce
    /// must never be reused with the same shared secret.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length;
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    fn encrypt_in_place_aead(
        &self,
        body: &mut dyn CryptoSystemBuffer,
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<&[u8]>,
    ) -> VeilidAPIResult<()>;
    /// Encrypt and authenticate `body`, returning the ciphertext with appended tag. Allocating
    /// form of [`encrypt_in_place_aead`](Self::encrypt_in_place_aead).
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length;
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    fn encrypt_aead(
        &self,
        body: &[u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
        associated_data: Option<&[u8]>,
    ) -> VeilidAPIResult<Vec<u8>>;

    // NoAuth Encrypt/Decrypt
    /// Apply the stream cipher to `body` in place, without authentication. Same operation for both
    /// directions: re-applying with the same nonce and secret reverses it. Provides confidentiality
    /// only, no integrity; callers needing tamper detection must use the AEAD variants.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length;
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    fn crypt_in_place_no_auth(
        &self,
        body: &mut [u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<()>;
    /// Apply the stream cipher from `in_buf` into `out_buf` (buffer-to-buffer), without
    /// authentication. `out_buf` must be at least as long as `in_buf`. See
    /// [`crypt_in_place_no_auth`](Self::crypt_in_place_no_auth) for the integrity caveat.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length;
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    fn crypt_b2b_no_auth(
        &self,
        in_buf: &[u8],
        out_buf: &mut [u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<()>;
    /// Stream-cipher `body` into a freshly allocated 8-byte-aligned buffer, without authentication.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length;
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    fn crypt_no_auth_aligned_8(
        &self,
        body: &[u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<Vec<u8>>;
    /// Stream-cipher `body` into a freshly allocated unaligned buffer, without authentication.
    ///
    /// Errors `VeilidAPIError::Generic` if `nonce` or `shared_secret` has the wrong kind or length;
    /// `VeilidAPIError::Internal` on an internal length conversion failure.
    fn crypt_no_auth_unaligned(
        &self,
        body: &[u8],
        nonce: &Nonce,
        shared_secret: &SharedSecret,
    ) -> VeilidAPIResult<Vec<u8>>;
}
