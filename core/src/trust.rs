//! §9.5 — costly identity, and §9.2 — the rated receipt.
//!
//! Two objects, both carried as §18.3 envelopes under the persona named inside
//! them, the way a contact inbox's halves are (§16.9):
//!
//! - [`BurnProof`]: a persona's proof that it sacrificed XMR. The proof
//!   itself is Monero's `OutProofV2`, made by the sender from the transaction's
//!   secret key, whose *message* names the persona and a purpose. This object
//!   is the shape it travels in; the arithmetic — does the proof verify against
//!   the burn address for that amount, is the transaction in a block on two
//!   nodes — is the reader's, against the chain, and lives with the wallet.
//! - [`Attestation`]: a rated `RECEIPT` signed to a persona after a settled
//!   deal, weighted by the reader according to the signer's own burn or bond.
//!
//! Neither is money. Both are what a stranger shows before money moves.

use std::collections::BTreeMap;

use crate::cbor::{decode, Value};
use crate::reject::{Reject, RejectCode};
use crate::sig::{ObjectType, PublicKey, SecretKey, SignedBytes, Suite};
use crate::wire::{f, open, peek_body, seal, type_code, Reader};

/// A burn under this counts for nothing. 0.01 XMR — about the price of a
/// coffee — so that the smallest useful burn is still a cost. A client
/// policy rather than a wire rule: it can move without a draft.
pub const BURN_FLOOR_PXMR: u64 = 10_000_000_000;

/// The domain string in every burn proof's message, and in the burn
/// address's derivation (§9.5).
pub const BURN_DOMAIN: &[u8] = b"DUCAT-BURN-v1";

/// The header every Monero out-proof of this version starts with.
pub const OUT_PROOF_HEADER: &str = "OutProofV2";
/// A shared secret (32 bytes) and a signature (64 bytes) in Monero's base58:
/// 44 and 88 characters, one pair per transaction key.
const OUT_PROOF_PAIR_CHARS: usize = 44 + 88;
/// A transaction has one key and at most one additional key per output; a
/// proof longer than this is not a proof of one transaction.
pub const MAX_OUT_PROOF_CHARS: usize = OUT_PROOF_HEADER.len() + 17 * OUT_PROOF_PAIR_CHARS;
/// A purpose is a label: "identity", "listing", "arbiter".
pub const MAX_PURPOSE_CHARS: usize = 32;

/// The message a burn's `OutProofV2` signs: the domain, the persona, the
/// purpose, separated by zero bytes. What makes it *this persona's* burn.
pub fn burn_message(persona: &[u8], purpose: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(BURN_DOMAIN.len() + 2 + persona.len() + purpose.len());
    v.extend_from_slice(BURN_DOMAIN);
    v.push(0);
    v.extend_from_slice(persona);
    v.push(0);
    v.extend_from_slice(purpose.as_bytes());
    v
}

/// The version every `BURN_PROOF` written today carries.
pub const BURN_PROOF_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BurnProof {
    pub version: u64,
    pub suite: u8,
    /// The transaction that paid the burn address.
    pub txid: [u8; 32],
    /// What the proof proves was paid, in pXMR. The reader believes the
    /// proof's arithmetic, never this field on its own.
    pub amount_pxmr: u64,
    /// The block it is in — the persona's birth certificate, since a block
    /// hash cannot be backdated.
    pub height: u64,
    /// Monero's `OutProofV2` string.
    pub proof: String,
    /// The persona the proof's message names, and the envelope's signer.
    pub persona: Vec<u8>,
    /// The purpose in the message.
    pub purpose: String,
}

/// The shape of an `OutProofV2` string, checked before anything is asked of
/// a node: the header, then whole base58 pairs, at least one.
fn out_proof_shape_ok(p: &str) -> bool {
    if !p.starts_with(OUT_PROOF_HEADER) || p.len() > MAX_OUT_PROOF_CHARS {
        return false;
    }
    let rest = &p[OUT_PROOF_HEADER.len()..];
    !rest.is_empty()
        && rest.len() % OUT_PROOF_PAIR_CHARS == 0
        && rest.bytes().all(|b| b.is_ascii_alphanumeric())
}

impl BurnProof {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::BurnProof)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::BP_TXID, Value::Bytes(self.txid.to_vec()));
        m.insert(f::BP_AMOUNT, Value::Uint(self.amount_pxmr));
        m.insert(f::BP_HEIGHT, Value::Uint(self.height));
        m.insert(f::BP_PROOF, Value::Text(self.proof.clone()));
        m.insert(f::BP_PERSONA, Value::Bytes(self.persona.clone()));
        m.insert(f::BP_PURPOSE, Value::Text(self.purpose.clone()));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::BurnProof) {
            return Err(Reject::with_detail(RejectCode::Malformed, "object type is not BURN_PROOF"));
        }
        let version = r.uint(f::VERSION)?;
        if version != BURN_PROOF_VERSION {
            return Err(Reject::with_detail(RejectCode::Malformed, "burn proof version is not 1"));
        }
        let out = BurnProof {
            version,
            suite: r.uint(f::SUITE)? as u8,
            txid: r.bytes(f::BP_TXID, Some(32))?.try_into().unwrap(),
            amount_pxmr: r.uint(f::BP_AMOUNT)?,
            height: r.uint(f::BP_HEIGHT)?,
            proof: r
                .opt_text(f::BP_PROOF, MAX_OUT_PROOF_CHARS)?
                .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "no proof"))?,
            persona: r.bytes(f::BP_PERSONA, Some(32))?,
            purpose: r
                .opt_text(f::BP_PURPOSE, MAX_PURPOSE_CHARS)?
                .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "no purpose"))?,
        };
        r.finish()?;
        if out.amount_pxmr == 0 {
            return Err(Reject::with_detail(RejectCode::Malformed, "a burn of nothing"));
        }
        if out.height == 0 {
            return Err(Reject::with_detail(RejectCode::Malformed, "a burn needs a block"));
        }
        if !out_proof_shape_ok(&out.proof) {
            return Err(Reject::with_detail(RejectCode::Malformed, "not an OutProofV2"));
        }
        Ok(out)
    }

    /// The message the `OutProofV2` inside must have been made over.
    pub fn message(&self) -> Vec<u8> {
        burn_message(&self.persona, &self.purpose)
    }
}

/// Sign a burn proof under the persona it names (§18.3).
pub fn sign_burn_proof(b: &BurnProof, key: &SecretKey) -> Vec<u8> {
    seal(&SignedBytes::from_value(b.to_value()), ObjectType::BurnProof, key)
}

fn suite_of(code: u8) -> Result<Suite, Reject> {
    match code {
        1 => Ok(Suite::Ed25519X25519),
        2 => Ok(Suite::P256),
        _ => Err(Reject::with_detail(RejectCode::UnsupportedSuite, "unknown suite")),
    }
}

/// Open a burn proof: verify the envelope under the persona in the body.
/// The reader then checks that persona is the one presenting it, and takes
/// the proof to its node (§9.5) — neither of which this function can do.
pub fn open_burn_proof(envelope: &[u8]) -> Result<BurnProof, Reject> {
    let peek = BurnProof::from_value(decode(&peek_body(envelope)?)?)?;
    let pk = PublicKey::from_bytes(suite_of(peek.suite)?, &peek.persona)
        .map_err(|_| Reject::with_detail(RejectCode::Malformed, "persona is not a public key"))?;
    let (ty, body) = open(envelope, &pk)?;
    if ty != ObjectType::BurnProof {
        return Err(Reject::with_detail(RejectCode::Malformed, "object type is not BURN_PROOF"));
    }
    BurnProof::from_value(decode(body.bytes())?)
}

/// The version every `ATTESTATION` written today carries.
pub const ATTESTATION_VERSION: u64 = 1;
/// A note is a sentence beside a rating, not a review.
pub const MAX_ATTESTATION_NOTE_CHARS: usize = 140;
/// Ratings are a closed set; a client draws them, it does not average free text.
pub const RATING_MIN: u8 = 1;
pub const RATING_MAX: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attestation {
    pub version: u64,
    pub suite: u8,
    /// Who is speaking — the envelope's signer.
    pub signer: Vec<u8>,
    /// Who is spoken about.
    pub subject: Vec<u8>,
    /// What settled between them, in pXMR.
    pub amount_pxmr: u64,
    /// 1..=5.
    pub rating: u8,
    /// Seconds since the epoch.
    pub ts: u64,
    /// The transaction the deal settled on, when there was one.
    pub txid: Option<[u8; 32]>,
    /// A sentence, optional.
    pub note: Option<String>,
}

impl Attestation {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::Attestation)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::AT_SUBJECT, Value::Bytes(self.subject.clone()));
        m.insert(f::AT_AMOUNT, Value::Uint(self.amount_pxmr));
        m.insert(f::AT_RATING, Value::Uint(self.rating as u64));
        m.insert(f::AT_TS, Value::Uint(self.ts));
        if let Some(t) = &self.txid {
            m.insert(f::AT_TXID, Value::Bytes(t.to_vec()));
        }
        if let Some(n) = &self.note {
            m.insert(f::AT_NOTE, Value::Text(n.clone()));
        }
        m.insert(f::AT_SIGNER, Value::Bytes(self.signer.clone()));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::Attestation) {
            return Err(Reject::with_detail(RejectCode::Malformed, "object type is not ATTESTATION"));
        }
        let version = r.uint(f::VERSION)?;
        if version != ATTESTATION_VERSION {
            return Err(Reject::with_detail(RejectCode::Malformed, "attestation version is not 1"));
        }
        let rating = r.uint(f::AT_RATING)?;
        if rating < RATING_MIN as u64 || rating > RATING_MAX as u64 {
            return Err(Reject::with_detail(RejectCode::Malformed, "rating is 1 to 5"));
        }
        let out = Attestation {
            version,
            suite: r.uint(f::SUITE)? as u8,
            subject: r.bytes(f::AT_SUBJECT, Some(32))?,
            amount_pxmr: r.uint(f::AT_AMOUNT)?,
            rating: rating as u8,
            ts: r.uint(f::AT_TS)?,
            txid: match r.opt_bytes(f::AT_TXID, Some(32))? {
                None => None,
                Some(b) => Some(b.try_into().map_err(|_| {
                    Reject::with_detail(RejectCode::Malformed, "txid is 32 bytes")
                })?),
            },
            note: r.opt_text(f::AT_NOTE, MAX_ATTESTATION_NOTE_CHARS)?,
            signer: r.bytes(f::AT_SIGNER, Some(32))?,
        };
        r.finish()?;
        if out.ts == 0 {
            return Err(Reject::with_detail(RejectCode::Malformed, "an attestation needs a time"));
        }
        if out.subject == out.signer {
            // The one review nobody needs: a persona rating itself.
            return Err(Reject::with_detail(RejectCode::Malformed, "a persona cannot attest to itself"));
        }
        Ok(out)
    }
}

/// Sign an attestation under the signer it names (§18.3).
pub fn sign_attestation(a: &Attestation, key: &SecretKey) -> Vec<u8> {
    seal(&SignedBytes::from_value(a.to_value()), ObjectType::Attestation, key)
}

/// Open an attestation: verify the envelope under the signer in the body.
/// Whose word it is, is then the reader's to weigh (§9.2).
pub fn open_attestation(envelope: &[u8]) -> Result<Attestation, Reject> {
    let peek = Attestation::from_value(decode(&peek_body(envelope)?)?)?;
    let pk = PublicKey::from_bytes(suite_of(peek.suite)?, &peek.signer)
        .map_err(|_| Reject::with_detail(RejectCode::Malformed, "signer is not a public key"))?;
    let (ty, body) = open(envelope, &pk)?;
    if ty != ObjectType::Attestation {
        return Err(Reject::with_detail(RejectCode::Malformed, "object type is not ATTESTATION"));
    }
    Attestation::from_value(decode(body.bytes())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> SecretKey {
        SecretKey::ed25519_from_bytes(&[b; 32])
    }

    fn proof_string(pairs: usize) -> String {
        format!("{}{}", OUT_PROOF_HEADER, "A".repeat(pairs * OUT_PROOF_PAIR_CHARS))
    }

    fn burn(sk: &SecretKey) -> BurnProof {
        BurnProof {
            version: BURN_PROOF_VERSION,
            suite: 1,
            txid: [0x11; 32],
            amount_pxmr: BURN_FLOOR_PXMR,
            height: 1_800_000,
            proof: proof_string(1),
            persona: sk.public().to_bytes().to_vec(),
            purpose: "identity".into(),
        }
    }

    #[test]
    fn a_burn_proof_opens_only_under_the_persona_it_names() {
        let sk = key(0x61);
        let b = burn(&sk);
        let env = sign_burn_proof(&b, &sk);
        assert_eq!(open_burn_proof(&env).unwrap(), b);
        assert!(open_burn_proof(&sign_burn_proof(&b, &key(0x62))).is_err(), "somebody else's key");
        assert!(open_burn_proof(&b.to_value().encode()).is_err(), "a bare map");
    }

    #[test]
    fn a_burn_proof_has_a_shape_before_it_has_a_chain() {
        let sk = key(0x61);
        let ok = burn(&sk);
        assert!(BurnProof::from_value(ok.to_value()).is_ok());
        for (why, bad) in [
            ("nothing burned", BurnProof { amount_pxmr: 0, ..ok.clone() }),
            ("no block", BurnProof { height: 0, ..ok.clone() }),
            ("not an out proof", BurnProof { proof: format!("InProofV2{}", "A".repeat(132)), ..ok.clone() }),
            ("a torn pair", BurnProof { proof: proof_string(1) + "A", ..ok.clone() }),
            ("no pairs", BurnProof { proof: OUT_PROOF_HEADER.into(), ..ok.clone() }),
            ("purpose too long", BurnProof { purpose: "p".repeat(MAX_PURPOSE_CHARS + 1), ..ok.clone() }),
            ("version 2", BurnProof { version: 2, ..ok.clone() }),
        ] {
            assert!(BurnProof::from_value(bad.to_value()).is_err(), "{why}");
        }
        assert_eq!(ok.message(), burn_message(&ok.persona, "identity"));
        assert!(ok.message().starts_with(BURN_DOMAIN));
    }

    fn attestation(signer: &SecretKey, subject: &SecretKey) -> Attestation {
        Attestation {
            version: ATTESTATION_VERSION,
            suite: 1,
            signer: signer.public().to_bytes().to_vec(),
            subject: subject.public().to_bytes().to_vec(),
            amount_pxmr: 250_000_000_000,
            rating: 5,
            ts: 1_760_000_000,
            txid: Some([0x22; 32]),
            note: Some("On time, as described.".into()),
        }
    }

    #[test]
    fn an_attestation_opens_only_under_its_signer_and_rates_within_the_set() {
        let (s, t) = (key(0x71), key(0x72));
        let a = attestation(&s, &t);
        assert_eq!(open_attestation(&sign_attestation(&a, &s)).unwrap(), a);
        assert!(open_attestation(&sign_attestation(&a, &t)).is_err(), "the subject signing about itself");
        for (why, bad) in [
            ("rating 0", Attestation { rating: 0, ..a.clone() }),
            ("rating 6", Attestation { rating: 6, ..a.clone() }),
            ("no time", Attestation { ts: 0, ..a.clone() }),
            ("self", Attestation { subject: a.signer.clone(), ..a.clone() }),
            ("note too long", Attestation { note: Some("n".repeat(MAX_ATTESTATION_NOTE_CHARS + 1)), ..a.clone() }),
            ("bidi in the note", Attestation { note: Some("fine\u{202E}".into()), ..a.clone() }),
        ] {
            assert!(Attestation::from_value(bad.to_value()).is_err(), "{why}");
        }
        let bare = Attestation { txid: None, note: None, ..a.clone() };
        assert_eq!(Attestation::from_value(bare.to_value()).unwrap(), bare);
    }
}
