//! Wire objects and their field numbering.
//!
//! Part II says which fields exist; Part V says how to encode them. Neither
//! assigned integer keys, so nothing could be built. This module does that.
//!
//! # Envelope
//!
//! A signed object is carried as `{1: body, 2: sig}` where `body` is an opaque
//! byte string holding the object's own canonical CBOR. The signature covers
//! exactly those bytes.
//!
//! The obvious alternative — a `sig` field inside the signed map — requires
//! every signer and verifier to remove that field, re-encode, and compare. That
//! dance is a reliable source of verifier bugs, and it fights §18.3's rule that
//! verification runs over bytes as received rather than a re-encoding. With an
//! envelope the signed bytes are simply *there*, and a relay can check a
//! signature without parsing the body at all.
//!
//! # Numbering
//!
//! Keys are small unsigned integers, so each costs one byte (§18.1). Field 0 is
//! always the object type, which makes an object self-describing and selects the
//! domain-separation label without the parser having to guess from context. The
//! type is inside the signed body, so an attacker who edits it invalidates the
//! signature rather than redirecting the object.

use std::collections::BTreeMap;

use crate::cbor::{decode, Value};
use crate::commit::{commit, commit_eq, Purpose};
use crate::reject::{Reject, RejectCode};
use crate::sig::{ObjectType, PublicKey, SecretKey, SignedBytes};

// Envelope keys.
const ENV_BODY: u64 = 1;
const ENV_SIG: u64 = 2;

// Fields common to every object.
pub mod f {
    pub const TYPE: u64 = 0;
    pub const VERSION: u64 = 1;
    pub const SUITE: u64 = 2;

    // TapPresent (§15.3)
    pub const PROFILE: u64 = 3;
    pub const PRESENTER_ROLE: u64 = 4;
    pub const AMOUNT_AUTHORITY: u64 = 5;
    pub const INTENT: u64 = 6;
    pub const RMODE: u64 = 7;
    pub const NONCE: u64 = 8;
    pub const EXPIRY: u64 = 9;
    pub const SESSION_PK: u64 = 10;
    pub const ROUTE: u64 = 11;
    pub const OFFER_COMMIT: u64 = 12;
    pub const DEST: u64 = 13;
    pub const SESSION_REF: u64 = 14;

    // FullOffer (§15.4)
    pub const PAYTO: u64 = 15;
    pub const AMOUNT_PXMR: u64 = 16;
    pub const SUPPORTED_VERSIONS: u64 = 17;
    pub const SUPPORTED_SUITES: u64 = 18;
    pub const SETTLE_MODE: u64 = 19;
    pub const FEE_POLICY: u64 = 20;
    pub const NONCE_ECHO: u64 = 21;

    // ACCEPT (§15.5) — covers exactly the typed fields the payer's app rendered
    pub const OFFER_HASH: u64 = 22;
    pub const AMOUNT_FINAL: u64 = 23;
    pub const READER_SESSION_PK: u64 = 24;
    pub const TIMESTAMP: u64 = 25;
    pub const CHOSEN_VERSION: u64 = 26;
    pub const CHOSEN_SUITE: u64 = 27;
    /// Where a refund should be sent (§7.3). Optional — see `Accept`.
    pub const REFUND_TO: u64 = 102;
    /// Where a refund actually went, checked against the above.
    pub const REFUND_PAID_TO: u64 = 103;

    // TXID (§17.4). A pointer into the mempool, not evidence — see escrow.rs.
    pub const TXID_ACCEPT_LINK: u64 = 46;
    pub const TXID_TXID: u64 = 47;
    pub const TXID_AMOUNT: u64 = 48;
    pub const TXID_TS: u64 = 49;

    // ESCROW_SETUP (§8.2)
    pub const ESC_ID: u64 = 104;
    pub const ESC_ROUND: u64 = 105;
    pub const ESC_INFO: u64 = 106;
    pub const ESC_FROM: u64 = 107;
    pub const ESC_TS: u64 = 108;

    // ESCROW_READY (§8.2)
    pub const RDY_ID: u64 = 111;
    pub const RDY_ADDRESS: u64 = 112;
    pub const RDY_THRESHOLD: u64 = 113;
    pub const RDY_TOTAL: u64 = 114;
    pub const RDY_ARBITER: u64 = 115;
    pub const RDY_FROM: u64 = 116;
    pub const RDY_TS: u64 = 117;

    // RELEASE (§8.2)
    pub const REL_ID: u64 = 119;
    pub const REL_READY_LINK: u64 = 120;
    pub const REL_TO: u64 = 121;
    pub const REL_AMOUNT: u64 = 122;
    pub const REL_TS: u64 = 123;

    // TXPROOF (§17.5) — arbitration evidence only.
    pub const PRF_TXID: u64 = 125;
    pub const PRF_PROOF: u64 = 126;
    pub const PRF_DESTINATION: u64 = 127;
    pub const PRF_MESSAGE: u64 = 128;
    pub const PRF_AMOUNT: u64 = 129;
    pub const PRF_TS: u64 = 130;

    // SLASH_CLAIM (§17.5)
    pub const SLC_ACCEPT_LINK: u64 = 132;
    pub const SLC_RECEIPT_LINK: u64 = 133;
    pub const SLC_TXID: u64 = 134;
    pub const SLC_REASON: u64 = 135;
    pub const SLC_KEY_IMAGE: u64 = 136;
    pub const SLC_AMOUNT: u64 = 137;
    pub const SLC_TS: u64 = 138;

    /// Nested terms map (§7.3, §15.7, §8.8). Its inner keys are their own
    /// namespace, defined in `terms`.
    pub const TERMS: u64 = 96;

    // CANCEL (§7.3) — reserved range 38-39
    pub const CANCEL_PRIOR: u64 = 38;
    pub const CANCEL_FEE: u64 = 39;

    // TapStatic (§15.9) — reserved range 31-33
    pub const STATIC_PAYTO: u64 = 31;
    pub const STATIC_PERSONA: u64 = 32;
    pub const STATIC_SIG: u64 = 33;

    // HAIL and its sealed reply (§5.2.1) — reserved range 60-67
    pub const HAIL_GEOCELL: u64 = 60;
    pub const HAIL_EPHEMERAL_PK: u64 = 61;
    pub const HAIL_EXPIRY: u64 = 62;
    pub const HAILREPLY_NONCE_ECHO: u64 = 63;
    pub const HAILREPLY_SESSION_PK: u64 = 64;
    pub const HAILREPLY_QUOTE: u64 = 65;

    // DISPUTE / RULING (§9.3.2) — reserved range 52-59
    pub const DISPUTE_CLASS: u64 = 52;
    pub const DISPUTE_TRANSCRIPT: u64 = 53;
    pub const DISPUTE_CLAIM_PXMR: u64 = 54;
    pub const DISPUTE_TS: u64 = 55;
    pub const RULING_DISPUTE: u64 = 56;
    pub const RULING_OUTCOME: u64 = 57;
    pub const RULING_AWARD: u64 = 58;
    pub const RULING_TS: u64 = 59;

    // MANDATE (§7.3)
    pub const MANDATE_PAYEE: u64 = 97;
    pub const MANDATE_CAP: u64 = 98;
    pub const MANDATE_PERIOD: u64 = 99;
    pub const MANDATE_EXPIRY: u64 = 100;
    pub const MANDATE_NONCE: u64 = 101;

    // REFUND (§7.3) — reserved range 34-39
    pub const PRIOR_RECEIPT: u64 = 34;
    pub const REFUND_AMOUNT: u64 = 35;
    pub const REFUND_TXID: u64 = 36;
    pub const REFUND_TS: u64 = 37;

    // RECEIPT
    pub const ACCEPT_HASH: u64 = 28;
    pub const PREV: u64 = 29;
    pub const UNILATERAL: u64 = 30;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PresenterRole {
    Payee = 0,
    Payer = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AmountAuthority {
    Fixed = 0,
    Open = 1,
    Rated = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Intent {
    Oneshot = 0,
    Start = 1,
    Stop = 2,
}

/// How the reader reaches back (§15.3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ReachMode {
    Inline = 0,
    Token = 1,
    Ble = 2,
}

/// Terms the payer signs by accepting an offer.
///
/// Several requirements referenced `terms.*` before anything carried them:
/// §7.3's cancellation schedule and refund window, §15.7's mandatory meter cap,
/// §8.8's minimum fee tier. A rule about a field that does not exist cannot be
/// obeyed, and this is where they live.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Terms {
    /// §7.3. Fee owed if the payer cancels after ACCEPT and before FUND.
    /// Uncollectable against an unbonded counterparty, which is stated rather
    /// than pretended otherwise.
    pub cancellation_pxmr: u64,
    /// §7.3. Seconds during which this receipt may be referenced by a REFUND.
    /// Zero is legitimate and means final sale.
    pub refund_window_s: u64,
    /// §15.7. Ceiling on a metered total. **Required whenever
    /// `amount_authority = rated`** — an open-ended obligation cannot be
    /// consented to, and §15.5 fails without it.
    pub meter_cap_pxmr: u64,
    /// §15.7. Seconds after which an unstopped meter auto-stops.
    pub meter_max_s: u64,
    /// §8.8. Minimum fee tier the payee will accept, so fee underpayment is a
    /// pre-condition rather than a cure-window problem after the fact.
    pub min_fee_tier: u64,
}

mod terms_keys {
    pub const CANCELLATION: u64 = 0;
    pub const REFUND_WINDOW: u64 = 1;
    pub const METER_CAP: u64 = 2;
    pub const METER_MAX: u64 = 3;
    pub const MIN_FEE_TIER: u64 = 4;
}

impl Terms {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(terms_keys::CANCELLATION, Value::Uint(self.cancellation_pxmr));
        m.insert(terms_keys::REFUND_WINDOW, Value::Uint(self.refund_window_s));
        m.insert(terms_keys::METER_CAP, Value::Uint(self.meter_cap_pxmr));
        m.insert(terms_keys::METER_MAX, Value::Uint(self.meter_max_s));
        m.insert(terms_keys::MIN_FEE_TIER, Value::Uint(self.min_fee_tier));
        Value::Map(m)
    }

    pub fn from_value(v: &Value) -> Result<Self, Reject> {
        let m = v.as_map().ok_or_else(|| {
            Reject::with_detail(RejectCode::Malformed, "terms must be a map")
        })?;
        let get = |k: u64| -> Result<u64, Reject> {
            m.get(&k)
                .and_then(|x| x.as_uint())
                .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, format!("terms field {}", k)))
        };
        let out = Terms {
            cancellation_pxmr: get(terms_keys::CANCELLATION)?,
            refund_window_s: get(terms_keys::REFUND_WINDOW)?,
            meter_cap_pxmr: get(terms_keys::METER_CAP)?,
            meter_max_s: get(terms_keys::METER_MAX)?,
            min_fee_tier: get(terms_keys::MIN_FEE_TIER)?,
        };
        if m.len() != 5 {
            return Err(Reject::with_detail(
                RejectCode::UnknownField,
                "unrecognised field in terms",
            ));
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FeePolicy {
    /// Payee receives `amount_pxmr` exactly; the fee is on top of the payer's
    /// outlay, and the confirm screen MUST show the total (§8.8).
    PayerPays = 0,
    PayeeAbsorbs = 1,
}

pub(crate) fn type_code(t: ObjectType) -> u64 {
    match t {
        ObjectType::TapPresent => 1,
        ObjectType::FullOffer => 2,
        ObjectType::Accept => 3,
        ObjectType::Receipt => 4,
        ObjectType::TxProof => 5,
        ObjectType::Refund => 6,
        ObjectType::Cancel => 7,
        ObjectType::Mandate => 8,
        ObjectType::ContactOffer => 9,
        ObjectType::ContactAccept => 10,
        ObjectType::BondProof => 11,
        ObjectType::Attestation => 12,
        ObjectType::Dispute => 13,
        ObjectType::Ruling => 14,
        ObjectType::Hail => 15,
        ObjectType::HailReply => 16,
        ObjectType::TapStatic => 17,
        ObjectType::TxId => 18,
        ObjectType::EscrowSetup => 19,
        ObjectType::EscrowReady => 20,
        ObjectType::Release => 21,
        ObjectType::SlashClaim => 22,
    }
}

fn type_from_code(c: u64) -> Option<ObjectType> {
    Some(match c {
        1 => ObjectType::TapPresent,
        2 => ObjectType::FullOffer,
        3 => ObjectType::Accept,
        4 => ObjectType::Receipt,
        5 => ObjectType::TxProof,
        6 => ObjectType::Refund,
        7 => ObjectType::Cancel,
        8 => ObjectType::Mandate,
        9 => ObjectType::ContactOffer,
        10 => ObjectType::ContactAccept,
        11 => ObjectType::BondProof,
        12 => ObjectType::Attestation,
        _ => return None,
    })
}

// ------------------------------------------------------------- strict read --

/// Consuming reader. Every field taken is removed, so `finish` can enforce
/// §18.8's rule that an unknown field is a rejection: a client that tolerated
/// fields it did not understand would be signing something it never displayed.
pub(crate) struct Reader {
    m: BTreeMap<u64, Value>,
}

impl Reader {
    pub(crate) fn new(v: Value) -> Result<Self, Reject> {
        match v {
            Value::Map(m) => Ok(Reader { m }),
            _ => Err(Reject::with_detail(
                RejectCode::Malformed,
                "object must be a map",
            )),
        }
    }

    fn missing(k: u64) -> Reject {
        Reject::with_detail(RejectCode::Malformed, format!("missing field {}", k))
    }
    fn wrong(k: u64) -> Reject {
        Reject::with_detail(RejectCode::Malformed, format!("wrong type for field {}", k))
    }

    pub(crate) fn uint(&mut self, k: u64) -> Result<u64, Reject> {
        self.m
            .remove(&k)
            .ok_or_else(|| Self::missing(k))?
            .as_uint()
            .ok_or_else(|| Self::wrong(k))
    }

    pub(crate) fn bytes(&mut self, k: u64, len: Option<usize>) -> Result<Vec<u8>, Reject> {
        let b = self
            .m
            .remove(&k)
            .ok_or_else(|| Self::missing(k))?
            .as_bytes()
            .ok_or_else(|| Self::wrong(k))?
            .to_vec();
        if let Some(n) = len {
            if b.len() != n {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    format!("field {} must be {} bytes, got {}", k, n, b.len()),
                ));
            }
        }
        Ok(b)
    }

    pub(crate) fn opt_bytes(&mut self, k: u64, len: Option<usize>) -> Result<Option<Vec<u8>>, Reject> {
        if !self.m.contains_key(&k) {
            return Ok(None);
        }
        self.bytes(k, len).map(Some)
    }

    fn uint_array(&mut self, k: u64) -> Result<Vec<u64>, Reject> {
        match self.m.remove(&k).ok_or_else(|| Self::missing(k))? {
            Value::Array(items) => items
                .iter()
                .map(|i| i.as_uint().ok_or_else(|| Self::wrong(k)))
                .collect(),
            _ => Err(Self::wrong(k)),
        }
    }

    /// Reject anything left over (§18.8).
    pub(crate) fn finish(self) -> Result<(), Reject> {
        if let Some((k, _)) = self.m.into_iter().next() {
            return Err(Reject::with_detail(
                RejectCode::UnknownField,
                format!("unrecognised field {}", k),
            ));
        }
        Ok(())
    }
}

fn enum_u8<T: Copy>(k: u64, raw: u64, table: &[(u64, T)]) -> Result<T, Reject> {
    table
        .iter()
        .find(|(c, _)| *c == raw)
        .map(|(_, v)| *v)
        .ok_or_else(|| {
            Reject::with_detail(
                RejectCode::Malformed,
                format!("field {} has no such value: {}", k, raw),
            )
        })
}

// --------------------------------------------------------------- envelope --

/// Wrap a signed body and its signature.
pub fn seal(body: &SignedBytes, object_type: ObjectType, key: &SecretKey) -> Vec<u8> {
    let sig = body.sign(object_type, key);
    let mut m = BTreeMap::new();
    m.insert(ENV_BODY, Value::Bytes(body.bytes().to_vec()));
    m.insert(ENV_SIG, Value::Bytes(sig.to_vec()));
    Value::Map(m).encode()
}

/// Unwrap and verify, returning the body's canonical bytes.
///
/// The object type is read from the body itself and used to select the
/// domain-separation label, so a caller cannot accidentally verify an object
/// under the wrong context — and an attacker who edits the type field breaks
/// the signature rather than redirecting the object.
pub fn open(envelope: &[u8], key: &PublicKey) -> Result<(ObjectType, SignedBytes), Reject> {
    let mut env = Reader::new(decode(envelope)?)?;
    let body_bytes = env.bytes(ENV_BODY, None)?;
    let sig_bytes = env.bytes(ENV_SIG, Some(64))?;
    env.finish()?;

    let body = SignedBytes::from_received(body_bytes)
        .map_err(|_| Reject::with_detail(RejectCode::Malformed, "body is not canonical CBOR"))?;

    let raw_type = body
        .value()
        .as_map()
        .and_then(|m| m.get(&f::TYPE))
        .and_then(|v| v.as_uint())
        .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "missing object type"))?;
    let object_type = type_from_code(raw_type)
        .ok_or_else(|| Reject::with_detail(RejectCode::UnsupportedProfile, "unknown object type"))?;

    // The object *declares* a suite and the key *is* of a suite. Nothing
    // previously required them to agree, so a mismatch surfaced as `BAD_SIG` —
    // technically safe, since the signature cannot verify under the wrong key,
    // but a misleading diagnostic and a place two implementations could refuse
    // the same object for different stated reasons. §18.5 wants them to agree.
    let declared = body
        .value()
        .as_map()
        .and_then(|m| m.get(&f::SUITE))
        .and_then(|v| v.as_uint())
        .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "missing suite"))?;
    let actual = key.suite() as u64;
    if declared != actual {
        return Err(Reject::with_detail(
            RejectCode::UnsupportedSuite,
            format!("object declares suite {} but the key is suite {}", declared, actual),
        ));
    }

    let sig: [u8; 64] = sig_bytes.try_into().unwrap();
    body.verify(object_type, key, &sig)
        .map_err(|_| Reject::new(RejectCode::BadSig))?;

    Ok((object_type, body))
}

// ------------------------------------------------------------ TapPresent --

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapPresent {
    pub version: u64,
    pub suite: u8,
    pub profile: u64,
    pub presenter_role: PresenterRole,
    pub amount_authority: AmountAuthority,
    pub intent: Intent,
    pub rmode: ReachMode,
    pub nonce: [u8; 16],
    pub expiry: u64,
    pub session_pk: Vec<u8>,
    pub route: Vec<u8>,
    pub offer_commit: [u8; 32],
    pub dest: Option<Vec<u8>>,
    pub session_ref: Option<[u8; 32]>,
}

impl TapPresent {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::TapPresent)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::PROFILE, Value::Uint(self.profile));
        m.insert(f::PRESENTER_ROLE, Value::Uint(self.presenter_role as u64));
        m.insert(f::AMOUNT_AUTHORITY, Value::Uint(self.amount_authority as u64));
        m.insert(f::INTENT, Value::Uint(self.intent as u64));
        m.insert(f::RMODE, Value::Uint(self.rmode as u64));
        m.insert(f::NONCE, Value::Bytes(self.nonce.to_vec()));
        m.insert(f::EXPIRY, Value::Uint(self.expiry));
        m.insert(f::SESSION_PK, Value::Bytes(self.session_pk.clone()));
        m.insert(f::ROUTE, Value::Bytes(self.route.clone()));
        m.insert(f::OFFER_COMMIT, Value::Bytes(self.offer_commit.to_vec()));
        if let Some(d) = &self.dest {
            m.insert(f::DEST, Value::Bytes(d.clone()));
        }
        if let Some(r) = &self.session_ref {
            m.insert(f::SESSION_REF, Value::Bytes(r.to_vec()));
        }
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        let t = r.uint(f::TYPE)?;
        if t != type_code(ObjectType::TapPresent) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not TapPresent",
            ));
        }
        let version = r.uint(f::VERSION)?;
        let suite = r.uint(f::SUITE)? as u8;
        let profile = r.uint(f::PROFILE)?;
        let presenter_role = enum_u8(
            f::PRESENTER_ROLE,
            r.uint(f::PRESENTER_ROLE)?,
            &[(0, PresenterRole::Payee), (1, PresenterRole::Payer)],
        )?;
        let amount_authority = enum_u8(
            f::AMOUNT_AUTHORITY,
            r.uint(f::AMOUNT_AUTHORITY)?,
            &[
                (0, AmountAuthority::Fixed),
                (1, AmountAuthority::Open),
                (2, AmountAuthority::Rated),
            ],
        )?;
        let intent = enum_u8(
            f::INTENT,
            r.uint(f::INTENT)?,
            &[(0, Intent::Oneshot), (1, Intent::Start), (2, Intent::Stop)],
        )?;
        let rmode = enum_u8(
            f::RMODE,
            r.uint(f::RMODE)?,
            &[
                (0, ReachMode::Inline),
                (1, ReachMode::Token),
                (2, ReachMode::Ble),
            ],
        )?;
        let nonce: [u8; 16] = r.bytes(f::NONCE, Some(16))?.try_into().unwrap();
        let expiry = r.uint(f::EXPIRY)?;
        let session_pk = r.bytes(f::SESSION_PK, None)?;
        let route = r.bytes(f::ROUTE, None)?;
        let offer_commit: [u8; 32] = r.bytes(f::OFFER_COMMIT, Some(32))?.try_into().unwrap();
        let dest = r.opt_bytes(f::DEST, Some(16))?;
        let session_ref = r
            .opt_bytes(f::SESSION_REF, Some(32))?
            .map(|b| b.try_into().unwrap());
        r.finish()?;

        // §15.7: a `stop` is only meaningful against a meter you started, and a
        // `session_ref` on anything else is a sign of a confused or hostile
        // presenter rather than a harmless extra.
        if (intent == Intent::Stop) != session_ref.is_some() {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "session_ref must be present exactly when intent = stop",
            ));
        }

        Ok(TapPresent {
            version,
            suite,
            profile,
            presenter_role,
            amount_authority,
            intent,
            rmode,
            nonce,
            expiry,
            session_pk,
            route,
            offer_commit,
            dest,
            session_ref,
        })
    }
}

// -------------------------------------------------------------- FullOffer --

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullOffer {
    pub version: u64,
    pub suite: u8,
    pub profile: u64,
    pub payto: Vec<u8>,
    pub amount_pxmr: u64,
    pub supported_versions: Vec<u64>,
    pub supported_suites: Vec<u64>,
    pub settle_mode: u64,
    pub fee_policy: FeePolicy,
    /// Echoes the tap's nonce, binding this offer to the bootstrap that
    /// advertised it in the payer-visible direction as well as via the digest.
    pub nonce_echo: [u8; 16],
    /// §7.3, §15.7, §8.8. Signed by the payer along with everything else.
    pub terms: Terms,
}

impl FullOffer {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::FullOffer)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::PROFILE, Value::Uint(self.profile));
        m.insert(f::PAYTO, Value::Bytes(self.payto.clone()));
        m.insert(f::AMOUNT_PXMR, Value::Uint(self.amount_pxmr));
        m.insert(
            f::SUPPORTED_VERSIONS,
            Value::Array(self.supported_versions.iter().map(|v| Value::Uint(*v)).collect()),
        );
        m.insert(
            f::SUPPORTED_SUITES,
            Value::Array(self.supported_suites.iter().map(|v| Value::Uint(*v)).collect()),
        );
        m.insert(f::SETTLE_MODE, Value::Uint(self.settle_mode));
        m.insert(f::FEE_POLICY, Value::Uint(self.fee_policy as u64));
        m.insert(f::NONCE_ECHO, Value::Bytes(self.nonce_echo.to_vec()));
        m.insert(f::TERMS, self.terms.to_value());
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::FullOffer) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not FullOffer",
            ));
        }
        let out = FullOffer {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            profile: r.uint(f::PROFILE)?,
            payto: r.bytes(f::PAYTO, None)?,
            amount_pxmr: r.uint(f::AMOUNT_PXMR)?,
            supported_versions: r.uint_array(f::SUPPORTED_VERSIONS)?,
            supported_suites: r.uint_array(f::SUPPORTED_SUITES)?,
            settle_mode: r.uint(f::SETTLE_MODE)?,
            fee_policy: enum_u8(
                f::FEE_POLICY,
                r.uint(f::FEE_POLICY)?,
                &[(0, FeePolicy::PayerPays), (1, FeePolicy::PayeeAbsorbs)],
            )?,
            nonce_echo: r.bytes(f::NONCE_ECHO, Some(16))?.try_into().unwrap(),
            terms: {
                let v = r
                    .m
                    .remove(&f::TERMS)
                    .ok_or_else(|| Reader::missing(f::TERMS))?;
                Terms::from_value(&v)?
            },
        };
        r.finish()?;
        Ok(out)
    }

    /// The commitment a `TapPresent` carries (§15.3).
    pub fn commitment(&self) -> [u8; 32] {
        commit(Purpose::Offer, &self.to_value().encode())
    }
}

// ------------------------------------------------------------------ ACCEPT --

/// §15.5: covers exactly the typed fields the payer's app rendered and verified.
/// Nothing here originates as a display string from the counterparty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accept {
    pub version: u64,
    pub suite: u8,
    pub nonce: [u8; 16],
    pub offer_hash: [u8; 32],
    pub amount_final: u64,
    pub dest: Option<Vec<u8>>,
    pub reader_session_pk: Vec<u8>,
    pub timestamp: u64,
    pub chosen_version: u64,
    pub chosen_suite: u64,
    /// Where a refund should be sent, if the payer wants one to be possible.
    ///
    /// Nothing previously carried this, so a merchant willing to refund had no
    /// address to refund *to* and had to obtain one out of band. That ambiguity
    /// is what a published attack on Bitcoin's BIP-70 exploited: if the refund
    /// destination is not bound to the transaction the payer signed, it can be
    /// substituted.
    ///
    /// Optional, because supplying it costs privacy — the payee learns a payer
    /// address even when no refund ever happens, which §15.10 otherwise avoids.
    /// A payer who omits it has chosen unlinkability over refundability, and a
    /// client MUST present that as the trade it is rather than filling the field
    /// silently.
    pub refund_to: Option<Vec<u8>>,
}

impl Accept {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::Accept)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::NONCE, Value::Bytes(self.nonce.to_vec()));
        m.insert(f::OFFER_HASH, Value::Bytes(self.offer_hash.to_vec()));
        m.insert(f::AMOUNT_FINAL, Value::Uint(self.amount_final));
        if let Some(d) = &self.dest {
            m.insert(f::DEST, Value::Bytes(d.clone()));
        }
        m.insert(
            f::READER_SESSION_PK,
            Value::Bytes(self.reader_session_pk.clone()),
        );
        m.insert(f::TIMESTAMP, Value::Uint(self.timestamp));
        m.insert(f::CHOSEN_VERSION, Value::Uint(self.chosen_version));
        m.insert(f::CHOSEN_SUITE, Value::Uint(self.chosen_suite));
        if let Some(r) = &self.refund_to {
            m.insert(f::REFUND_TO, Value::Bytes(r.clone()));
        }
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::Accept) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not ACCEPT",
            ));
        }
        let out = Accept {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            nonce: r.bytes(f::NONCE, Some(16))?.try_into().unwrap(),
            offer_hash: r.bytes(f::OFFER_HASH, Some(32))?.try_into().unwrap(),
            amount_final: r.uint(f::AMOUNT_FINAL)?,
            dest: r.opt_bytes(f::DEST, Some(16))?,
            reader_session_pk: r.bytes(f::READER_SESSION_PK, None)?,
            timestamp: r.uint(f::TIMESTAMP)?,
            chosen_version: r.uint(f::CHOSEN_VERSION)?,
            chosen_suite: r.uint(f::CHOSEN_SUITE)?,
            refund_to: r.opt_bytes(f::REFUND_TO, None)?,
        };
        r.finish()?;
        Ok(out)
    }
}

// ----------------------------------------------------------------- RECEIPT --

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub version: u64,
    pub suite: u8,
    pub accept_hash: [u8; 32],
    /// §6's chain link: the digest of the predecessor message.
    pub prev: [u8; 32],
    pub amount_final: u64,
    pub timestamp: u64,
    /// True when emitted without the counterparty's co-signature (§6.2). It
    /// proves what the payer signed and paid; it does not claim delivery.
    pub unilateral: bool,
}

impl Receipt {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::Receipt)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::AMOUNT_FINAL, Value::Uint(self.amount_final));
        m.insert(f::TIMESTAMP, Value::Uint(self.timestamp));
        m.insert(f::ACCEPT_HASH, Value::Bytes(self.accept_hash.to_vec()));
        m.insert(f::PREV, Value::Bytes(self.prev.to_vec()));
        m.insert(f::UNILATERAL, Value::Bool(self.unilateral));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::Receipt) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not RECEIPT",
            ));
        }
        let version = r.uint(f::VERSION)?;
        let suite = r.uint(f::SUITE)? as u8;
        let amount_final = r.uint(f::AMOUNT_FINAL)?;
        let timestamp = r.uint(f::TIMESTAMP)?;
        let accept_hash: [u8; 32] = r.bytes(f::ACCEPT_HASH, Some(32))?.try_into().unwrap();
        let prev: [u8; 32] = r.bytes(f::PREV, Some(32))?.try_into().unwrap();
        let unilateral = match r.m.remove(&f::UNILATERAL) {
            Some(Value::Bool(b)) => b,
            Some(_) => return Err(Reader::wrong(f::UNILATERAL)),
            None => return Err(Reader::missing(f::UNILATERAL)),
        };
        r.finish()?;
        Ok(Receipt {
            version,
            suite,
            accept_hash,
            prev,
            amount_final,
            timestamp,
            unilateral,
        })
    }
}

// -------------------------------------------------------------- transcript --

/// Verify a complete tap-to-receipt transcript (§6).
///
/// Checks the chain of commitments end to end, which is what makes a completed
/// transaction self-verifying for the two parties who hold it:
///
///   * the offer matches the commitment the tap advertised
///   * the ACCEPT covers the offer the payer actually saw
///   * the ACCEPT's amount matches the offer's
///   * the RECEIPT covers that ACCEPT and links to it as predecessor
pub fn verify_transcript(
    tap: &TapPresent,
    offer: &FullOffer,
    accept: &Accept,
    accept_bytes: &[u8],
    receipt: &Receipt,
) -> Result<(), Reject> {
    let offer_commit = offer.commitment();
    if !commit_eq(&offer_commit, &tap.offer_commit) {
        return Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "FullOffer does not match the tap's offer_commit",
        ));
    }
    if !commit_eq(&accept.offer_hash, &offer_commit) {
        return Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "ACCEPT does not cover this offer",
        ));
    }
    if accept.nonce != tap.nonce || offer.nonce_echo != tap.nonce {
        return Err(Reject::with_detail(
            RejectCode::Replay,
            "nonce does not match the bootstrap",
        ));
    }
    // The payer signs the amount its own client derived (§15.5); a receipt that
    // reports a different figure is the presenter rewriting history.
    if accept.amount_final != offer.amount_pxmr {
        return Err(Reject::with_detail(
            RejectCode::PriceMismatch,
            "ACCEPT amount differs from the offer",
        ));
    }
    let accept_hash = commit(Purpose::ChainLink, accept_bytes);
    if !commit_eq(&receipt.accept_hash, &accept_hash) || !commit_eq(&receipt.prev, &accept_hash) {
        return Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "RECEIPT does not chain to this ACCEPT",
        ));
    }
    if receipt.amount_final != accept.amount_final {
        return Err(Reject::with_detail(
            RejectCode::PriceMismatch,
            "RECEIPT amount differs from what the payer signed",
        ));
    }
    Ok(())
}

/// §15.7: a metered offer must carry a cap, because an open-ended obligation
/// cannot be consented to and §15.5's argument collapses without one.
///
/// Checked separately from `FullOffer::from_value` because it is a *pairing*
/// rule — it depends on the tap's `amount_authority`, which lives in a
/// different object. A client MUST run this before rendering a confirm screen.
pub fn check_meter_terms(tap: &TapPresent, offer: &FullOffer) -> Result<(), Reject> {
    if tap.amount_authority == AmountAuthority::Rated {
        if offer.terms.meter_cap_pxmr == 0 {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a rated offer must declare a meter cap (§15.7)",
            ));
        }
        if offer.terms.meter_max_s == 0 {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a rated offer must declare a maximum duration (§15.7)",
            ));
        }
    }
    Ok(())
}

/// §15.7: what a payee may claim when a meter is never stopped.
///
/// Returns the accrued amount, capped. Collection is a separate matter and
/// depends entirely on collateral — against an unbonded payer this figure is
/// uncollectable and the provider bears the loss, exactly as a bar bears a
/// walked tab.
pub fn abandoned_meter_claim(
    offer: &FullOffer,
    rate_pxmr_per_s: u64,
    elapsed_s: u64,
) -> u64 {
    let capped_time = elapsed_s.min(offer.terms.meter_max_s);
    let accrued = rate_pxmr_per_s.saturating_mul(capped_time);
    accrued.min(offer.terms.meter_cap_pxmr)
}

// ------------------------------------------------------------------ REFUND --

/// §7.3. A refund is not a reversal — A2's finality is a property of the
/// ledger, not a prohibition on commerce. It is a **new, voluntary payment**
/// bound to a prior receipt, payee-initiated only, and never compellable.
///
/// Building the clawback would mean building the arbiter that can seize funds,
/// which is precisely the party this protocol deletes. A customer refused a
/// refund has the recourse they have at a market stall: reputation (§9.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refund {
    pub version: u64,
    pub suite: u8,
    /// Commitment to the receipt being refunded.
    pub prior_receipt: [u8; 32],
    /// Partial or full. Never more than the original — checked separately,
    /// since the original amount lives in a different object.
    pub amount_pxmr: u64,
    /// The settling transaction. A refund that claims to have paid and has not
    /// is just a message.
    pub txid: [u8; 32],
    /// Where the refund was actually sent. Checked against the `refund_to` the
    /// payer signed, so a refund cannot be redirected after the fact.
    pub paid_to: Vec<u8>,
    pub timestamp: u64,
}

impl Refund {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::Refund)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::PRIOR_RECEIPT, Value::Bytes(self.prior_receipt.to_vec()));
        m.insert(f::REFUND_AMOUNT, Value::Uint(self.amount_pxmr));
        m.insert(f::REFUND_TXID, Value::Bytes(self.txid.to_vec()));
        m.insert(f::REFUND_PAID_TO, Value::Bytes(self.paid_to.clone()));
        m.insert(f::REFUND_TS, Value::Uint(self.timestamp));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::Refund) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not REFUND",
            ));
        }
        let out = Refund {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            prior_receipt: r.bytes(f::PRIOR_RECEIPT, Some(32))?.try_into().unwrap(),
            amount_pxmr: r.uint(f::REFUND_AMOUNT)?,
            txid: r.bytes(f::REFUND_TXID, Some(32))?.try_into().unwrap(),
            paid_to: r.bytes(f::REFUND_PAID_TO, None)?,
            timestamp: r.uint(f::REFUND_TS)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// Check a refund against the transaction it claims to refund.
///
/// Three rules, each a way a refund can be wrong that a signature alone will not
/// catch — the object is perfectly valid while referring to the wrong thing,
/// too much of it, or too late.
pub fn check_refund(
    refund: &Refund,
    original_receipt: &Receipt,
    original_receipt_bytes: &[u8],
    original_terms: &Terms,
    original_accept: &Accept,
) -> Result<(), Reject> {
    // 0. It must have gone where the payer said, and the payer must have said.
    //    Without this the destination comes from outside the signed transcript,
    //    which is the substitution a published attack on BIP-70 exploited.
    match &original_accept.refund_to {
        None => {
            return Err(Reject::with_detail(
                RejectCode::PolicyRefused,
                "this payer supplied no refund address, so no refund is payable",
            ))
        }
        Some(addr) if addr.as_slice() != refund.paid_to.as_slice() => {
            return Err(Reject::with_detail(
                RejectCode::PolicyRefused,
                "refund was sent somewhere other than the address the payer signed",
            ))
        }
        Some(_) => {}
    }

    // 1. It must name *this* receipt.
    let link = commit(Purpose::ChainLink, original_receipt_bytes);
    if !commit_eq(&refund.prior_receipt, &link) {
        return Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "refund does not reference this receipt",
        ));
    }

    // 2. Partial is fine; more than the original is not. A payee refunding more
    //    than was paid is either confused or draining someone's float.
    if refund.amount_pxmr > original_receipt.amount_final {
        return Err(Reject::with_detail(
            RejectCode::PriceMismatch,
            "refund exceeds the original amount",
        ));
    }

    // 3. §7.3's window, which the payer signed as part of `terms`. Without it a
    //    merchant carries an unbounded open liability; zero is legitimate and
    //    means final sale.
    let elapsed = refund.timestamp.saturating_sub(original_receipt.timestamp);
    if elapsed > original_terms.refund_window_s {
        return Err(Reject::with_detail(
            RejectCode::PolicyRefused,
            format!(
                "refund window closed: {} s elapsed, {} s allowed",
                elapsed, original_terms.refund_window_s
            ),
        ));
    }

    Ok(())
}

// ----------------------------------------------------------------- MANDATE --

/// §7.3. A payer-signed standing authorisation: a named payee may draw up to
/// `cap_pxmr` per `period_s` until revoked.
///
/// **This is the one place the protocol authorises payment without a
/// per-payment human checkpoint**, which §15.5 otherwise makes mandatory. The
/// checkpoint moves rather than disappearing: the human confirms *the cap and
/// the period*, once, and every later draw is bounded by what they saw. A
/// mandate with no cap would be a blank cheque and is not representable.
///
/// Two properties are non-negotiable and are the whole difference from a
/// card-network subscription: the cap is enforced by the **payer's own client**,
/// and revocation is unilateral — you stop honouring requests, and that is the
/// end of it. There is no cancellation flow to survive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mandate {
    pub version: u64,
    pub suite: u8,
    /// The persona authorised to draw. Bound to a persona pair (§16), not a
    /// session — a mandate outlives the transaction that created it.
    pub payee_persona: Vec<u8>,
    pub cap_pxmr: u64,
    pub period_s: u64,
    /// Absolute expiry. A mandate that never expires is one the user will
    /// forget they granted.
    pub expiry: u64,
    pub nonce: [u8; 16],
}

impl Mandate {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::Mandate)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::MANDATE_PAYEE, Value::Bytes(self.payee_persona.clone()));
        m.insert(f::MANDATE_CAP, Value::Uint(self.cap_pxmr));
        m.insert(f::MANDATE_PERIOD, Value::Uint(self.period_s));
        m.insert(f::MANDATE_EXPIRY, Value::Uint(self.expiry));
        m.insert(f::MANDATE_NONCE, Value::Bytes(self.nonce.to_vec()));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::Mandate) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not MANDATE",
            ));
        }
        let out = Mandate {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            payee_persona: r.bytes(f::MANDATE_PAYEE, None)?,
            cap_pxmr: r.uint(f::MANDATE_CAP)?,
            period_s: r.uint(f::MANDATE_PERIOD)?,
            expiry: r.uint(f::MANDATE_EXPIRY)?,
            nonce: r.bytes(f::MANDATE_NONCE, Some(16))?.try_into().unwrap(),
        };
        r.finish()?;
        // A capless or periodless mandate is a blank cheque. Refusing at parse
        // time means such an object cannot exist in a client's store at all.
        if out.cap_pxmr == 0 || out.period_s == 0 {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a mandate must declare a non-zero cap and period (§7.3)",
            ));
        }
        Ok(out)
    }
}

/// What a payer's client must track to enforce a mandate locally.
#[derive(Debug, Clone, Default)]
pub struct MandateUsage {
    /// Start of the current period, in the same epoch as `now`.
    pub period_start: u64,
    /// Drawn so far within the current period.
    pub drawn_pxmr: u64,
}

/// Authorise a draw against a mandate, or refuse it.
///
/// Run by the **payer's** client. §7.3 puts enforcement here deliberately: a cap
/// the payee enforces is not a cap, it is a promise.
pub fn check_mandate_draw(
    mandate: &Mandate,
    usage: &MandateUsage,
    requesting_persona: &[u8],
    amount_pxmr: u64,
    now: u64,
) -> Result<MandateUsage, Reject> {
    if now >= mandate.expiry {
        return Err(Reject::with_detail(
            RejectCode::Expired,
            "mandate has expired",
        ));
    }
    // Only the named persona may draw. Without this a mandate is bearer paper.
    if requesting_persona != mandate.payee_persona.as_slice() {
        return Err(Reject::with_detail(
            RejectCode::PolicyRefused,
            "this persona is not the one the mandate names",
        ));
    }

    // Roll the period forward if the last draw was in an earlier one. Periods
    // are anchored to the first draw rather than to a calendar, so there is no
    // timezone in the protocol and no midnight at which caps reset globally.
    let mut next = usage.clone();
    if usage.period_start == 0 || now.saturating_sub(usage.period_start) >= mandate.period_s {
        next.period_start = now;
        next.drawn_pxmr = 0;
    }

    let would_be = next.drawn_pxmr.saturating_add(amount_pxmr);
    if would_be > mandate.cap_pxmr {
        return Err(Reject::with_detail(
            RejectCode::PolicyRefused,
            format!(
                "draw would exceed the mandate cap: {} + {} > {}",
                next.drawn_pxmr, amount_pxmr, mandate.cap_pxmr
            ),
        ));
    }
    next.drawn_pxmr = would_be;
    Ok(next)
}

// ------------------------------------------------------- DISPUTE / RULING --

/// §9.3.1. The distinction that governs everything else about arbitration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DisputeClass {
    /// Decidable from the signed transcript plus public chain state — *did this
    /// confirm, does a conflicting key image exist*. The arbiter exercises no
    /// discretion and a wrong ruling is **provably** wrong.
    Mechanical = 0,
    /// Turns on facts neither party can prove — *was the room clean*. A wrong
    /// ruling here is not provable, only unpopular.
    Judgment = 1,
}

/// §9.3.2. Carries the transcript, not a story.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispute {
    pub version: u64,
    pub suite: u8,
    pub class: DisputeClass,
    /// Commitment to the transaction being disputed.
    pub transcript: [u8; 32],
    pub claim_pxmr: u64,
    pub timestamp: u64,
}

/// §9.3.2. A ruling is **not an instruction** — it is a co-signature. The
/// arbiter's authority is exactly its key in the relevant multisig, so it
/// cannot rule beyond funds it can already move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Outcome {
    ForClaimant = 0,
    ForRespondent = 1,
    /// Frivolous. §17.5: the claimant's own stake is at risk on dismissal, so
    /// providers cannot grief payers with bogus claims.
    Dismissed = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ruling {
    pub version: u64,
    pub suite: u8,
    pub dispute: [u8; 32],
    pub outcome: Outcome,
    pub award_pxmr: u64,
    pub timestamp: u64,
}

impl Dispute {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::Dispute)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::DISPUTE_CLASS, Value::Uint(self.class as u64));
        m.insert(f::DISPUTE_TRANSCRIPT, Value::Bytes(self.transcript.to_vec()));
        m.insert(f::DISPUTE_CLAIM_PXMR, Value::Uint(self.claim_pxmr));
        m.insert(f::DISPUTE_TS, Value::Uint(self.timestamp));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        // Checked, not discarded. Until 0.47 these five objects — every one added
        // after the original four — read the type field and threw it away, so a
        // second byte string differing only in its declared type decoded to the
        // same object. §18.3: anywhere the protocol admits two byte
        // representations of one value, it has a transcript-divergence bug.
        if r.uint(f::TYPE)? != type_code(ObjectType::Dispute) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not DISPUTE",
            ));
        }
        let out = Dispute {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            class: match r.uint(f::DISPUTE_CLASS)? {
                0 => DisputeClass::Mechanical,
                1 => DisputeClass::Judgment,
                n => {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        format!("no such dispute class: {}", n),
                    ))
                }
            },
            transcript: r.bytes(f::DISPUTE_TRANSCRIPT, Some(32))?.try_into().unwrap(),
            claim_pxmr: r.uint(f::DISPUTE_CLAIM_PXMR)?,
            timestamp: r.uint(f::DISPUTE_TS)?,
        };
        r.finish()?;
        Ok(out)
    }
}

impl Ruling {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::Ruling)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::RULING_DISPUTE, Value::Bytes(self.dispute.to_vec()));
        m.insert(f::RULING_OUTCOME, Value::Uint(self.outcome as u64));
        m.insert(f::RULING_AWARD, Value::Uint(self.award_pxmr));
        m.insert(f::RULING_TS, Value::Uint(self.timestamp));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        // Checked, not discarded. Until 0.47 these five objects — every one added
        // after the original four — read the type field and threw it away, so a
        // second byte string differing only in its declared type decoded to the
        // same object. §18.3: anywhere the protocol admits two byte
        // representations of one value, it has a transcript-divergence bug.
        if r.uint(f::TYPE)? != type_code(ObjectType::Ruling) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not RULING",
            ));
        }
        let out = Ruling {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            dispute: r.bytes(f::RULING_DISPUTE, Some(32))?.try_into().unwrap(),
            outcome: match r.uint(f::RULING_OUTCOME)? {
                0 => Outcome::ForClaimant,
                1 => Outcome::ForRespondent,
                2 => Outcome::Dismissed,
                n => {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        format!("no such outcome: {}", n),
                    ))
                }
            },
            award_pxmr: r.uint(f::RULING_AWARD)?,
            timestamp: r.uint(f::RULING_TS)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// Validate a ruling against the dispute it answers.
pub fn check_ruling(
    ruling: &Ruling,
    dispute: &Dispute,
    dispute_bytes: &[u8],
    arbiter_set: &[Vec<u8>],
    arbiter_persona: &[u8],
) -> Result<(), Reject> {
    // §2.5's lesson, in one check: the arbiter is whoever the *market
    // descriptor* says it is. A ruling from a key that is not in the signed set
    // is a stranger's opinion, however well-formed. RetoSwap was drained by
    // accepting an arbitrator's address from a message.
    if !arbiter_set.iter().any(|k| k.as_slice() == arbiter_persona) {
        return Err(Reject::with_detail(
            RejectCode::UntrustedArbiterSet,
            "ruling is from a persona outside the market's signed arbiter set",
        ));
    }
    if !commit_eq(&ruling.dispute, &commit(Purpose::ChainLink, dispute_bytes)) {
        return Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "ruling does not answer this dispute",
        ));
    }
    // An arbiter cannot award more than was claimed. It has no authority to
    // invent an obligation neither party asserted.
    if ruling.award_pxmr > dispute.claim_pxmr {
        return Err(Reject::with_detail(
            RejectCode::PriceMismatch,
            "award exceeds the amount claimed",
        ));
    }
    // A ruling for the respondent or a dismissal awards nothing; anything else
    // is an outcome disagreeing with its own award.
    if ruling.outcome != Outcome::ForClaimant && ruling.award_pxmr != 0 {
        return Err(Reject::with_detail(
            RejectCode::Malformed,
            "only a ruling for the claimant may carry an award",
        ));
    }
    Ok(())
}

/// §9.3.4: what an abandoned dispute produces.
///
/// The section said funds "return to the pre-dispute allocation, claim
/// abandoned". Under escrow that is a deadlock rather than a resolution: the
/// pre-dispute allocation *is* funds locked in a 2-of-3 awaiting a RELEASE that
/// two disagreeing parties will never co-sign. Doing nothing freezes them
/// permanently — the exact outcome §9.3.4 claims to avoid.
///
/// So expiry must produce an actual ruling. It resolves **for the respondent**,
/// which is a co-signature that moves the funds, not an absence of one.
pub fn expired_dispute_ruling(dispute: &Dispute, dispute_bytes: &[u8], now: u64) -> Ruling {
    Ruling {
        version: 1,
        suite: dispute.suite,
        dispute: commit(Purpose::ChainLink, dispute_bytes),
        outcome: Outcome::ForRespondent,
        award_pxmr: 0,
        timestamp: now,
    }
}

// -------------------------------------------------------------------- HAIL --

/// §5.2.1. What a consumer writes into a market's hail record.
///
/// **Note what is absent: there is no route field, and that is the design.**
/// §5.2's whole safety argument is that matching happens over DHT reads, which
/// import nothing, and that the single route import happens only after mutual
/// selection. If a `Hail` could carry a route, a provider could learn the
/// consumer's address merely by watching — which is the harvesting this
/// section was rewritten to eliminate.
///
/// Making the field unrepresentable is stronger than forbidding it. A rule can
/// go unimplemented; a missing field cannot be populated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hail {
    pub version: u64,
    pub suite: u8,
    pub profile: u64,
    /// Coarse geocell — a district, not a position (§5.2.3).
    pub geocell: Vec<u8>,
    pub nonce: [u8; 16],
    /// Ephemeral key that providers seal their replies to. Fresh per hail, so
    /// two hails from the same consumer are unlinkable to a watcher.
    pub ephemeral_pk: Vec<u8>,
    pub expiry: u64,
}

/// §5.2.1. A provider's reply, sealed to the consumer's ephemeral key.
///
/// Also carries **no route**, for the same reason and by the same means. The
/// provider learns the consumer's route only if selected, and only sealed to
/// its own key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HailReply {
    pub version: u64,
    pub suite: u8,
    /// Echoes the hail's nonce. Without this a provider's old reply could be
    /// replayed against a fresh hail, and a consumer would be choosing from
    /// quotes nobody currently stands behind.
    pub nonce_echo: [u8; 16],
    pub session_pk: Vec<u8>,
    pub quote_pxmr: u64,
}

impl Hail {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::Hail)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::PROFILE, Value::Uint(self.profile));
        m.insert(f::HAIL_GEOCELL, Value::Bytes(self.geocell.clone()));
        m.insert(f::NONCE, Value::Bytes(self.nonce.to_vec()));
        m.insert(f::HAIL_EPHEMERAL_PK, Value::Bytes(self.ephemeral_pk.clone()));
        m.insert(f::HAIL_EXPIRY, Value::Uint(self.expiry));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        // Checked, not discarded. Until 0.47 these five objects — every one added
        // after the original four — read the type field and threw it away, so a
        // second byte string differing only in its declared type decoded to the
        // same object. §18.3: anywhere the protocol admits two byte
        // representations of one value, it has a transcript-divergence bug.
        if r.uint(f::TYPE)? != type_code(ObjectType::Hail) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not HAIL",
            ));
        }
        let out = Hail {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            profile: r.uint(f::PROFILE)?,
            geocell: r.bytes(f::HAIL_GEOCELL, None)?,
            nonce: r.bytes(f::NONCE, Some(16))?.try_into().unwrap(),
            ephemeral_pk: r.bytes(f::HAIL_EPHEMERAL_PK, None)?,
            expiry: r.uint(f::HAIL_EXPIRY)?,
        };
        r.finish()?;
        // A coarse cell is the point (§5.2.3). An over-precise one turns the
        // disclosure ladder's first rung into a position fix, so precision is
        // bounded here rather than left to a client's discretion.
        if out.geocell.len() > 5 {
            return Err(Reject::with_detail(
                RejectCode::PolicyRefused,
                "geocell is too precise for a hail (§5.2.3)",
            ));
        }
        Ok(out)
    }
}

impl HailReply {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::HailReply)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::HAILREPLY_NONCE_ECHO, Value::Bytes(self.nonce_echo.to_vec()));
        m.insert(f::HAILREPLY_SESSION_PK, Value::Bytes(self.session_pk.clone()));
        m.insert(f::HAILREPLY_QUOTE, Value::Uint(self.quote_pxmr));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        // Checked, not discarded. Until 0.47 these five objects — every one added
        // after the original four — read the type field and threw it away, so a
        // second byte string differing only in its declared type decoded to the
        // same object. §18.3: anywhere the protocol admits two byte
        // representations of one value, it has a transcript-divergence bug.
        if r.uint(f::TYPE)? != type_code(ObjectType::HailReply) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not HAILREPLY",
            ));
        }
        let out = HailReply {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            nonce_echo: r.bytes(f::HAILREPLY_NONCE_ECHO, Some(16))?.try_into().unwrap(),
            session_pk: r.bytes(f::HAILREPLY_SESSION_PK, None)?,
            quote_pxmr: r.uint(f::HAILREPLY_QUOTE)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// A consumer's check before selecting a reply.
pub fn check_hail_reply(hail: &Hail, reply: &HailReply, now: u64) -> Result<(), Reject> {
    if now >= hail.expiry {
        return Err(Reject::with_detail(RejectCode::Expired, "hail has expired"));
    }
    // Without the echo, a provider's stale reply could be replayed against a
    // fresh hail and the consumer would be picking from quotes nobody
    // currently stands behind.
    if reply.nonce_echo != hail.nonce {
        return Err(Reject::with_detail(
            RejectCode::Replay,
            "reply does not echo this hail's nonce",
        ));
    }
    Ok(())
}

// ------------------------------------------------------------------ CANCEL --

/// §7.3. Cancellation after `ACCEPT` and before `FUND`, invoking the fee
/// schedule the payer already signed in `terms`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cancel {
    pub version: u64,
    pub suite: u8,
    /// The ACCEPT being cancelled.
    pub prior_accept: [u8; 32],
    /// Must equal `terms.cancellation_pxmr` — a cancelling party cannot invent
    /// a different figure than the one on the confirm screen.
    pub fee_pxmr: u64,
    pub timestamp: u64,
}

impl Cancel {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::Cancel)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::CANCEL_PRIOR, Value::Bytes(self.prior_accept.to_vec()));
        m.insert(f::CANCEL_FEE, Value::Uint(self.fee_pxmr));
        m.insert(f::TIMESTAMP, Value::Uint(self.timestamp));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::Cancel) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not CANCEL",
            ));
        }
        let out = Cancel {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            prior_accept: r.bytes(f::CANCEL_PRIOR, Some(32))?.try_into().unwrap(),
            fee_pxmr: r.uint(f::CANCEL_FEE)?,
            timestamp: r.uint(f::TIMESTAMP)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// A cancelling party cannot name a fee other than the one already agreed.
pub fn check_cancel(
    cancel: &Cancel,
    accept_bytes: &[u8],
    terms: &Terms,
) -> Result<(), Reject> {
    if !commit_eq(&cancel.prior_accept, &commit(Purpose::ChainLink, accept_bytes)) {
        return Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "cancellation does not reference this ACCEPT",
        ));
    }
    if cancel.fee_pxmr != terms.cancellation_pxmr {
        return Err(Reject::with_detail(
            RejectCode::PriceMismatch,
            "cancellation fee differs from the signed terms",
        ));
    }
    Ok(())
}

// -------------------------------------------------------------- TapStatic --

/// §15.9. A passive tag or printed code holding a receive-only capability.
///
/// **A different object type from `TapPresent`, and readers must never confuse
/// them**: there is no session key, no channel, no negotiation, and no
/// co-signed receipt. Whatever a payer sends here, they send into the dark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapStatic {
    pub version: u64,
    pub suite: u8,
    pub payto: Vec<u8>,
    /// Optional pinned persona. See `check_static_tag` for what this is and is
    /// not worth.
    pub persona: Option<Vec<u8>>,
    /// Signature by `persona` over the canonical body. Present only when a
    /// persona is pinned.
    pub sig: Option<Vec<u8>>,
}

impl TapStatic {
    /// The body a pinned persona signs: everything except the signature.
    pub fn signing_body(&self) -> Vec<u8> {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::TapStatic)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::STATIC_PAYTO, Value::Bytes(self.payto.clone()));
        if let Some(p) = &self.persona {
            m.insert(f::STATIC_PERSONA, Value::Bytes(p.clone()));
        }
        Value::Map(m).encode()
    }

    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::TapStatic)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::STATIC_PAYTO, Value::Bytes(self.payto.clone()));
        if let Some(p) = &self.persona {
            m.insert(f::STATIC_PERSONA, Value::Bytes(p.clone()));
        }
        if let Some(s) = &self.sig {
            m.insert(f::STATIC_SIG, Value::Bytes(s.clone()));
        }
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        // Checked, not discarded. Until 0.47 these five objects — every one added
        // after the original four — read the type field and threw it away, so a
        // second byte string differing only in its declared type decoded to the
        // same object. §18.3: anywhere the protocol admits two byte
        // representations of one value, it has a transcript-divergence bug.
        if r.uint(f::TYPE)? != type_code(ObjectType::TapStatic) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not TAPSTATIC",
            ));
        }
        let out = TapStatic {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            payto: r.bytes(f::STATIC_PAYTO, None)?,
            persona: r.opt_bytes(f::STATIC_PERSONA, None)?,
            sig: r.opt_bytes(f::STATIC_SIG, Some(64))?,
        };
        r.finish()?;
        // A signature without a persona names nobody, so it can prove nothing.
        if out.sig.is_some() && out.persona.is_none() {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a static tag signature requires a pinned persona",
            ));
        }
        Ok(out)
    }
}

/// How much a static tag is worth trusting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticTrust {
    /// No persona, no signature. **Nothing is authenticated.** The payer is
    /// trusting a physical object, and a swapped tag is undetectable.
    Anonymous,
    /// A persona is pinned and has signed the address, so `payto` provably
    /// belongs to that persona. This is worth something **only to a payer who
    /// knows independently which persona to expect** — a swapped tag carries
    /// the attacker's persona and a valid signature over it, and a first-time
    /// donor has nothing to compare against.
    SignedBy(Vec<u8>),
}

/// §15.9's mitigation, and its honest limit.
///
/// The section suggested pinning a persona and warning on an unrecognised one.
/// That is weaker than it sounds: an attacker who replaces the physical tag
/// replaces the persona too, so the warning only fires for a payer who has seen
/// the *expected* persona before or learned it out of band. For a stranger
/// tapping a donation box it fires never.
///
/// A signature at least closes the gap between "claims persona X" and "is
/// persona X". Without one, an attacker can print X's name over their own
/// address.
pub fn check_static_tag(
    tag: &TapStatic,
    verify: impl Fn(&[u8], &[u8], &[u8]) -> bool,
) -> Result<StaticTrust, Reject> {
    match (&tag.persona, &tag.sig) {
        (None, _) => Ok(StaticTrust::Anonymous),
        (Some(p), None) => {
            // A pinned persona with no signature is a claim, not evidence.
            // Treating it as authentication is the trap §15.9 walked into.
            let _ = p;
            Ok(StaticTrust::Anonymous)
        }
        (Some(p), Some(s)) => {
            if verify(p, &tag.signing_body(), s) {
                Ok(StaticTrust::SignedBy(p.clone()))
            } else {
                Err(Reject::new(RejectCode::BadSig))
            }
        }
    }
}
