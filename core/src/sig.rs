//! Domain-separated signing, per protocol §18.3.
//!
//! Three failure modes this module exists to make unrepresentable:
//!
//! 1. **Re-encoding.** A verifier that decodes an object, re-encodes it, and
//!    checks the signature over its own encoding will accept input that a
//!    non-canonical sender encoded differently. So `SignedBytes` carries the
//!    bytes as received and verification never re-serializes.
//!
//! 2. **Cross-context replay.** The same keys sign TapPresent, ACCEPT, RECEIPT,
//!    CONTACT_OFFER, bond_proof, and attestations. Without separation, a
//!    signature harvested in one context can be presented as another wherever
//!    the signed byte strings can be made to coincide. Every signature input is
//!    prefixed with a fixed protocol label, the object type, and the suite id.
//!
//! 3. **ECDSA malleability.** See `SecretKey::sign` — this one is specific to
//!    the P-256 suite and has no analogue under Ed25519.

use crate::cbor::{self, CodecError, Value};
use ed25519_dalek as ed;
use p256::ecdsa as ec;
use p256::ecdsa::signature::{Signer as _, Verifier as _};

/// Protocol-wide domain separation prefix (§18.3).
pub const DOMAIN: &[u8] = b"DUCAT-v1";

/// Object type labels. These are part of the signature input, so they are wire
/// constants: changing one invalidates every signature of that type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    TapPresent,
    FullOffer,
    Accept,
    Receipt,
    TxProof,
    Refund,
    Cancel,
    Mandate,
    ContactOffer,
    ContactAccept,
    BondProof,
    Attestation,
    Dispute,
    Ruling,
    Hail,
    HailReply,
    TapStatic,
    /// `fast/1`'s mempool pointer (§17.4).
    ///
    /// Distinct from `TxProof`, and the distinction is the whole point. Draft
    /// 0.17 established that the payee *is* the recipient and can scan with its
    /// own view key, so acceptance needs a transaction identifier rather than a
    /// proof. A proof exists to convince someone who is not the recipient —
    /// which is an arbiter (§17.5), and nobody else.
    TxId,
    EscrowSetup,
    EscrowReady,
    Release,
    SlashClaim,
    /// One message on a persistent contact (§16.10).
    Message,
    /// Published prekeys a sender may seal to (§16.11).
    PreKeyBundle,
    /// A message encrypted to one of them (§16.11).
    SealedMessage,
}

impl ObjectType {
    pub fn label(self) -> &'static [u8] {
        match self {
            ObjectType::TapPresent => b"TapPresent",
            ObjectType::FullOffer => b"FullOffer",
            ObjectType::Accept => b"ACCEPT",
            ObjectType::Receipt => b"RECEIPT",
            ObjectType::TxProof => b"TXPROOF",
            ObjectType::Refund => b"REFUND",
            ObjectType::Cancel => b"CANCEL",
            ObjectType::Mandate => b"MANDATE",
            ObjectType::ContactOffer => b"CONTACT_OFFER",
            ObjectType::ContactAccept => b"CONTACT_ACCEPT",
            ObjectType::BondProof => b"bond_proof",
            ObjectType::Attestation => b"attestation",
            ObjectType::Dispute => b"DISPUTE",
            ObjectType::Ruling => b"RULING",
            ObjectType::Hail => b"HAIL",
            ObjectType::HailReply => b"HAIL_REPLY",
            ObjectType::TapStatic => b"TapStatic",
            ObjectType::TxId => b"TXID",
            ObjectType::EscrowSetup => b"ESCROW_SETUP",
            ObjectType::EscrowReady => b"ESCROW_READY",
            ObjectType::Release => b"RELEASE",
            ObjectType::SlashClaim => b"SLASH_CLAIM",
            ObjectType::Message => b"MESSAGE",
            ObjectType::PreKeyBundle => b"PREKEY_BUNDLE",
            ObjectType::SealedMessage => b"SEALED_MESSAGE",
        }
    }
}

/// Cipher suite identifier (§3, §4.1). Suite 1 is the Ed25519/X25519 default;
/// suite 2 is P-256, required by Core conformance because iOS's Secure Enclave
/// holds no Ed25519 key (§4.1) and personas would otherwise fragment by
/// platform.
///
/// `Ord` is derived so suites can live in sets and maps. It carries **no**
/// preference meaning: suite selection uses the payer's explicit preference
/// list, never the numeric identifier (see `negotiate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Suite {
    Ed25519X25519 = 1,
    P256 = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigError {
    BadSig,
    /// Bytes were not canonical CBOR (§18.1). Checked even when the signature
    /// verifies: a valid signature over non-canonical bytes still breaks every
    /// hash commitment downstream.
    NonCanonical(CodecError),
    /// Key material was malformed.
    BadKey,
    /// A P-256 signature carried a high `s` value. See `SecretKey::sign`.
    MalleableSignature,
    /// The signature was made under a different suite than the key presented.
    SuiteMismatch,
}

/// Build the signature input: a fixed prefix, the object type, and the suite,
/// each terminated by a 0x00 separator so adjacent variable-length fields
/// cannot be re-parsed into different boundaries.
///
/// Without separators, ("AB", "C") and ("A", "BC") would produce identical
/// inputs and a signature over one would verify as the other.
pub fn sig_input(object_type: ObjectType, suite: Suite, canonical_bytes: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(DOMAIN.len() + 32 + canonical_bytes.len());
    v.extend_from_slice(DOMAIN);
    v.push(0x00);
    v.extend_from_slice(object_type.label());
    v.push(0x00);
    v.push(suite as u8);
    v.push(0x00);
    v.extend_from_slice(canonical_bytes);
    v
}

// ------------------------------------------------------------------- keys --

/// A signing key. The suite is a property of the key rather than a separate
/// argument, which makes a key/suite mismatch unrepresentable instead of
/// merely detectable.
pub enum SecretKey {
    Ed25519(ed::SigningKey),
    P256(ec::SigningKey),
}

/// A verifying key, likewise carrying its own suite.
#[derive(Clone)]
pub enum PublicKey {
    Ed25519(ed::VerifyingKey),
    P256(ec::VerifyingKey),
}

impl SecretKey {
    pub fn suite(&self) -> Suite {
        match self {
            SecretKey::Ed25519(_) => Suite::Ed25519X25519,
            SecretKey::P256(_) => Suite::P256,
        }
    }

    pub fn ed25519_from_bytes(b: &[u8; 32]) -> Self {
        SecretKey::Ed25519(ed::SigningKey::from_bytes(b))
    }

    pub fn p256_from_bytes(b: &[u8; 32]) -> Result<Self, SigError> {
        ec::SigningKey::from_slice(b)
            .map(SecretKey::P256)
            .map_err(|_| SigError::BadKey)
    }

    pub fn public(&self) -> PublicKey {
        match self {
            SecretKey::Ed25519(k) => PublicKey::Ed25519(k.verifying_key()),
            SecretKey::P256(k) => PublicKey::P256(*k.verifying_key()),
        }
    }

    /// Sign the domain-separated input, producing 64 bytes for either suite.
    ///
    /// # Why P-256 signatures are normalized here
    ///
    /// ECDSA is malleable: for any valid `(r, s)`, the pair `(r, n - s)` is
    /// also a valid signature over the same message under the same key. Ed25519
    /// has no such property.
    ///
    /// That matters more for this protocol than for most. §6 chains messages by
    /// hash, each carrying the digest of its predecessor, and a completed
    /// transaction is a self-verifying transcript held by the two parties. If a
    /// third party can flip `s` in flight, both parties still see valid
    /// signatures — but they now hold transcripts that hash differently, and
    /// every downstream commitment silently diverges. The same flip would give
    /// a `fast/1` slash claim (§17.5) evidence that verifies yet does not match
    /// the counterparty's copy.
    ///
    /// So signatures are emitted in low-`s` form and high-`s` is refused on
    /// verification. This mirrors what Bitcoin had to do for the same reason.
    pub fn sign(&self, object_type: ObjectType, canonical_bytes: &[u8]) -> [u8; 64] {
        let input = sig_input(object_type, self.suite(), canonical_bytes);
        match self {
            SecretKey::Ed25519(k) => k.sign(&input).to_bytes(),
            SecretKey::P256(k) => {
                let sig: ec::Signature = k.sign(&input);
                // RustCrypto's signer already emits low-s, but normalizing
                // unconditionally means this holds even if that changes, and
                // even for signatures produced by a hardware key we did not
                // control (Secure Enclave makes no such guarantee).
                let sig = sig.normalize_s().unwrap_or(sig);
                sig.to_bytes().into()
            }
        }
    }
}

impl PublicKey {
    pub fn suite(&self) -> Suite {
        match self {
            PublicKey::Ed25519(_) => Suite::Ed25519X25519,
            PublicKey::P256(_) => Suite::P256,
        }
    }

    /// Wire encoding: 32 bytes for Ed25519, 33 for compressed P-256. Length is
    /// implied by the suite, so it is never sent separately.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            PublicKey::Ed25519(k) => k.to_bytes().to_vec(),
            PublicKey::P256(k) => k.to_encoded_point(true).as_bytes().to_vec(),
        }
    }

    pub fn from_bytes(suite: Suite, b: &[u8]) -> Result<Self, SigError> {
        match suite {
            Suite::Ed25519X25519 => {
                let arr: [u8; 32] = b.try_into().map_err(|_| SigError::BadKey)?;
                ed::VerifyingKey::from_bytes(&arr)
                    .map(PublicKey::Ed25519)
                    .map_err(|_| SigError::BadKey)
            }
            Suite::P256 => {
                // SEC1 admits several encodings of the same point: compressed
                // (0x02/0x03, 33 bytes), uncompressed (0x04, 65), and hybrid
                // (0x06/0x07, 65). Worse, the underlying parser is lenient
                // about the tag byte — it reads y-parity from the low bit, so
                // 0x05 is accepted and yields the same key as 0x03.
                //
                // That is the malleability problem one layer up. Public keys
                // appear inside signed objects (a persona in `FullOffer`, a
                // contact card in §16.3), so two encodings of one key means two
                // distinct canonical CBOR objects, two distinct hashes, and a
                // transcript that diverges between the parties while both
                // signatures still verify.
                //
                // Exactly one encoding is therefore legal: compressed, with the
                // tag byte checked explicitly rather than left to the parser.
                if b.len() != 33 || (b[0] != 0x02 && b[0] != 0x03) {
                    return Err(SigError::BadKey);
                }
                ec::VerifyingKey::from_sec1_bytes(b)
                    .map(PublicKey::P256)
                    .map_err(|_| SigError::BadKey)
            }
        }
    }

    fn verify_raw(
        &self,
        object_type: ObjectType,
        canonical_bytes: &[u8],
        sig: &[u8; 64],
    ) -> Result<(), SigError> {
        let input = sig_input(object_type, self.suite(), canonical_bytes);
        match self {
            PublicKey::Ed25519(k) => k
                .verify(&input, &ed::Signature::from_bytes(sig))
                .map_err(|_| SigError::BadSig),
            PublicKey::P256(k) => {
                let s = ec::Signature::from_slice(sig).map_err(|_| SigError::BadSig)?;
                // Refuse the malleable form outright rather than normalizing on
                // receipt: accepting both encodings would mean two distinct
                // byte strings are each "the" signature, and the transcript
                // hash would depend on which one arrived.
                if s.normalize_s().is_some() {
                    return Err(SigError::MalleableSignature);
                }
                k.verify(&input, &s).map_err(|_| SigError::BadSig)
            }
        }
    }
}

// ---------------------------------------------------------- signed objects --

/// An object as it arrived, paired with what it decoded to.
///
/// The bytes are authoritative. `value` is a convenience view, and is only
/// constructible by a decode that already proved the bytes canonical — so
/// holding a `SignedBytes` is evidence that re-encoding `value` reproduces
/// `bytes` exactly.
#[derive(Debug, Clone)]
pub struct SignedBytes {
    bytes: Vec<u8>,
    value: Value,
}

impl SignedBytes {
    /// Accept received bytes, proving canonical form in the process.
    pub fn from_received(bytes: Vec<u8>) -> Result<Self, SigError> {
        let value = cbor::decode(&bytes).map_err(SigError::NonCanonical)?;
        Ok(SignedBytes { bytes, value })
    }

    /// Build from a value we are about to sign ourselves.
    pub fn from_value(value: Value) -> Self {
        let bytes = value.encode();
        SignedBytes { bytes, value }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn sign(&self, object_type: ObjectType, key: &SecretKey) -> [u8; 64] {
        key.sign(object_type, &self.bytes)
    }

    /// Verify against the bytes as received — never against a re-encoding.
    pub fn verify(
        &self,
        object_type: ObjectType,
        key: &PublicKey,
        sig: &[u8; 64],
    ) -> Result<(), SigError> {
        key.verify_raw(object_type, &self.bytes, sig)
    }
}
