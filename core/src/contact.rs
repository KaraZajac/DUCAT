//! Contacts before money, and messages after (§16.9, §16.10).
//!
//! §16.3 establishes contacts as a coda: identity is exchanged **after** a
//! receipt, bound by `H(RECEIPT) ‖ session_pk`, which proves *the persistent
//! identity I am handing you is the same entity you just transacted with*.
//!
//! That binding is unavailable to a card handed over in person or sent through
//! Signal, because there is no receipt. So an invitation proves strictly less,
//! and the honest thing is to say what less means:
//!
//! - **It proves key possession.** Whoever produced this card holds the persona
//!   key, because the card is signed by it.
//! - **It proves nothing about who handed it to you.** A card forwarded over a
//!   messaging app was authenticated by *that app*, not by DUCAT. Received over
//!   NFC, the authentication is that someone was standing in front of you.
//!
//! This is §15.9's lesson for the third time: a signature proves who owns a key,
//! never that the artifact carrying it is the one its author put there. The
//! difference from a static tag is that the channel is usually a person.
//!
//! # Single use, on purpose
//!
//! An invitation is claimable **once** and expires. A card that could be claimed
//! repeatedly is a standing offer to anyone who ever saw the message it arrived
//! in — a screenshot in a group chat, a forwarded DM, a backup of someone else's
//! phone. Claim-once means the issuer learns that it was used, and by whom.
//!
//! A public, reusable artifact is a different thing and already exists: a static
//! payment tag (§15.9), which receives money and establishes no relationship.

use std::collections::BTreeMap;

use crate::cbor::Value;
use crate::commit::{commit, commit_eq, Purpose};
use crate::reject::{Reject, RejectCode};
use crate::sig::ObjectType;
use crate::wire::{f, type_code, Reader};

/// The longest a display name may be.
///
/// Short enough to sit beside an amount without wrapping, and short enough that
/// it cannot smuggle a paragraph. A name is a handle, not a bio.
pub const MAX_DISPLAY_NAME_CHARS: usize = 32;

/// An out-of-band offer of contact (§16.9, §16.12).
///
/// Carries a **DHT record key**, not a route blob. A record key is permanent; a
/// route is a snapshot of a process, and §16.12 records what that cost.
///
/// The `writer_public` names the one other party allowed to write the inbox's
/// reply subkey. The matching secret travels with the card and never appears
/// here, so a card seen without its secret — in a log, in a screenshot of this
/// object — cannot be answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactCard {
    pub version: u64,
    pub suite: u8,
    /// The persona being offered. The card is signed by it.
    pub persona: Vec<u8>,
    /// The contact-request inbox: `SMPL(1, [writer])`.
    pub inbox_key: String,
    /// The writer this inbox admits. Whoever holds the matching secret can
    /// reply, and **Veilid enforces that** — it is not a check we perform and
    /// could get wrong.
    pub writer_public: Vec<u8>,
    /// Self-asserted, and worth exactly what the channel that carried it is
    /// worth. A petname the receiver assigns locally is the real name (§7.5).
    pub display_name: Option<String>,
    /// Absolute expiry. A card that never expires is a credential the issuer
    /// has forgotten they published.
    pub expiry: u64,
}

impl ContactCard {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::ContactOffer)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::CARD_PERSONA, Value::Bytes(self.persona.clone()));
        m.insert(f::CARD_INBOX, Value::Text(self.inbox_key.clone()));
        m.insert(f::CARD_WRITER, Value::Bytes(self.writer_public.clone()));
        if let Some(n) = &self.display_name {
            m.insert(f::CARD_NAME, Value::Text(n.clone()));
        }
        m.insert(f::CARD_EXPIRY, Value::Uint(self.expiry));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::ContactOffer) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not CONTACT_OFFER",
            ));
        }
        let out = ContactCard {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            persona: r.bytes(f::CARD_PERSONA, None)?,
            inbox_key: r
                .opt_text(f::CARD_INBOX, MAX_RECORD_KEY_CHARS)?
                .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "card has no inbox"))?,
            writer_public: r.bytes(f::CARD_WRITER, None)?,
            display_name: r.opt_text(f::CARD_NAME, MAX_DISPLAY_NAME_CHARS)?,
            expiry: r.uint(f::CARD_EXPIRY)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// A record key is `KIND:owner:value` in base64url. Bounded so a card cannot
/// smuggle a payload through a field a reader will treat as an address.
pub const MAX_RECORD_KEY_CHARS: usize = 128;

/// What each side writes into the inbox: subkey 0 the issuer, subkey 1 the
/// claimant. Identical in shape, because the exchange is symmetric — both are
/// saying "here is who I am and where to leave things for me".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactDetails {
    pub version: u64,
    pub suite: u8,
    pub persona: Vec<u8>,
    /// Where this party's messages will appear (§16.12). Their outbox, which
    /// only they write and anyone holding the key may read.
    pub outbox_key: String,
    /// Encoded `PreKeyBundle` (§16.11), so the first message needs no extra
    /// round trip — and can be written while this party is offline.
    pub prekey_bundle: Vec<u8>,
    pub display_name: Option<String>,
}

impl ContactDetails {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::ContactAccept)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::DET_PERSONA, Value::Bytes(self.persona.clone()));
        m.insert(f::DET_OUTBOX, Value::Text(self.outbox_key.clone()));
        m.insert(f::DET_BUNDLE, Value::Bytes(self.prekey_bundle.clone()));
        if let Some(n) = &self.display_name {
            m.insert(f::DET_NAME, Value::Text(n.clone()));
        }
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::ContactAccept) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not CONTACT_ACCEPT",
            ));
        }
        let out = ContactDetails {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            persona: r.bytes(f::DET_PERSONA, None)?,
            outbox_key: r
                .opt_text(f::DET_OUTBOX, MAX_RECORD_KEY_CHARS)?
                .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "no outbox"))?,
            prekey_bundle: r.bytes(f::DET_BUNDLE, None)?,
            display_name: r.opt_text(f::DET_NAME, MAX_DISPLAY_NAME_CHARS)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// Subkey 0 of an outbox: how far the log has been written.
///
/// A reader polls this one subkey to learn whether there is anything new, which
/// is one small read rather than a scan of the whole ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogHead {
    pub version: u64,
    pub suite: u8,
    /// The sequence number the *next* message will carry. Also the count of
    /// messages ever written, which is what makes a gap detectable.
    pub next_seq: u64,
    /// The publisher's **current** prekey bundle (§16.11).
    ///
    /// Carried here because the head is read on every poll anyway, so a
    /// refreshed bundle reaches every reader for no extra round trip — and
    /// because there is otherwise nowhere to put one. The handshake inbox is a
    /// one-time artifact and may be deleted, so a supply exhausted after it was
    /// read could never be replenished: the pair would stay on the signed
    /// prekey permanently, with forward secrecy quietly gone for good.
    pub prekey_bundle: Option<Vec<u8>>,
}

impl LogHead {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::LogHead)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::HEAD_NEXT, Value::Uint(self.next_seq));
        if let Some(b) = &self.prekey_bundle {
            m.insert(f::HEAD_BUNDLE, Value::Bytes(b.clone()));
        }
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::LogHead) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not LOG_HEAD",
            ));
        }
        let out = LogHead {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            next_seq: r.uint(f::HEAD_NEXT)?,
            prekey_bundle: r.opt_bytes(f::HEAD_BUNDLE, None)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// Which subkey a message with this sequence number occupies.
///
/// Subkey 0 is the head, so messages start at 1 and wrap. The ring means an old
/// message is overwritten rather than kept — which §16.11 wants anyway, since a
/// message is meant to stop being readable rather than accumulate.
pub fn subkey_for(seq: u64, subkey_count: u32) -> u32 {
    debug_assert!(subkey_count > 1, "a log needs a head and at least one slot");
    let slots = (subkey_count - 1) as u64;
    ((seq % slots) + 1) as u32
}

/// Whether a reader can still fetch `seq`, or the ring has passed it by.
pub fn still_in_ring(seq: u64, next_seq: u64, subkey_count: u32) -> bool {
    let slots = (subkey_count - 1) as u64;
    seq < next_seq && next_seq - seq <= slots
}

// ---------------------------------------------------------------------------
// §16.10 — messages
// ---------------------------------------------------------------------------

/// A message is 1:1 and bounded.
///
/// Larger than a memo because this is prose rather than a label, and still
/// bounded: an unbounded field on a channel that persists is a file transfer
/// nobody designed, with the storage and retention consequences of one.
pub const MAX_MESSAGE_CHARS: usize = 2000;

/// What a message is (§16.13).
///
/// Money in a conversation is not a separate channel: a request rides the same
/// sealed, chained, offline-tolerant log as the text around it, which is what
/// §16.12 was for. §15's tap cannot express this at all — it assumes both
/// parties are present, and the whole point of asking someone for money is that
/// they might be asleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// Ordinary text.
    Text = 0,
    /// "Please send me this much." Carries no authority whatsoever — it is a
    /// message, and the payer still decides at §15.5's confirm screen. A request
    /// that could move money would be a request that malware sends for you.
    PaymentRequest = 1,
    /// "I sent you this much", with a transaction to look for. Advisory: the
    /// recipient verifies by finding the output, never by believing the note.
    PaymentSent = 2,
}

impl MessageKind {
    fn from_code(v: u64) -> Option<Self> {
        Some(match v {
            0 => MessageKind::Text,
            1 => MessageKind::PaymentRequest,
            2 => MessageKind::PaymentSent,
            _ => return None,
        })
    }
}

/// One message on a persistent contact (§16.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub version: u64,
    pub suite: u8,
    /// Sender-local, strictly increasing. Two participants each keep their own
    /// sequence rather than sharing one, because a shared counter needs
    /// agreement and agreement needs a round trip that an offline sender does
    /// not have.
    pub seq: u64,
    /// Chain link to this sender's previous message, or zero for the first.
    /// Makes a dropped message detectable rather than merely absent.
    pub prev: [u8; 32],
    pub body: String,
    pub timestamp: u64,
    /// Text unless this is about money (§16.13).
    pub kind: MessageKind,
    /// Required for a payment kind, refused for text. Piconero, so a request is
    /// exact — a rounded amount in a message someone acts on is a rounding
    /// error somebody pays for.
    pub amount_pxmr: Option<u64>,
    /// The transaction, for `PaymentSent`. Advisory: §17.5 verifies by scanning
    /// for the output, and a txid in a message is a pointer, not evidence.
    pub txid: Option<Vec<u8>>,
}

impl Message {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::Message)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::MSG_SEQ, Value::Uint(self.seq));
        m.insert(f::MSG_PREV, Value::Bytes(self.prev.to_vec()));
        m.insert(f::MSG_BODY, Value::Text(self.body.clone()));
        m.insert(f::MSG_TS, Value::Uint(self.timestamp));
        if self.kind != MessageKind::Text {
            m.insert(f::MSG_KIND, Value::Uint(self.kind as u64));
        }
        if let Some(a) = self.amount_pxmr {
            m.insert(f::MSG_AMOUNT, Value::Uint(a));
        }
        if let Some(t) = &self.txid {
            m.insert(f::MSG_TXID, Value::Bytes(t.clone()));
        }
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::Message) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not MESSAGE",
            ));
        }
        let out = Message {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            seq: r.uint(f::MSG_SEQ)?,
            prev: r.bytes(f::MSG_PREV, Some(32))?.try_into().unwrap(),
            body: r
                .opt_text(f::MSG_BODY, MAX_MESSAGE_CHARS)?
                .ok_or_else(|| Reject::with_detail(RejectCode::Malformed, "message has no body"))?,
            timestamp: r.uint(f::MSG_TS)?,
            // Absent means text. Encoding the default would give one meaning two
            // encodings, which §18.1 refuses.
            kind: match r.opt_uint(f::MSG_KIND)? {
                None => MessageKind::Text,
                Some(0) => {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        "text is the default and must be encoded by omitting the kind",
                    ))
                }
                Some(v) => MessageKind::from_code(v).ok_or_else(|| {
                    Reject::with_detail(RejectCode::Malformed, "unknown message kind")
                })?,
            },
            amount_pxmr: r.opt_uint(f::MSG_AMOUNT)?,
            txid: r.opt_bytes(f::MSG_TXID, Some(32))?,
        };
        r.finish()?;
        // A payment with no amount is a payment screen with a blank on it, and
        // an amount on a text message is a number nothing will honour. Both are
        // refused rather than ignored.
        match (out.kind, out.amount_pxmr) {
            (MessageKind::Text, Some(_)) => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a text message must not carry an amount",
                ))
            }
            (MessageKind::PaymentRequest, None) | (MessageKind::PaymentSent, None) => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a payment message must carry an amount",
                ))
            }
            _ => {}
        }
        if out.txid.is_some() && out.kind != MessageKind::PaymentSent {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "only a payment notice carries a transaction",
            ));
        }
        Ok(out)
    }

    /// The link the next message from this sender must carry.
    pub fn link(&self) -> [u8; 32] {
        commit(Purpose::ChainLink, &self.to_value().encode())
    }
}

/// Accept a message into a sender's thread.
///
/// Refuses a gap rather than storing around it: a chat that silently skips a
/// message shows a conversation that did not happen, and the reader cannot tell.
pub fn check_message(
    msg: &Message,
    expected_seq: u64,
    previous: Option<&Message>,
) -> Result<(), Reject> {
    if msg.seq != expected_seq {
        return Err(Reject::with_detail(
            RejectCode::StateViolation,
            format!("expected message {expected_seq}, got {}", msg.seq),
        ));
    }
    match previous {
        None => {
            if msg.prev != [0u8; 32] {
                return Err(Reject::with_detail(
                    RejectCode::CommitMismatch,
                    "the first message links to nothing",
                ));
            }
        }
        Some(p) => {
            if !commit_eq(&msg.prev, &p.link()) {
                return Err(Reject::with_detail(
                    RejectCode::CommitMismatch,
                    "message does not follow the one before it",
                ));
            }
        }
    }
    Ok(())
}


// ---------------------------------------------------------------------------
// `ducat:` card URIs (§18.7)
// ---------------------------------------------------------------------------

pub const CARD_URI_PREFIX: &str = "ducat:card/";

/// Format a signed card and its **writer secret** as a shareable link.
///
/// The secret is the capability: it is what lets the holder write the inbox's
/// reply subkey, and Veilid enforces that rather than this code checking it.
/// Which is why it travels beside the card and never inside it — the signed
/// object can be logged or screenshotted without becoming answerable.
///
/// Lives in `core` rather than in the bridge because the harness and the app
/// both need it, and two base64 implementations that disagree on padding is
/// precisely the class of divergence this project keeps finding by accident.
pub fn card_to_uri(envelope: &[u8], writer_secret: &[u8; 32]) -> String {
    format!("{CARD_URI_PREFIX}{}.{}", b64(envelope), b64(writer_secret))
}

/// Parse one. Returns the signed envelope and the writer secret.
pub fn card_from_uri(input: &str) -> Result<(Vec<u8>, [u8; 32]), Reject> {
    let t = input.trim();
    let rest = t.strip_prefix(CARD_URI_PREFIX).ok_or_else(|| {
        Reject::with_detail(RejectCode::Malformed, "not a DUCAT contact link")
    })?;
    // `rsplit_once`, not `split_once`: base64 never contains a period, but a
    // link pasted with trailing prose might, and the secret is the last field.
    let (a, b) = rest.rsplit_once('.').ok_or_else(|| {
        Reject::with_detail(RejectCode::Malformed, "contact link is incomplete")
    })?;
    let env = unb64(a)?;
    let secret: [u8; 32] = unb64(b)?.try_into().map_err(|_| {
        Reject::with_detail(RejectCode::Malformed, "writer secret is not 32 bytes")
    })?;
    Ok((env, secret))
}

/// URL-safe base64, no padding. Hand-rolled rather than adding a dependency for
/// forty lines; padding is omitted because a card travels in a URI.
fn b64(b: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((*c.get(1).unwrap_or(&0) as u32) << 8)
            | (*c.get(2).unwrap_or(&0) as u32);
        for i in 0..c.len() + 1 {
            out.push(A[((n >> (18 - 6 * i)) & 0x3F) as usize] as char);
        }
    }
    out
}

fn unb64(s: &str) -> Result<Vec<u8>, Reject> {
    let bad = || Reject::with_detail(RejectCode::Malformed, "contact link is not valid base64");
    let val = |c: u8| -> Result<u32, Reject> {
        Ok(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'-' => 62,
            b'_' => 63,
            _ => return Err(bad()),
        })
    };
    let mut out = Vec::new();
    for c in s.as_bytes().chunks(4) {
        if c.len() == 1 {
            return Err(bad());
        }
        let mut n = 0u32;
        for (i, &ch) in c.iter().enumerate() {
            n |= val(ch)? << (18 - 6 * i);
        }
        for i in 0..c.len() - 1 {
            out.push(((n >> (16 - 8 * i)) & 0xFF) as u8);
        }
    }
    Ok(out)
}

/// The AAD binding a ciphertext to one conversation (§16.11).
///
/// **Symmetric by construction.** The first implementation used "the other
/// party's persona", which reads correctly on each side and evaluates to a
/// different value on each side — A sealed under B's key and B opened under A's,
/// so nothing ever decrypted. Sorting the pair gives both ends the same bytes
/// without either needing to know who started the conversation.
pub fn thread_aad(one: &str, other: &str) -> Vec<u8> {
    let mut pair = [one, other];
    pair.sort_unstable();
    pair.join(":").into_bytes()
}
