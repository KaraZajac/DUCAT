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

    /// Nested terms map (§7.3, §15.7, §8.8). Its inner keys are their own
    /// namespace, defined in `terms`.
    pub const TERMS: u64 = 96;

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

fn type_code(t: ObjectType) -> u64 {
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
struct Reader {
    m: BTreeMap<u64, Value>,
}

impl Reader {
    fn new(v: Value) -> Result<Self, Reject> {
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

    fn uint(&mut self, k: u64) -> Result<u64, Reject> {
        self.m
            .remove(&k)
            .ok_or_else(|| Self::missing(k))?
            .as_uint()
            .ok_or_else(|| Self::wrong(k))
    }

    fn bytes(&mut self, k: u64, len: Option<usize>) -> Result<Vec<u8>, Reject> {
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

    fn opt_bytes(&mut self, k: u64, len: Option<usize>) -> Result<Option<Vec<u8>>, Reject> {
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
    fn finish(self) -> Result<(), Reject> {
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
