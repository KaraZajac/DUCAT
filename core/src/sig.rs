//! Domain-separated signing, per protocol §18.3.
//!
//! Two failure modes this module exists to make unrepresentable:
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

use crate::cbor::{self, CodecError, Value};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

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
        }
    }
}

/// Cipher suite identifier (§3, §4.1). Suite 1 is the Ed25519/X25519 default;
/// suite 2 is P-256, required by Core conformance because iOS's Secure Enclave
/// holds no Ed25519 key (§4.1) and personas would otherwise fragment by
/// platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Suite {
    Ed25519X25519 = 1,
    P256 = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigError {
    /// Signature did not verify.
    BadSig,
    /// Bytes were not canonical CBOR (§18.1). Checked even when the signature
    /// verifies: a valid signature over non-canonical bytes still breaks every
    /// hash commitment downstream.
    NonCanonical(CodecError),
    /// Suite is not implemented by this client.
    UnsupportedSuite(u8),
    /// Key material was malformed.
    BadKey,
}

/// Build the signature input for an object: a fixed prefix, the object type,
/// and the suite, each terminated by a 0x00 separator so that adjacent
/// variable-length fields cannot be re-parsed into different boundaries.
///
/// Concretely, without separators, ("AB", "C") and ("A", "BC") would produce
/// identical inputs and a signature over one would verify as the other.
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

    /// Sign over the domain-separated input. Suite 1 only for now; suite 2
    /// (P-256) is required for Core conformance and is not yet implemented.
    pub fn sign(
        &self,
        object_type: ObjectType,
        suite: Suite,
        key: &SigningKey,
    ) -> Result<[u8; 64], SigError> {
        if suite != Suite::Ed25519X25519 {
            return Err(SigError::UnsupportedSuite(suite as u8));
        }
        let input = sig_input(object_type, suite, &self.bytes);
        Ok(key.sign(&input).to_bytes())
    }

    /// Verify against the bytes as received — never against a re-encoding.
    pub fn verify(
        &self,
        object_type: ObjectType,
        suite: Suite,
        pubkey: &VerifyingKey,
        sig: &[u8; 64],
    ) -> Result<(), SigError> {
        if suite != Suite::Ed25519X25519 {
            return Err(SigError::UnsupportedSuite(suite as u8));
        }
        let input = sig_input(object_type, suite, &self.bytes);
        pubkey
            .verify(&input, &Signature::from_bytes(sig))
            .map_err(|_| SigError::BadSig)
    }
}
