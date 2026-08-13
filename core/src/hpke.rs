//! Forward-secret message encryption (§16.11).
//!
//! # Why HPKE alone was not enough
//!
//! §16.10 shipped its messages in the clear and said so. The obvious fix was
//! HPKE, because that is what VeilidChat moved to — but HPKE base mode encrypts
//! with an ephemeral *sender* key against a **static receiver** key. That gives
//! sender-side forward secrecy and nothing else: seize the receiver's phone,
//! recover one long-term X25519 key, and every message ever sent to them
//! decrypts. For an application whose threat model is §2.2's endpoint
//! compromise, that is the wrong half of the property.
//!
//! Forward secrecy requires the *receiver's* key to be gone. So the receiver
//! publishes short-lived keys, and deletes each one after it is used:
//!
//! - **One-time prekeys.** Used once, deleted on successful decryption. After
//!   that the ciphertext is undecryptable by anyone, including the receiver.
//! - **A signed prekey**, used when the one-time supply runs out, rotated on a
//!   schedule. Messages sealed to it are forward-secret only from the next
//!   rotation.
//!
//! This is X3DH's structure, and it is named rather than reinvented: the
//! exhaustion fallback is Signal's, and so is its known weakness.
//!
//! # What this still does not give
//!
//! **No post-compromise security.** There is no ratchet. An attacker who takes
//! the current prekey state reads everything sealed to those keys until they
//! rotate. A Double Ratchet closes that and is deliberately not attempted here —
//! it changes the message ordering model, and §16.10's per-sender sequences
//! would have to become ratchet state.
//!
//! # Suite
//!
//! RFC 9180 base mode, DHKEM(X25519, HKDF-SHA256) / HKDF-SHA256 /
//! ChaCha20Poly1305 — the RFC's A.2 configuration, chosen so the implementation
//! is checkable against *published* vectors rather than against a second
//! implementation by the same author.
//!
//! This module holds no randomness, matching the rest of `core`: callers supply
//! the CSPRNG, which is also what makes the known-answer tests possible.

use std::collections::BTreeMap;

use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::rand_core::CryptoRng;

/// Re-exported so callers can supply a CSPRNG without depending on `hpke`
/// directly, and so the version stays pinned by this crate.
pub use hpke::rand_core;
use hpke::{Deserializable, Kem as KemTrait, OpModeR, OpModeS, Serializable};

use crate::cbor::Value;
use crate::reject::{Reject, RejectCode};
use crate::sig::ObjectType;
use crate::wire::{f, type_code, Reader};

type Kem = X25519HkdfSha256;
type Kdf = HkdfSha256;
type Aead = ChaCha20Poly1305;

/// An X25519 public key, as it appears on the wire.
pub const PUBKEY_LEN: usize = 32;
/// The KEM's encapsulated key.
pub const ENC_LEN: usize = 32;

/// Derive a keypair from 32 bytes of input keying material.
///
/// Deterministic, so `core` stays free of randomness and the caller decides
/// where entropy comes from — `mobile` draws it from `OsRng`, tests use fixed
/// bytes, and RFC 9180's vectors supply their own.
pub fn derive_keypair(ikm: &[u8]) -> ([u8; 32], [u8; PUBKEY_LEN]) {
    let (sk, pk) = Kem::derive_keypair(ikm);
    (
        sk.to_bytes().as_slice().try_into().unwrap(),
        pk.to_bytes().as_slice().try_into().unwrap(),
    )
}

/// The `info` string binding a ciphertext to this protocol and purpose.
///
/// Same shape as §18.3's signature domain separation, and for the same reason:
/// a ciphertext sealed for one purpose must not open under another, even when
/// the keys are identical.
pub fn message_info(suite: u8) -> Vec<u8> {
    let mut v = Vec::from(&b"DUCAT-v1"[..]);
    v.push(0);
    v.extend_from_slice(b"MESSAGE");
    v.push(0);
    v.push(suite);
    v
}

/// Seal a plaintext to a recipient public key.
///
/// `aad` is authenticated but not encrypted; the caller binds the ciphertext to
/// its conversation with it.
pub fn seal(
    rng: &mut impl CryptoRng,
    recipient_pk: &[u8; PUBKEY_LEN],
    info: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), Reject> {
    let pk = <Kem as KemTrait>::PublicKey::from_bytes(recipient_pk)
        .map_err(|_| Reject::with_detail(RejectCode::Malformed, "recipient key is not on the curve"))?;
    let (enc, mut ctx) =
        hpke::setup_sender_with_rng::<Aead, Kdf, Kem>(&OpModeS::Base, &pk, info, rng)
            // Unreachable with a key that already parsed. §18.5's registry has
            // no internal-error code and should not grow one for a local
            // failure that never travels, so this reuses MALFORMED rather than
            // panicking inside a library.
            .map_err(|_| Reject::with_detail(RejectCode::Malformed, "HPKE sender setup failed"))?;
    let ct = ctx
        .seal(plaintext, aad)
        .map_err(|_| Reject::with_detail(RejectCode::Malformed, "HPKE seal failed"))?;
    Ok((enc.to_bytes().to_vec(), ct))
}

/// Open a ciphertext with the recipient's secret key.
pub fn open(
    recipient_sk: &[u8; 32],
    enc: &[u8],
    info: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, Reject> {
    let sk = <Kem as KemTrait>::PrivateKey::from_bytes(recipient_sk)
        .map_err(|_| Reject::with_detail(RejectCode::Malformed, "bad recipient secret key"))?;
    let encapped = <Kem as KemTrait>::EncappedKey::from_bytes(enc)
        .map_err(|_| Reject::with_detail(RejectCode::Malformed, "bad encapsulated key"))?;
    let mut ctx = hpke::setup_receiver::<Aead, Kdf, Kem>(&OpModeR::Base, &sk, &encapped, info)
        .map_err(|_| Reject::with_detail(RejectCode::BadSig, "HPKE setup failed"))?;
    ctx.open(ciphertext, aad)
        .map_err(|_| Reject::with_detail(RejectCode::BadSig, "ciphertext did not authenticate"))
}

// ---------------------------------------------------------------------------
// Prekeys
// ---------------------------------------------------------------------------

/// Zero is reserved for "the signed prekey", so a one-time key can never be
/// confused with the fallback by an off-by-one.
pub const SIGNED_PREKEY_ID: u32 = 0;

/// One published key a sender may seal to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreKey {
    pub id: u32,
    pub public: [u8; PUBKEY_LEN],
}

/// What a persona publishes to its rendezvous (§16.4) so it can be written to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreKeyBundle {
    pub version: u64,
    pub suite: u8,
    /// Rotated on a schedule. The fallback when one-time keys run out.
    pub signed_prekey: [u8; PUBKEY_LEN],
    /// Consumed one per message. Empty is legal and means degraded secrecy.
    pub one_time: Vec<PreKey>,
    pub expiry: u64,
}

impl PreKeyBundle {
    /// Which key a sender should use, and why it matters which.
    ///
    /// Prefers a one-time key because that is the only one that becomes
    /// unrecoverable after use. Falling back to the signed prekey is a real
    /// weakening, so callers are expected to surface it rather than treat both
    /// outcomes as success.
    pub fn select(&self) -> (PreKey, bool) {
        match self.one_time.first() {
            Some(k) => (k.clone(), true),
            None => (
                PreKey { id: SIGNED_PREKEY_ID, public: self.signed_prekey },
                false,
            ),
        }
    }

    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::PreKeyBundle)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::PKB_SIGNED, Value::Bytes(self.signed_prekey.to_vec()));
        m.insert(
            f::PKB_ONETIME,
            Value::Array(
                self.one_time
                    .iter()
                    .flat_map(|k| [Value::Uint(k.id as u64), Value::Bytes(k.public.to_vec())])
                    .collect(),
            ),
        );
        m.insert(f::PKB_EXPIRY, Value::Uint(self.expiry));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::PreKeyBundle) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not PREKEY_BUNDLE",
            ));
        }
        let version = r.uint(f::VERSION)?;
        let suite = r.uint(f::SUITE)? as u8;
        let signed_prekey: [u8; PUBKEY_LEN] =
            r.bytes(f::PKB_SIGNED, Some(PUBKEY_LEN))?.try_into().unwrap();
        let flat = r.array(f::PKB_ONETIME)?;
        if flat.len() % 2 != 0 {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "prekey list is not id/key pairs",
            ));
        }
        let mut one_time = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for pair in flat.chunks(2) {
            let id = pair[0]
                .as_uint()
                .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "prekey id"))?;
            if id > u32::MAX as u64 {
                return Err(Reject::with_detail(RejectCode::Malformed, "prekey id too large"));
            }
            if id == SIGNED_PREKEY_ID as u64 {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "id 0 is reserved for the signed prekey",
                ));
            }
            // A duplicate id makes "delete after use" ambiguous, which is the
            // one operation the whole property rests on.
            if !seen.insert(id) {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "duplicate prekey id",
                ));
            }
            let pk = pair[1]
                .as_bytes()
                .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "prekey"))?;
            if pk.len() != PUBKEY_LEN {
                return Err(Reject::with_detail(RejectCode::Malformed, "prekey length"));
            }
            one_time.push(PreKey { id: id as u32, public: pk.try_into().unwrap() });
        }
        let expiry = r.uint(f::PKB_EXPIRY)?;
        r.finish()?;
        Ok(PreKeyBundle { version, suite, signed_prekey, one_time, expiry })
    }
}

/// The largest ciphertext a legal §16.10 message can produce: 2000 characters
/// at UTF-8's four-byte worst case, plus CBOR framing and the AEAD tag, rounded
/// up. Anything larger is refused before a key is touched.
pub const MAX_CIPHERTEXT: usize = 8 * 1024;

/// A message as it travels: which key it was sealed to, and the sealed bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedMessage {
    pub version: u64,
    pub suite: u8,
    pub prekey_id: u32,
    pub enc: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

impl SealedMessage {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::SealedMessage)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::SM_PREKEY_ID, Value::Uint(self.prekey_id as u64));
        m.insert(f::SM_ENC, Value::Bytes(self.enc.clone()));
        m.insert(f::SM_CT, Value::Bytes(self.ciphertext.clone()));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::SealedMessage) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not SEALED_MESSAGE",
            ));
        }
        let version = r.uint(f::VERSION)?;
        let suite = r.uint(f::SUITE)? as u8;
        let id = r.uint(f::SM_PREKEY_ID)?;
        if id > u32::MAX as u64 {
            return Err(Reject::with_detail(RejectCode::Malformed, "prekey id too large"));
        }
        let out = SealedMessage {
            version,
            suite,
            prekey_id: id as u32,
            enc: r.bytes(f::SM_ENC, Some(ENC_LEN))?,
            // Bounded so a peer cannot make the receiver allocate arbitrarily
            // before any key is even consulted. §16.10 bounds the plaintext at
            // 2000 characters; UTF-8 worst case is four bytes each, plus the
            // AEAD tag and CBOR framing.
            ciphertext: r.bytes(f::SM_CT, None)?,
        };
        if out.ciphertext.len() > MAX_CIPHERTEXT {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "ciphertext is larger than any legal message",
            ));
        }
        r.finish()?;
        Ok(out)
    }
}

/// The receiver's private prekey material, and the deletion that makes forward
/// secrecy real.
///
/// Deliberately not `Clone`: a copy of this store is a copy of the keys that
/// were supposed to be gone, and the whole property is that they are.
#[derive(Debug)]
pub struct PreKeyStore {
    signed: [u8; 32],
    one_time: BTreeMap<u32, [u8; 32]>,
}

impl PreKeyStore {
    pub fn new(signed: [u8; 32]) -> Self {
        PreKeyStore { signed, one_time: BTreeMap::new() }
    }

    pub fn insert_one_time(&mut self, id: u32, secret: [u8; 32]) {
        debug_assert_ne!(id, SIGNED_PREKEY_ID, "id 0 is the signed prekey");
        self.one_time.insert(id, secret);
    }

    pub fn remaining(&self) -> usize {
        self.one_time.len()
    }

    /// Look up a secret without consuming it.
    fn peek(&self, id: u32) -> Option<[u8; 32]> {
        if id == SIGNED_PREKEY_ID {
            Some(self.signed)
        } else {
            self.one_time.get(&id).copied()
        }
    }

    /// Decrypt, and burn the key if it was a one-time one.
    ///
    /// Deletion happens only on success. Deleting on a failed open would let
    /// anyone who can reach the rendezvous destroy a recipient's prekeys by
    /// sending garbage — a denial of service that also degrades the recipient
    /// to the signed-prekey fallback, which is exactly the weaker state an
    /// attacker wants them in.
    pub fn open_and_consume(
        &mut self,
        sealed: &SealedMessage,
        info: &[u8],
        aad: &[u8],
    ) -> Result<(Vec<u8>, bool), Reject> {
        let sk = self.peek(sealed.prekey_id).ok_or_else(|| {
            Reject::with_detail(RejectCode::StateViolation, "unknown or already-used prekey")
        })?;
        let pt = open(&sk, &sealed.enc, info, aad, &sealed.ciphertext)?;
        let was_one_time = sealed.prekey_id != SIGNED_PREKEY_ID;
        if was_one_time {
            self.one_time.remove(&sealed.prekey_id);
        }
        Ok((pt, was_one_time))
    }
}
