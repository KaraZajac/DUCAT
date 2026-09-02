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
    /// Where this party can be paid without asking first.
    ///
    /// **Optional, and a real trade.** A stored address is a reused address,
    /// and a reused address is a public ledger entry linking every payment
    /// anyone ever made to this person. §16.13's per-request destination avoids
    /// that and should be preferred when the two sides can wait for a request.
    ///
    /// It exists because the alternative — "ask them to send a request first"
    /// — is a wall in front of the ordinary case of paying someone you already
    /// talk to. Publishing it is the contact's own choice about their own
    /// linkability, and an implementation MUST let them decline.
    pub payto: Option<String>,
    /// A small picture, so a contact list is faces rather than hex.
    ///
    /// **Hard-bounded, and format-checked.** These are attacker-supplied bytes
    /// handed to an image decoder on someone else's phone, which is one of the
    /// most reliably exploitable surfaces there is. The bound is small enough
    /// that this is an avatar and not a file transfer, and the magic-number
    /// check means a decoder is only ever asked to parse what it was told it
    /// was getting.
    pub avatar: Option<Vec<u8>>,
    /// Optional contact details, each **validated on the wire** (§16.9).
    ///
    /// Validated here rather than at the screen, because these render as
    /// identity: a "phone number" that is really a sentence, or an "email" that
    /// is really a lookalike domain with control characters in it, is a claim
    /// about who someone is being drawn by a client that did not check. A field
    /// nobody validates is a field that says whatever the sender wants.
    pub email: Option<String>,
    pub phone: Option<String>,
    pub signal: Option<String>,
    pub pronouns: Option<Pronouns>,
    /// The car, for a rider scanning a curb full of strangers (§15.12). Each
    /// is a claim like the rest of the profile — the plate on the screen must
    /// match the plate on the bumper, and that check is the rider's.
    pub car_model: Option<String>,
    pub car_color: Option<String>,
    pub plate: Option<String>,
    /// What this handshake is *for* — "profile" for a standing contact code,
    /// "sale"/"hail"/"tab"/… for a transaction (§16.9).
    ///
    /// It travels so the party answering the card can scope what *they* reveal
    /// to the moment: a plate belongs on a hail and a phone number belongs in a
    /// contact exchange, but neither has any business riding a bar tab. The
    /// issuer stamps it; the claimant reads it and trims its reply accordingly.
    /// None (an older record, or a card that did not say) is treated as the
    /// most private case — reveal nothing optional beyond a name.
    pub purpose: Option<String>,
}

/// How to refer to someone.
///
/// A closed set rather than free text, because this is drawn next to a name on
/// a stranger's screen and a free-text field there is a place to put a message.
///
/// The cost is real and worth stating: a closed list cannot express every
/// pronoun anyone uses, and someone whose pronouns are not here has only
/// [`Pronouns::Any`] or absence. Absence is not a failure state — a client MUST
/// render a person with no pronouns set exactly as it renders anyone else, and
/// MUST NOT substitute a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pronouns {
    SheHer = 1,
    SheThey = 2,
    HeHim = 3,
    HeThey = 4,
    TheyThem = 5,
    Any = 6,
}

impl Pronouns {
    fn from_code(v: u64) -> Option<Self> {
        Some(match v {
            1 => Pronouns::SheHer,
            2 => Pronouns::SheThey,
            3 => Pronouns::HeHim,
            4 => Pronouns::HeThey,
            5 => Pronouns::TheyThem,
            6 => Pronouns::Any,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Pronouns::SheHer => "she/her",
            Pronouns::SheThey => "she/they",
            Pronouns::HeHim => "he/him",
            Pronouns::HeThey => "he/they",
            Pronouns::TheyThem => "they/them",
            Pronouns::Any => "any",
        }
    }
}

/// The most an avatar may weigh.
///
/// Sized for a contact-list thumbnail, not a photograph. It also has to fit in
/// a DHT subkey alongside everything else in this record, and a profile that
/// does not fit is a contact who cannot be reached at all.
pub const MAX_AVATAR_BYTES: usize = 12 * 1024;

/// How far ahead a board notice may claim to be good for.
///
/// board.rs prices flooding by making each slot cost a search, and its own
/// cost model depends on the notices *expiring*: "a region of a hundred cells
/// costs a couple of hours — repeated as notices expire." Without a ceiling
/// that repetition never comes. Every reader tests only `expiry > now`, and
/// every writer skips a slot that is still live, so 128 correctly signed and
/// correctly paid-for notices dated to the year 2100 fill a cell's sixteen
/// shards for good: nobody can hail from that corner again, and every sweep
/// renders ghosts that never age out. `clear_own_slot` quite rightly refuses
/// to erase somebody else's notice, so no honest client ever reclaims one.
///
/// Thirty-one days. A hail lives ten minutes and a listing a day, so this is
/// far above any real use and far below "for ever".
///
/// Checked by the *reader*, not at decode, and deliberately: decoding must not
/// depend on the clock. The conformance vectors pin exact bytes to exact
/// outcomes, and a decode that consulted the time would start failing on its
/// own one day with nothing changed. Every reader already tests `expiry > now`
/// to drop a stale notice; the ceiling is the other half of that same test.
pub const MAX_NOTICE_TTL_SECS: u64 = 31 * 24 * 60 * 60;

pub const MAX_EMAIL_CHARS: usize = 254;
pub const MAX_PHONE_DIGITS: usize = 15;
pub const MAX_SIGNAL_CHARS: usize = 48;
/// "Toyota Corolla" fits; a paragraph does not. These render beside a name on
/// a stranger's screen, which is exactly where a free-form field becomes a
/// message board.
pub const MAX_CAR_MODEL_CHARS: usize = 24;
pub const MAX_CAR_COLOR_CHARS: usize = 16;
pub const MAX_PLATE_CHARS: usize = 12;
/// Long enough for the handshake kinds a client sends ("profile", "sale",
/// "hail", "tab", "intro"); short enough that it is a tag and not a payload.
pub const MAX_PURPOSE_CHARS: usize = 16;

/// `local@domain.tld`, deliberately strict.
///
/// Not RFC 5322 — that grammar admits quoted strings, comments and characters
/// that no client should be rendering as an identity. This accepts the shape
/// people actually have and refuses the rest, which is the right trade for a
/// field whose only job is to be displayed and copied.
fn email_is_plausible(s: &str) -> bool {
    let bytes = s.as_bytes();
    if s.len() > MAX_EMAIL_CHARS || s.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let Some(at) = s.find('@') else { return false };
    if s[at + 1..].contains('@') {
        return false;
    }
    let (local, domain) = (&s[..at], &s[at + 1..]);
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    let local_ok = local.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-' | '\'' )
    }) && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..");
    // A domain needs a dot and a real TLD; `a@b` is not an address anyone has.
    let Some(dot) = domain.rfind('.') else { return false };
    let tld = &domain[dot + 1..];
    let domain_ok = domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !domain.starts_with(['.', '-'])
        && !domain.ends_with(['.', '-'])
        && !domain.contains("..")
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic());
    let _ = bytes;
    local_ok && domain_ok
}

/// Digits only.
///
/// No punctuation, no `+`, no spaces: one number has a dozen spellings, and a
/// field that accepts all of them is a field two clients will render two ways
/// and neither will match when someone searches. The country code is digits
/// too, so nothing is lost.
fn phone_is_plausible(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_PHONE_DIGITS
        && s.chars().all(|c| c.is_ascii_digit())
}

/// Signal's own shape: a username, a dot, then digits.
fn signal_is_plausible(s: &str) -> bool {
    if s.len() > MAX_SIGNAL_CHARS {
        return false;
    }
    let Some((name, digits)) = s.split_once('.') else { return false };
    // Signal requires at least three characters and at least two digits.
    name.len() >= 3
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && digits.len() >= 2
        && digits.chars().all(|c| c.is_ascii_digit())
}

/// What an avatar's first bytes must say it is.
///
/// PNG, JPEG or WebP. Checked because a decoder should never be handed bytes
/// whose format it has to guess, and because "it is whatever it turns out to
/// be" is how a picture becomes an exploit.
fn avatar_format_is_known(b: &[u8]) -> bool {
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    b.starts_with(PNG)
        || b.starts_with(&[0xFF, 0xD8, 0xFF])
        || (b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP")
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
        if let Some(p) = &self.payto {
            m.insert(f::DET_PAYTO, Value::Text(p.clone()));
        }
        if let Some(a) = &self.avatar {
            m.insert(f::DET_AVATAR, Value::Bytes(a.clone()));
        }
        if let Some(e) = &self.email {
            m.insert(f::DET_EMAIL, Value::Text(e.clone()));
        }
        if let Some(p) = &self.phone {
            m.insert(f::DET_PHONE, Value::Text(p.clone()));
        }
        if let Some(sg) = &self.signal {
            m.insert(f::DET_SIGNAL, Value::Text(sg.clone()));
        }
        if let Some(p) = self.pronouns {
            m.insert(f::DET_PRONOUNS, Value::Uint(p as u64));
        }
        if let Some(v) = &self.car_model {
            m.insert(f::DET_CAR_MODEL, Value::Text(v.clone()));
        }
        if let Some(v) = &self.car_color {
            m.insert(f::DET_CAR_COLOR, Value::Text(v.clone()));
        }
        if let Some(v) = &self.plate {
            m.insert(f::DET_PLATE, Value::Text(v.clone()));
        }
        if let Some(v) = &self.purpose {
            m.insert(f::DET_PURPOSE, Value::Text(v.clone()));
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
            payto: r.opt_text(f::DET_PAYTO, MAX_ADDRESS_CHARS)?,
            avatar: r.opt_bytes(f::DET_AVATAR, None)?,
            email: r.opt_text(f::DET_EMAIL, MAX_EMAIL_CHARS)?,
            phone: r.opt_text(f::DET_PHONE, MAX_PHONE_DIGITS)?,
            signal: r.opt_text(f::DET_SIGNAL, MAX_SIGNAL_CHARS)?,
            pronouns: match r.opt_uint(f::DET_PRONOUNS)? {
                None => None,
                Some(v) => Some(Pronouns::from_code(v).ok_or_else(|| {
                    Reject::with_detail(RejectCode::Malformed, "unknown pronouns code")
                })?),
            },
            car_model: r.opt_text(f::DET_CAR_MODEL, MAX_CAR_MODEL_CHARS)?,
            car_color: r.opt_text(f::DET_CAR_COLOR, MAX_CAR_COLOR_CHARS)?,
            plate: r.opt_text(f::DET_PLATE, MAX_PLATE_CHARS)?,
            purpose: r.opt_text(f::DET_PURPOSE, MAX_PURPOSE_CHARS)?,
        };
        r.finish()?;
        for (v, what) in [
            (&out.car_model, "car model"),
            (&out.car_color, "car colour"),
            (&out.plate, "plate"),
        ] {
            if let Some(t) = v {
                if t.is_empty() || t.chars().any(|c| c.is_control()) {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        match what {
                            "car model" => "a car model is short plain text",
                            "car colour" => "a car colour is short plain text",
                            _ => "a plate is short plain text",
                        },
                    ));
                }
            }
        }
        if let Some(a) = &out.avatar {
            if a.is_empty() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "an empty avatar is not an avatar; omit the key instead",
                ));
            }
            if a.len() > MAX_AVATAR_BYTES {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    format!("an avatar may be at most {MAX_AVATAR_BYTES} bytes"),
                ));
            }
            if !avatar_format_is_known(a) {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "an avatar must be PNG, JPEG or WebP",
                ));
            }
        }
        if out.email.as_deref().is_some_and(|e| !email_is_plausible(e)) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "that is not the shape of an email address",
            ));
        }
        if out.phone.as_deref().is_some_and(|p| !phone_is_plausible(p)) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a phone number is digits only, country code included",
            ));
        }
        if out.signal.as_deref().is_some_and(|s| !signal_is_plausible(s)) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a Signal username is name.digits",
            ));
        }
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
    /// How far into the *peer's* log this publisher has displayed (§16.16):
    /// "I have shown your messages below this sequence to my user."
    ///
    /// On the head rather than in a message, deliberately: the head is
    /// rewritten constantly anyway, so a read watermark costs no ring slot,
    /// no prekey, and no chain entry — a receipt as a message would spend all
    /// three per glance. Absent means the publisher does not send read
    /// receipts, and §16.16 makes that the default: when a message is read is
    /// behavioural data, and it leaves the device only by explicit opt-in.
    pub read_up_to: Option<u64>,
    /// The ring's subkey count, head included (§16.12).
    ///
    /// Carried so it can *change*: the original eight was sized for text, and
    /// reactions and receipts multiply message count. Readers MUST take the
    /// ring from the head rather than assuming a constant — the failure of a
    /// mismatch is reading the wrong slot and refusing a valid thread. Absent
    /// means eight, the size every log had before this field existed.
    pub ring: Option<u32>,
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
        if let Some(r) = self.read_up_to {
            m.insert(f::HEAD_READ, Value::Uint(r));
        }
        if let Some(r) = self.ring {
            m.insert(f::HEAD_RING, Value::Uint(r as u64));
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
            read_up_to: r.opt_uint(f::HEAD_READ)?,
            ring: match r.opt_uint(f::HEAD_RING)? {
                None => None,
                // Eight is the default and MUST be encoded by omission (§18.1),
                // and a ring needs a head plus at least one slot.
                Some(8) => {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        "eight is the default ring and is encoded by omitting the field",
                    ))
                }
                Some(v) if !(2..=1024).contains(&v) => {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        "a ring is 2..=1024 subkeys",
                    ))
                }
                Some(v) => Some(v as u32),
            },
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

/// The longest a Monero address may be. Integrated addresses are 106
/// characters; the bound leaves room without letting the field carry a payload.
pub const MAX_ADDRESS_CHARS: usize = 128;

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
    /// An emoji on an existing message (§16.14): "this, about that one".
    ///
    /// A message like any other — sealed, chained, sequenced — because a
    /// side-channel for reactions would be a second delivery path with its own
    /// bugs. The body carries the emoji; the target is named by sequence.
    Reaction = 4,
    /// "Here is what you paid me for" — issued by the party who *received* the
    /// money.
    ///
    /// A separate kind because it is a different claim, and neither existing one
    /// can make it. A vendor sending `PaymentSent` would be stating they sent
    /// money, which is false; a `PaymentRequest` after the fact would be asking
    /// again. What a receipt says is: I have your payment, and this is the
    /// breakdown it settles.
    ///
    /// Advisory, like everything else here. §17.5 verifies a payment by finding
    /// the output; a receipt is the vendor's account of what it was *for*, and
    /// the chain records amounts and never reasons.
    Receipt = 3,
    /// "The message at `re_seq` is withdrawn." One mechanism for two moments:
    /// with `re_own` the sender cancels their own earlier message (a bill
    /// nobody should pay now), without it they decline the counterparty's (an
    /// offer refused). Advisory like everything here — a retracted bill's
    /// button goes dead in the UI; no money moves or un-moves.
    Retract = 5,
    /// A driver's terms for a claimed hail (§15.12): the fare, and optionally
    /// how far away they are. The claim opened the channel; this message is
    /// the application. Nothing is owed until the accept.
    RideOffer = 6,
    /// The rider's yes, naming the offer it answers and echoing its fare —
    /// binding the acceptance to a price, so "accepted" can never mean a
    /// number neither party said.
    RideAccept = 7,
    /// A round of the interactive DKG that builds a bond or escrow's threshold
    /// key (§17.9). Carries an opaque `payload` the threshold library reads
    /// and DUCAT does not, a `round` tag, and a `ceremony_id` binding it to
    /// one escrow. Neither party ever holds the other's share; the message is
    /// how the PedPoP rounds cross the §16.12 thread.
    DkgRound = 8,
    /// A round of FROST signing that releases a bond or escrow (§17.9): the
    /// deposit returned, or an arbiter's RULING executed. Same three fields.
    /// A signer MUST verify the transaction's destination before it signs —
    /// a co-signature is §15.5's consent, to a specific place for the money.
    FrostRound = 9,
    /// This ceremony is abandoned; its state may be discarded. "Nothing
    /// happens" is never safe (§9.3.4), so an aborted build says so rather
    /// than leaving the other side waiting for a round that never comes.
    CeremonyAbort = 10,
    /// A reference to a live-position stream (§15.12): a DHT record and the
    /// key to read it, sealed into the thread once after a `RideAccept`. The
    /// stream itself is not messages — it is one record overwritten in place,
    /// a *now* with no past — so this message only hands over the pointer.
    /// MUST NOT be sent before a `RideAccept` exists in the thread.
    PositionRef = 11,
    /// A group's member list (§16.19), carried in `payload`: the group id,
    /// its name, and every member's persona. The creator's first roster *is*
    /// the invitation; a member adding someone sends the grown set to
    /// everyone including the newcomer. Rosters only grow — removal would
    /// need a consensus a peer-to-peer group cannot have, so the set is
    /// grow-only and every view converges by union, in any order.
    GroupRoster = 12,
    /// §16.20: a publication period's content key, handed down the paid
    /// thread — with the shelf itself (record + standing head key) on the
    /// first delivery. The message that turns a settled bill into readable
    /// content; it carries a capability, never content, so the thread stays
    /// small while the shelf holds the weight.
    PublicationKey = 13,
    /// "Pick up — here is the door" (§16.21). The offer carries a fresh
    /// private-route blob and a call id; media flows as app messages on
    /// that route, never through the mailbox. Ringing is a message, so
    /// missed calls are simply messages you read later.
    CallOffer = 14,
    /// The other half: the callee's own route and the echoed id. Declining
    /// is §16.13's Retract naming the offer, hanging up is stopping.
    CallAnswer = 15,
}

impl MessageKind {
    fn from_code(v: u64) -> Option<Self> {
        Some(match v {
            0 => MessageKind::Text,
            1 => MessageKind::PaymentRequest,
            2 => MessageKind::PaymentSent,
            3 => MessageKind::Receipt,
            4 => MessageKind::Reaction,
            5 => MessageKind::Retract,
            6 => MessageKind::RideOffer,
            7 => MessageKind::RideAccept,
            8 => MessageKind::DkgRound,
            9 => MessageKind::FrostRound,
            10 => MessageKind::CeremonyAbort,
            11 => MessageKind::PositionRef,
            12 => MessageKind::GroupRoster,
            13 => MessageKind::PublicationKey,
            14 => MessageKind::CallOffer,
            15 => MessageKind::CallAnswer,
            _ => return None,
        })
    }
}

/// Longest a line item's description may be.
///
/// Short on purpose. This is a word for a thing on a bill — "large flat white",
/// "2 × shoes" — not a place to put a paragraph, and a receipt has to render on
/// a phone held at a counter.
pub const MAX_ITEM_CHARS: usize = 64;

/// Most line items one message may carry.
///
/// A bound because a receipt is displayed, and an unbounded list is a rendering
/// job someone else's device has to do on your say-so.
pub const MAX_ITEMS: usize = 64;

/// One line on a bill (§16.13).
///
/// Deliberately just a description and an amount. No quantity field: "2 × shoes"
/// is a description, and a separate `qty` would give a single line two encodings
/// — `qty: 1` and omitted — which is the ambiguity §18.1 exists to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineItem {
    pub description: String,
    pub amount_pxmr: u64,
}

impl LineItem {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::ITEM_DESC, Value::Text(self.description.clone()));
        m.insert(f::ITEM_AMOUNT, Value::Uint(self.amount_pxmr));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        let description = r.opt_text(f::ITEM_DESC, MAX_ITEM_CHARS)?.ok_or_else(|| {
            Reject::with_detail(RejectCode::Malformed, "a line item needs a description")
        })?;
        let amount_pxmr = r.uint(f::ITEM_AMOUNT)?;
        r.finish()?;
        Ok(LineItem { description, amount_pxmr })
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
    /// Where to pay, for `PaymentRequest`.
    ///
    /// Carried in the request rather than kept on the contact, for two reasons.
    /// A stored address is reused, and a reused address is a public ledger entry
    /// linking every payment anyone ever made to that person. And a request that
    /// names its own destination is self-contained: the payer needs nothing from
    /// a record that may be stale.
    ///
    /// **This does not make the address trustworthy.** Nothing in DUCAT binds a
    /// Monero address to a persona, so a compromised contact can ask you to pay
    /// a stranger. §15.5's confirm screen must show it.
    pub payto: Option<String>,
    /// What the money is for, line by line (§16.13). Empty means not itemised.
    ///
    /// **Not the network fee.** A Monero fee is paid by the sender to the
    /// network, not by the payer to the vendor, so a fee line inside a bill
    /// charges it twice: once in the total requested and again when the payer's
    /// wallet builds the transaction. There is deliberately no field for it —
    /// the payer's own wallet knows what the transfer cost and is the only
    /// party that can state it truthfully.
    pub items: Vec<LineItem>,
    /// Tax, if any, on top of the items. Only meaningful alongside them.
    pub tax_pxmr: Option<u64>,
    /// For a `Reaction`: the sequence of the message reacted to (§16.14).
    /// Refers to the **recipient's** outbox unless [`Self::re_own`].
    pub re_seq: Option<u64>,
    /// Present when the reaction targets the *sender's own* earlier message.
    pub re_own: bool,
    /// For a `RideOffer`: how far away the driver is, in seconds (§15.12).
    /// A courtesy figure the rider weighs, not a promise anything enforces.
    pub eta_secs: Option<u64>,
    /// §17.9 ceremony payload — serialized threshold-library bytes DUCAT
    /// carries but does not parse. Present on `DkgRound`/`FrostRound` only.
    pub payload: Option<Vec<u8>>,
    /// Which round of the ceremony this is; the reader refuses one it did not
    /// expect (§2.5: out-of-order ceremony messages are never applied).
    pub round: Option<u64>,
    /// The 32-byte per-escrow context binding every ceremony message to one
    /// multisig, so a stale message cannot replay into a live ceremony.
    pub ceremony_id: Option<[u8; 32]>,
    /// A file or picture, by reference (§16.15).
    ///
    /// The bytes live in their own DHT record — up to 32 chunks of 32 KiB,
    /// Veilid's measured per-subkey and per-record caps — encrypted under a
    /// key that travels *here*, inside the sealed message, so the record on
    /// the network is noise to everyone but the thread. The message stays
    /// small; the ring stays a ring.
    pub attachment: Option<Attachment>,
    /// A live-position stream, by reference (§15.12). Present only on a
    /// `PositionRef`.
    pub position: Option<PositionRef>,
    /// §16.20: a publication period's key. Present only on a
    /// `PublicationKey`, where it is mandatory.
    pub publication: Option<PublicationKey>,
    /// §16.21: the door a call offer or answer opens.
    pub call: Option<CallRef>,
    /// §16.19: which group this message belongs to — 16 random bytes minted
    /// at creation. Present with [`Self::group_seq`] or not at all.
    pub group_id: Option<Vec<u8>>,
    /// The sender's own counter within the group. (sender, group_seq) is the
    /// one name a group message has that every member can resolve: the same
    /// body fans out into N pairwise threads and takes a different thread
    /// sequence in each, so `seq` stops naming anything shared.
    pub group_seq: Option<u64>,
    /// A reference to another group message: its sender's persona…
    pub group_re_sender: Option<Vec<u8>>,
    /// …and that sender's group counter. The group's own re_seq — the
    /// pairwise one is meaningless here (see [`Self::group_seq`]).
    pub group_re_seq: Option<u64>,
}

/// The pointer to a live-position stream (§15.12).
///
/// Like an [`Attachment`], the payload lives in its own DHT record and the
/// key to read it travels inside the sealed message — so the record on the
/// network is noise to anyone who was not a party. Unlike an attachment, the
/// record is a *single subkey overwritten in place*: the stream has a now and
/// no past by construction, which is the whole point (a chat history that
/// doubled as a movement log would be §5.2.3's surveillance database rebuilt
/// inside the E2EE).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionRef {
    /// The record whose subkey 0 the sender overwrites each cadence.
    pub record_key: String,
    /// XChaCha20-Poly1305 key for the stream, one per ride, never reused —
    /// reuse would make the key a long-lived identifier linking rides.
    pub stream_key: [u8; 32],
}

/// Longest a call-route blob may be: a measured default-config blob is
/// 832 bytes; past this something is being smuggled that is not a route.
pub const MAX_CALL_ROUTE: usize = 4096;

/// A live call's door (§16.21): the private route to stream media to and
/// the eight random bytes both halves quote so an answer names its offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRef {
    pub route: Vec<u8>,
    pub id: [u8; 8],
}

/// A publication period's key, with the shelf on first delivery (§16.20).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationKey {
    /// The publisher's own label for the period — "2026-09", "issue-12".
    /// What the reader files the key under; never parsed for meaning.
    pub period_id: String,
    /// The period's content key. Opaque here: the publisher derives it
    /// (core::publish), the reader only holds and uses it.
    pub period_key: [u8; 32],
    /// The publication's root record — present with [`Self::head_key`] on
    /// the first delivery, optional after (the reader already has it).
    pub record_key: Option<String>,
    /// The standing key that opens the shelf's index for the life of the
    /// subscription. Travels with the record or not at all.
    pub head_key: Option<[u8; 32]>,
    /// A heavy period ships by swarm (§16.20): the share key to bootstrap
    /// from, with the index digest that authenticates what answers —
    /// together or not at all, and only aboard a publication key.
    pub swarm_key: Option<String>,
    pub swarm_digest: Option<[u8; 32]>,
}

/// A sealed blob parked in a DHT record (§16.15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// The record holding the ciphertext chunks, subkey 0 upward.
    /// The sealed blob chunked on a DHT record — the small road
    /// (≤ [MAX_ATTACHMENT_BYTES]). Exactly one transport is present.
    pub record_key: Option<String>,
    /// The sealed blob as a swarm share — the big road (§16.20's engine,
    /// ≤ [MAX_SWARM_ATTACHMENT_BYTES]). Key and digest travel together.
    pub swarm_key: Option<String>,
    pub swarm_digest: Option<[u8; 32]>,
    /// XChaCha20-Poly1305 key, one per attachment, never reused.
    pub key: [u8; 32],
    pub nonce: [u8; 24],
    /// Plaintext length, so a fetcher can size and bound before decrypting.
    pub len: u64,
    /// SHA-256 of the ciphertext: fetch, hash, *then* decrypt — bytes from
    /// the network never reach the AEAD without matching the hash the sealed
    /// message promised.
    pub ct_hash: [u8; 32],
    /// What the bytes are, so the receiver knows what decoder they feed.
    pub mime: String,
    /// A filename, when the sender had one worth keeping.
    pub name: Option<String>,
}

/// An attachment may not out-size its record: 32 chunks of 32 KiB is Veilid's
/// 1 MiB record cap, and the AEAD tag rides inside it.
pub const MAX_ATTACHMENT_BYTES: u64 = 1_048_576 - 64;
/// The swarm transport's bound (§16.15 post-1.0): a share carries what a
/// record cannot. Generous on the wire; clients bound their own seals.
pub const MAX_SWARM_ATTACHMENT_BYTES: u64 = 268_435_456;
/// A share key is "VLD0:<key>:<owner>" — two encoded keys, not one.
pub const MAX_SHARE_KEY_CHARS: usize = 128;
pub const MAX_MIME_CHARS: usize = 64;
pub const MAX_FILENAME_CHARS: usize = 96;

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
        if let Some(p) = &self.payto {
            m.insert(f::MSG_PAYTO, Value::Text(p.clone()));
        }
        if !self.items.is_empty() {
            m.insert(
                f::MSG_ITEMS,
                Value::Array(self.items.iter().map(|i| i.to_value()).collect()),
            );
        }
        if let Some(t) = self.tax_pxmr {
            m.insert(f::MSG_TAX, Value::Uint(t));
        }
        if let Some(r) = self.re_seq {
            m.insert(f::MSG_RE_SEQ, Value::Uint(r));
        }
        if self.re_own {
            m.insert(f::MSG_RE_OWN, Value::Uint(1));
        }
        if let Some(e) = self.eta_secs {
            m.insert(f::MSG_ETA, Value::Uint(e));
        }
        if let Some(p) = &self.payload {
            m.insert(f::MSG_PAYLOAD, Value::Bytes(p.clone()));
        }
        if let Some(r) = self.round {
            m.insert(f::MSG_ROUND, Value::Uint(r));
        }
        if let Some(c) = &self.ceremony_id {
            m.insert(f::MSG_CEREMONY, Value::Bytes(c.to_vec()));
        }
        if let Some(g) = &self.group_id {
            m.insert(f::MSG_GROUP_ID, Value::Bytes(g.clone()));
        }
        if let Some(g) = self.group_seq {
            m.insert(f::MSG_GROUP_SEQ, Value::Uint(g));
        }
        if let Some(g) = &self.group_re_sender {
            m.insert(f::MSG_GROUP_RE_SENDER, Value::Bytes(g.clone()));
        }
        if let Some(g) = self.group_re_seq {
            m.insert(f::MSG_GROUP_RE_SEQ, Value::Uint(g));
        }
        if let Some(a) = &self.attachment {
            if let Some(rk) = &a.record_key {
                m.insert(f::MSG_ATT_RECORD, Value::Text(rk.clone()));
            }
            if let Some(sk) = &a.swarm_key {
                m.insert(f::MSG_ATT_SWARM, Value::Text(sk.clone()));
            }
            if let Some(d) = &a.swarm_digest {
                m.insert(f::MSG_ATT_SWARM_DIGEST, Value::Bytes(d.to_vec()));
            }
            m.insert(f::MSG_ATT_KEY, Value::Bytes(a.key.to_vec()));
            m.insert(f::MSG_ATT_NONCE, Value::Bytes(a.nonce.to_vec()));
            m.insert(f::MSG_ATT_LEN, Value::Uint(a.len));
            m.insert(f::MSG_ATT_HASH, Value::Bytes(a.ct_hash.to_vec()));
            m.insert(f::MSG_ATT_MIME, Value::Text(a.mime.clone()));
            if let Some(n) = &a.name {
                m.insert(f::MSG_ATT_NAME, Value::Text(n.clone()));
            }
        }
        if let Some(p) = &self.position {
            m.insert(f::MSG_POS_RECORD, Value::Text(p.record_key.clone()));
            m.insert(f::MSG_POS_STREAM, Value::Bytes(p.stream_key.to_vec()));
        }
        if let Some(p) = &self.publication {
            m.insert(f::MSG_PUB_PERIOD, Value::Text(p.period_id.clone()));
            m.insert(f::MSG_PUB_KEY, Value::Bytes(p.period_key.to_vec()));
            if let Some(rk) = &p.record_key {
                m.insert(f::MSG_PUB_RECORD, Value::Text(rk.clone()));
            }
            if let Some(hk) = &p.head_key {
                m.insert(f::MSG_PUB_HEAD, Value::Bytes(hk.to_vec()));
            }
            if let Some(sk) = &p.swarm_key {
                m.insert(f::MSG_PUB_SWARM_KEY, Value::Text(sk.clone()));
            }
            if let Some(sd) = &p.swarm_digest {
                m.insert(f::MSG_PUB_SWARM_DIGEST, Value::Bytes(sd.to_vec()));
            }
        }
        if let Some(c) = &self.call {
            m.insert(f::MSG_CALL_ROUTE, Value::Bytes(c.route.clone()));
            m.insert(f::MSG_CALL_ID, Value::Bytes(c.id.to_vec()));
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
            payto: r.opt_text(f::MSG_PAYTO, MAX_ADDRESS_CHARS)?,
            items: match r.opt_array(f::MSG_ITEMS)? {
                None => Vec::new(),
                // Present-but-empty is a second spelling of "not itemised", and
                // omitting the key is the first. §18.1 allows one.
                Some(a) if a.is_empty() => {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        "an empty item list is not itemisation; omit the key instead",
                    ))
                }
                Some(a) if a.len() > MAX_ITEMS => {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        format!("a bill may carry at most {MAX_ITEMS} items"),
                    ))
                }
                Some(a) => a
                    .into_iter()
                    .map(LineItem::from_value)
                    .collect::<Result<Vec<_>, _>>()?,
            },
            tax_pxmr: r.opt_uint(f::MSG_TAX)?,
            group_id: r.opt_bytes(f::MSG_GROUP_ID, Some(16))?.map(|b| b.to_vec()),
            group_seq: r.opt_uint(f::MSG_GROUP_SEQ)?,
            group_re_sender: r
                .opt_bytes(f::MSG_GROUP_RE_SENDER, Some(32))?
                .map(|b| b.to_vec()),
            group_re_seq: r.opt_uint(f::MSG_GROUP_RE_SEQ)?,
            re_seq: r.opt_uint(f::MSG_RE_SEQ)?,
            eta_secs: r.opt_uint(f::MSG_ETA)?,
            payload: r.opt_bytes(f::MSG_PAYLOAD, None)?.map(|b| b.to_vec()),
            round: r.opt_uint(f::MSG_ROUND)?,
            ceremony_id: match r.opt_bytes(f::MSG_CEREMONY, Some(32))? {
                Some(b) => Some(b.try_into().unwrap()),
                None => None,
            },
            re_own: match r.opt_uint(f::MSG_RE_OWN)? {
                None => false,
                // One meaning, one encoding: presence is the flag (§18.1).
                Some(1) => true,
                Some(_) => {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        "re_own is a presence flag and may only be 1",
                    ))
                }
            },
            attachment: {
                let record_key = r.opt_text(f::MSG_ATT_RECORD, MAX_RECORD_KEY_CHARS)?;
                let swarm_key = r.opt_text(f::MSG_ATT_SWARM, MAX_SHARE_KEY_CHARS)?;
                let swarm_digest = r.opt_bytes(f::MSG_ATT_SWARM_DIGEST, Some(32))?;
                let key = r.opt_bytes(f::MSG_ATT_KEY, Some(32))?;
                let nonce = r.opt_bytes(f::MSG_ATT_NONCE, Some(24))?;
                let len = r.opt_uint(f::MSG_ATT_LEN)?;
                let ct_hash = r.opt_bytes(f::MSG_ATT_HASH, Some(32))?;
                let mime = r.opt_text(f::MSG_ATT_MIME, MAX_MIME_CHARS)?;
                let name = r.opt_text(f::MSG_ATT_NAME, MAX_FILENAME_CHARS)?;
                // The swarm pair travels together or not at all.
                let swarm = match (swarm_key, swarm_digest) {
                    (None, None) => None,
                    (Some(k), Some(d)) => Some((k, d)),
                    _ => {
                        return Err(Reject::with_detail(
                            RejectCode::Malformed,
                            "a swarm attachment carries its share key and digest together",
                        ))
                    }
                };
                let any_core = key.is_some()
                    || nonce.is_some()
                    || len.is_some()
                    || ct_hash.is_some()
                    || mime.is_some();
                match (record_key, swarm) {
                    (None, None) => {
                        if any_core || name.is_some() {
                            return Err(Reject::with_detail(
                                RejectCode::Malformed,
                                "attachment fields without a transport reference nothing",
                            ));
                        }
                        None
                    }
                    (Some(_), Some(_)) => {
                        return Err(Reject::with_detail(
                            RejectCode::Malformed,
                            "one road for the bytes: a record or the swarm, never both",
                        ))
                    }
                    (record_key, swarm) => {
                        // All or nothing: a partial attachment is a reference
                        // that can be fetched but not decrypted, or decrypted
                        // but not verified — every subset is a trap.
                        let (key, nonce, len, ct_hash, mime) =
                            match (key, nonce, len, ct_hash, mime) {
                                (Some(k), Some(n), Some(l), Some(h), Some(m)) => {
                                    (k, n, l, h, m)
                                }
                                _ => {
                                    return Err(Reject::with_detail(
                                        RejectCode::Malformed,
                                        "an attachment carries transport, key, nonce, length, hash and mime together",
                                    ))
                                }
                            };
                        let bound = if swarm.is_some() {
                            MAX_SWARM_ATTACHMENT_BYTES
                        } else {
                            MAX_ATTACHMENT_BYTES
                        };
                        if len == 0 || len > bound {
                            return Err(Reject::with_detail(
                                RejectCode::Malformed,
                                format!("an attachment is 1..={bound} bytes"),
                            ));
                        }
                        let (swarm_key, swarm_digest) = match swarm {
                            Some((k, d)) => {
                                (Some(k), Some(d.try_into().unwrap()))
                            }
                            None => (None, None),
                        };
                        Some(Attachment {
                            record_key,
                            swarm_key,
                            swarm_digest,
                            key: key.try_into().unwrap(),
                            nonce: nonce.try_into().unwrap(),
                            len,
                            ct_hash: ct_hash.try_into().unwrap(),
                            mime,
                            name,
                        })
                    }
                }
            },
            position: {
                let record_key = r.opt_text(f::MSG_POS_RECORD, MAX_RECORD_KEY_CHARS)?;
                let stream_key = r.opt_bytes(f::MSG_POS_STREAM, Some(32))?;
                match (record_key, stream_key) {
                    (None, None) => None,
                    (Some(record_key), Some(stream_key)) => Some(PositionRef {
                        record_key,
                        stream_key: stream_key.try_into().unwrap(),
                    }),
                    // Both or neither, §16.15's rule: a reference with no key
                    // cannot be opened, a key with no record points nowhere.
                    _ => {
                        return Err(Reject::with_detail(
                            RejectCode::Malformed,
                            "a position reference carries its record and its key together",
                        ))
                    }
                }
            },
            publication: {
                let period_id = r.opt_text(f::MSG_PUB_PERIOD, crate::publish::MAX_PERIOD_ID)?;
                let period_key = r.opt_bytes(f::MSG_PUB_KEY, Some(32))?;
                let record_key = r.opt_text(f::MSG_PUB_RECORD, MAX_RECORD_KEY_CHARS)?;
                let head_key = r.opt_bytes(f::MSG_PUB_HEAD, Some(32))?;
                let swarm_key = r.opt_text(f::MSG_PUB_SWARM_KEY, MAX_RECORD_KEY_CHARS)?;
                let swarm_digest = r.opt_bytes(f::MSG_PUB_SWARM_DIGEST, Some(32))?;
                // The swarm pair is one thing, and it rides a publication:
                // a bootstrap key without the digest that authenticates its
                // answers is an ask, not a fetch — and either of them away
                // from a period key describes a shipment of nothing.
                let swarm = match (swarm_key, swarm_digest) {
                    (None, None) => None,
                    (Some(k), Some(d)) => Some((k, d)),
                    _ => {
                        return Err(Reject::with_detail(
                            RejectCode::Malformed,
                            "a swarm share carries its key and its index digest together",
                        ))
                    }
                };
                match (period_id, period_key, record_key, head_key) {
                    (None, None, None, None) if swarm.is_none() => None,
                    (None, None, None, None) => {
                        return Err(Reject::with_detail(
                            RejectCode::Malformed,
                            "a swarm share rides a publication key",
                        ))
                    }
                    // The period pair is the kind's whole point: a key with
                    // no name cannot be filed, a name with no key opens
                    // nothing.
                    (Some(period_id), Some(period_key), record_key, head_key) => {
                        // Emptiness is already refused below the field layer:
                        // opt_text treats present-but-empty as a second
                        // encoding of "omitted" (§18.1) and rejects it.
                        // The shelf reference is one thing: the record and
                        // the head key that opens its index, together or
                        // not at all.
                        let shelf = match (record_key, head_key) {
                            (None, None) => (None, None),
                            (Some(rk), Some(hk)) => (Some(rk), Some(hk)),
                            _ => {
                                return Err(Reject::with_detail(
                                    RejectCode::Malformed,
                                    "a publication shelf carries its record and its head key together",
                                ))
                            }
                        };
                        Some(PublicationKey {
                            period_id,
                            period_key: period_key.try_into().unwrap(),
                            record_key: shelf.0,
                            head_key: shelf.1.map(|h: Vec<u8>| h.try_into().unwrap()),
                            swarm_key: swarm.as_ref().map(|(k, _)| k.clone()),
                            swarm_digest: swarm.map(|(_, d)| d.try_into().unwrap()),
                        })
                    }
                    _ => {
                        return Err(Reject::with_detail(
                            RejectCode::Malformed,
                            "a publication key carries its period id and its key together",
                        ))
                    }
                }
            },
            call: {
                let route = r.opt_bytes(f::MSG_CALL_ROUTE, None)?;
                let id = r.opt_bytes(f::MSG_CALL_ID, Some(8))?;
                match (route, id) {
                    (None, None) => None,
                    (Some(route), Some(id)) => {
                        // A route is a real blob with a real ceiling: empty
                        // opens no door, oversize is not a route.
                        if route.is_empty() || route.len() > MAX_CALL_ROUTE {
                            return Err(Reject::with_detail(
                                RejectCode::Malformed,
                                "a call route is 1 to 4096 bytes",
                            ));
                        }
                        Some(CallRef { route, id: id.try_into().unwrap() })
                    }
                    // Both or neither: a door with no name cannot be
                    // answered, a name with no door opens nothing.
                    _ => {
                        return Err(Reject::with_detail(
                            RejectCode::Malformed,
                            "a call carries its route and its id together",
                        ))
                    }
                }
            },
        };
        r.finish()?;
        // A payment with no amount is a payment screen with a blank on it, and
        // an amount on a text message is a number nothing will honour. Both are
        // refused rather than ignored.
        match (out.kind, out.amount_pxmr) {
            // FrostRound is deliberately absent from this arm: a release
            // proposal (round 0) MAY carry the amount it claims the funder
            // gets back — the consent screen states it beside the signed
            // payload (§15.12's settlement). Rounds that answer carry none,
            // and nothing verifies the claim but the eventual chain — it is
            // a statement, not authority, like every number in §16.13.
            (MessageKind::Text, Some(_))
            | (MessageKind::Retract, Some(_))
            | (MessageKind::DkgRound, Some(_))
            | (MessageKind::CeremonyAbort, Some(_))
            | (MessageKind::GroupRoster, Some(_))
            | (MessageKind::PublicationKey, Some(_))
            | (MessageKind::CallOffer, Some(_))
            | (MessageKind::CallAnswer, Some(_)) => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "this message kind must not carry an amount",
                ))
            }
            (MessageKind::PaymentRequest, None)
            | (MessageKind::PaymentSent, None)
            | (MessageKind::Receipt, None) => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a payment message must carry an amount",
                ))
            }
            // An offer without a fare offers nothing; an accept without the
            // fare it echoes binds the rider to a number neither party said.
            (MessageKind::RideOffer, None) | (MessageKind::RideAccept, None) => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a ride message must carry the fare",
                ))
            }
            _ => {}
        }
        // A notice points at the transaction it made; a receipt points at the
        // transaction it acknowledges. A request cannot point at either without
        // claiming the payment it is simultaneously asking for.
        if out.txid.is_some()
            && out.kind != MessageKind::PaymentSent
            && out.kind != MessageKind::Receipt
        {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "only a notice or a receipt carries a transaction",
            ));
        }
        if out.payto.is_some() && out.kind != MessageKind::PaymentRequest {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "only a request names where to pay",
            ));
        }
        if matches!(
            out.kind,
            MessageKind::Text
                | MessageKind::Retract
                | MessageKind::RideOffer
                | MessageKind::RideAccept
                | MessageKind::DkgRound
                | MessageKind::FrostRound
                | MessageKind::CeremonyAbort
        ) && (!out.items.is_empty() || out.tax_pxmr.is_some())
        {
            // The ride's bill comes later, through §15.11's meter; a retract
            // withdraws a bill rather than being one.
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "this message kind has no bill to itemise",
            ));
        }
        // Tax only alongside items, so that itemisation is *always* arithmetic
        // anyone can check. A tax line with nothing to tax states a split of a
        // total the message never breaks down, which is a number the recipient
        // has to take on faith — and a bill nobody can check is a bill that can
        // say anything.
        if out.tax_pxmr.is_some() && out.items.is_empty() {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "tax needs items to be tax on",
            ));
        }
        // §16.19: group fields travel together, and only where a group can.
        if out.group_id.is_some() != out.group_seq.is_some() {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a group message carries its group and its own counter together",
            ));
        }
        if out.group_re_sender.is_some() != out.group_re_seq.is_some() {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a group reference names a sender and their counter together",
            ));
        }
        if out.group_re_sender.is_some() && out.group_id.is_none() {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a group reference rides only a group message",
            ));
        }
        if out.group_id.is_some() {
            // Words, remarks about words, withdrawals of words, and the
            // roster. Money stays pairwise: a bill "to a group" is N debts
            // wearing one number, and every settlement rail here — requests,
            // receipts, escrow — is pairwise or a ceremony. RideOffer and the
            // ceremony kinds are two-party by construction.
            if !matches!(
                out.kind,
                MessageKind::Text
                    | MessageKind::Reaction
                    | MessageKind::Retract
                    | MessageKind::GroupRoster
            ) {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "this kind of message does not travel in a group",
                ));
            }
            // One meaning, one encoding (§18.1): in a group the target is the
            // group reference, because the pairwise sequence names a slot in
            // one thread and the same fanned-out message sits at a different
            // slot in every other.
            if out.re_seq.is_some() || out.re_own {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a group message targets by group reference, not thread sequence",
                ));
            }
        }
        if out.kind == MessageKind::GroupRoster {
            if out.group_id.is_none() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a roster names its group",
                ));
            }
            // A roster answers nothing; it is the membership, stated.
            if out.group_re_sender.is_some() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a roster does not target another message",
                ));
            }
        }
        // §16.14: a reaction is an emoji about a message, and nothing else.
        if out.kind == MessageKind::Reaction {
            if out.re_seq.is_none() && out.group_re_seq.is_none() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a reaction names the message it is about",
                ));
            }
            if out.body.chars().count() > 16 {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a reaction's body is the emoji, not a message",
                ));
            }
            if out.amount_pxmr.is_some() || out.attachment.is_some() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a reaction carries no money and no attachment",
                ));
            }
        } else if matches!(out.kind, MessageKind::Retract | MessageKind::RideAccept) {
            if out.re_seq.is_none() && out.group_re_seq.is_none() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a retract or an accept names the message it answers",
                ));
            }
            // Accepting your own offer is not a ceremony, it is a soliloquy.
            if out.kind == MessageKind::RideAccept && out.re_own {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "an accept answers the counterparty's offer",
                ));
            }
        } else if !matches!(
            out.kind,
            MessageKind::Text | MessageKind::PaymentSent | MessageKind::Receipt
        ) && (out.re_seq.is_some() || out.re_own)
        {
            // Three kinds *must* name a target (above); three *may* (here);
            // the rest may not.
            //
            // **A reply, and the two money messages that answer something.**
            // The field has carried "this, about that one" since reactions;
            // what changed is who is allowed to say it. A text answering a
            // text is an ordinary reply. A `PaymentSent` naming the
            // `PaymentRequest` it settles, and a `Receipt` naming the request
            // it receipts, turn a relationship that used to be *inferred* into
            // one that is stated — and the inference was wrong in a way that
            // showed: with no back-reference the only thread from a payment to
            // its bill was the amount, so two identical bills answered by one
            // payment both read as paid.
            //
            // Still advisory, like every other claim in a message. A payment
            // naming a request does not make the money arrive; §17.5 verifies
            // by finding the output. What the reference settles is *which*
            // request the sender says it was for, which is a question the
            // chain has never been able to answer.
            //
            // Not constrained by direction. `re_own` on a reply is somebody
            // answering their own earlier message, which people do; on a
            // payment it is "the £20 I said I would send", naming the sentence
            // rather than the bill. Only an accept forbids it, because
            // accepting your own offer is a soliloquy.
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "this kind of message does not target another",
            ));
        }
        // An eta is a RideOffer's courtesy figure and nothing else's — and a
        // day is the honest ceiling for "on my way".
        if let Some(e) = out.eta_secs {
            if out.kind != MessageKind::RideOffer {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "only a ride offer carries an eta",
                ));
            }
            if e > 86_400 {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "an eta longer than a day is not an eta",
                ));
            }
        }
        // A position reference is a PositionRef's whole content and nothing
        // else's, and a PositionRef with no reference is an empty gesture.
        // The gate that it MUST NOT precede a RideAccept is the sender's and
        // the reader's (they hold the thread; this decoder sees one message),
        // exactly as the "no offer before a claim" rule lives above the wire.
        match (out.kind, out.position.is_some()) {
            (MessageKind::PositionRef, false) => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a position message carries a reference to the stream",
                ))
            }
            (k, true) if k != MessageKind::PositionRef => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "only a position message carries a stream reference",
                ))
            }
            _ => {}
        }
        // §16.20's rule, the same closed world: the key IS the kind. A
        // publication message with nothing to hand over is an empty gesture,
        // and a period key on any other kind is a capability smuggled where
        // no reader is looking for one.
        match (out.kind, out.publication.is_some()) {
            (MessageKind::PublicationKey, false) => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a publication message carries the period's key",
                ))
            }
            (k, true) if k != MessageKind::PublicationKey => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "only a publication message carries a period key",
                ))
            }
            _ => {}
        }
        // §16.21, the same closed world again: the door IS the kind. An
        // offer or answer with no route rings nothing, and a route on any
        // other kind is a door held open where no call is happening.
        let call_kind = matches!(out.kind, MessageKind::CallOffer | MessageKind::CallAnswer);
        match (call_kind, out.call.is_some()) {
            (true, false) => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a call message carries its route and id",
                ))
            }
            (false, true) => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "only a call message carries a call route",
                ))
            }
            _ => {}
        }
        // §17.9 ceremony fields ride only on ceremony kinds. A payload or a
        // ceremony_id anywhere else is a field with no meaning to act on, and
        // a build/release message MUST carry both — the round bytes and the
        // context that binds them to one escrow. Abort carries the context so
        // the far side knows *which* ceremony died, but no payload.
        let is_round = matches!(out.kind, MessageKind::DkgRound | MessageKind::FrostRound);
        if is_round {
            if out.payload.as_ref().map(|p| p.is_empty()).unwrap_or(true) {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a ceremony round carries a payload",
                ));
            }
            if let Some(p) = &out.payload {
                if p.len() as u64 > MAX_ATTACHMENT_BYTES {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        "a ceremony payload is bounded like an attachment",
                    ));
                }
            }
            if out.round.is_none() || out.ceremony_id.is_none() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a ceremony round names its round and its escrow",
                ));
            }
        } else if out.kind == MessageKind::CeremonyAbort {
            if out.ceremony_id.is_none() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "an abort names the ceremony it ends",
                ));
            }
            if out.payload.is_some() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "an abort withdraws a ceremony; it carries no round payload",
                ));
            }
        } else if out.kind == MessageKind::GroupRoster {
            // §16.19: the member list rides the payload — it is structure,
            // not prose — bounded like a ceremony round's, and the ceremony's
            // own fields stay off it: a roster is not a round.
            if out.payload.as_ref().map(|p| p.is_empty()).unwrap_or(true) {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a roster carries its member list",
                ));
            }
            if let Some(p) = &out.payload {
                if p.len() as u64 > MAX_ATTACHMENT_BYTES {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        "a roster is bounded like an attachment",
                    ));
                }
            }
            if out.round.is_some() || out.ceremony_id.is_some() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a roster is not a ceremony round",
                ));
            }
        } else if out.payload.is_some() || out.round.is_some() || out.ceremony_id.is_some() {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "only a ceremony message carries ceremony fields",
            ));
        }
        // §16.15: attachments ride ordinary messages. A bill or a receipt with
        // a file in it is two features fused at their least-tested corner.
        if out.attachment.is_some() && out.kind != MessageKind::Text {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "only a text message carries an attachment",
            ));
        }
        if !out.items.is_empty() {
            let mut subtotal: u64 = 0;
            for i in &out.items {
                subtotal = subtotal.checked_add(i.amount_pxmr).ok_or_else(|| {
                    Reject::with_detail(RejectCode::Malformed, "item amounts overflow")
                })?;
            }
            let total = subtotal
                .checked_add(out.tax_pxmr.unwrap_or(0))
                .ok_or_else(|| {
                    Reject::with_detail(RejectCode::Malformed, "bill total overflows")
                })?;
            // The invariant that makes itemisation worth carrying. Without it a
            // bill is decoration next to an amount, and the two can disagree —
            // which is the one way an itemised receipt is worse than none,
            // because it looks like a check that was never performed.
            if Some(total) != out.amount_pxmr {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "the items and tax do not add up to the amount",
                ));
            }
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

// ---------------------------------------------------------------------------
// §16.15 — attachment sealing
// ---------------------------------------------------------------------------

/// Seal attachment bytes under a one-use key.
///
/// Symmetric, not HPKE: the key travels inside the already-sealed message, so
/// the thread's forward secrecy covers it, and the record on the network is
/// noise to everyone who was not a party. XChaCha because the nonce is random
/// and 24 bytes makes random safe; the same construction §4.3's backup uses.
pub fn attachment_seal(key: &[u8; 32], nonce: &[u8; 24], plaintext: &[u8]) -> Vec<u8> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .encrypt(XNonce::from_slice(nonce), plaintext)
        .expect("XChaCha encrypt is infallible for in-memory buffers")
}

/// Open sealed attachment bytes. The caller MUST have verified the ciphertext
/// hash first (§16.15): bytes from the network never reach the AEAD unchecked.
pub fn attachment_open(
    key: &[u8; 32],
    nonce: &[u8; 24],
    ciphertext: &[u8],
) -> Result<Vec<u8>, Reject> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| Reject::with_detail(RejectCode::BadSig, "attachment did not authenticate"))
}

/// Longest `card` URI a notice may carry (§16.17). A real card is ~500 bytes;
/// the cap is generous headroom, not an invitation — a board subkey is 32 KiB
/// and a notice that fills it is an attack, not a hail.
pub const MAX_HAIL_CARD_CHARS: usize = 1024;
/// Longest `dest` (§16.17): 64 bytes of human words, no coordinates by
/// construction — the place a notice can say is the cell it is pinned to.
pub const MAX_HAIL_DEST_CHARS: usize = 64;

/// A hail notice (§16.17): the one DUCAT object on a public surface.
///
/// Short because the surface is hostile. The card is the only field with
/// teeth — claiming it is what §16.9 verifies; everything else is an
/// untrusted claim a reader renders at its own judgment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HailNotice {
    pub version: u64,
    /// A `ducat:` card URI, purpose `hail`, claim-once.
    pub card: String,
    /// Coarse destination or area — human words, never coordinates.
    pub dest: String,
    /// The rider's offer. Absent means "quote me". Zero is `MALFORMED`.
    pub fare_pxmr: Option<u64>,
    /// Unix seconds. A reader MUST drop an expired notice unrendered.
    pub expiry: u64,
    /// Where the pickup roughly is — a geocell no finer than precision 6
    /// (~1.2 km), refused above. What lets a driver judge the distance to
    /// the fare before claiming, priced in privacy the §15.12 board already
    /// spends: the notice is pinned to a cell anyway.
    pub origin_cell: Option<String>,
    /// Where the ride roughly goes, same cap. The Uber-shaped triage field:
    /// a driver reads the job, not just the presence of one.
    pub dest_cell: Option<String>,
}

impl HailNotice {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::HN_VERSION, Value::Uint(self.version));
        m.insert(f::HN_CARD, Value::Text(self.card.clone()));
        m.insert(f::HN_DEST, Value::Text(self.dest.clone()));
        if let Some(fare) = self.fare_pxmr {
            m.insert(f::HN_FARE, Value::Uint(fare));
        }
        m.insert(f::HN_EXPIRY, Value::Uint(self.expiry));
        if let Some(c) = &self.origin_cell {
            m.insert(f::HN_ORIGIN_CELL, Value::Text(c.clone()));
        }
        if let Some(c) = &self.dest_cell {
            m.insert(f::HN_DEST_CELL, Value::Text(c.clone()));
        }
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        let version = r.uint(f::HN_VERSION)?;
        // 2, not 1: a version-1 notice carries neither an author nor a proof
        // of work, and there is no safe way to read one — accepting it would
        // be the downgrade that makes both worthless. See board.rs.
        if version != 2 {
            return Err(Reject::with_detail(RejectCode::Malformed, "unknown hail notice version"));
        }
        let card = r.opt_text(f::HN_CARD, MAX_HAIL_CARD_CHARS)?.ok_or_else(|| {
            Reject::with_detail(RejectCode::Malformed, "a hail notice needs a card")
        })?;
        if !card.starts_with("ducat:") {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a hail card must be a ducat: URI",
            ));
        }
        let dest = r.opt_text(f::HN_DEST, MAX_HAIL_DEST_CHARS)?.ok_or_else(|| {
            Reject::with_detail(RejectCode::Malformed, "a hail notice needs a destination")
        })?;
        if dest.is_empty() {
            return Err(Reject::with_detail(RejectCode::Malformed, "an empty destination says nothing"));
        }
        let fare_pxmr = r.opt_uint(f::HN_FARE)?;
        if fare_pxmr == Some(0) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a zero fare offer is a missing one",
            ));
        }
        let expiry = r.uint(f::HN_EXPIRY)?;
        let origin_cell = r.opt_text(f::HN_ORIGIN_CELL, 6)?;
        let dest_cell = r.opt_text(f::HN_DEST_CELL, 6)?;
        for cell in [&origin_cell, &dest_cell].into_iter().flatten() {
            if !crate::geo::valid_board_cell(cell) {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a board cell is a geohash no finer than precision 6",
                ));
            }
        }
        r.finish()?;
        Ok(HailNotice { version, card, dest, fare_pxmr, expiry, origin_cell, dest_cell })
    }
}

/// A listing on a public board (§16.18).
///
/// The second object DUCAT puts in the open, and the one that stays there.
/// A hail lives for minutes and describes a person who is about to move; a
/// listing lives for days and describes a car or a home that does not. That
/// difference decides everything about what may be in here.
///
/// **What is here** is what a stranger needs to decide whether to ask: the
/// shape of the thing, roughly where, what it costs, what each side stakes,
/// and a claim-once card to start a conversation with.
///
/// **What is deliberately not here** is everything they would need to
/// *arrive*: the address, the plate, the door code, photographs of the
/// inside of someone's home. Those pass through the sealed thread after the
/// two of them have agreed, because a listing is an advertisement and an
/// advertisement read by everyone should not double as a burglary brief.
#[derive(Debug, Clone, PartialEq)]
pub struct RentalNotice {
    pub version: u64,
    /// A `ducat:` card URI, purpose `rental`, claim-once.
    pub card: String,
    /// 1 = a place to stay, 2 = a vehicle.
    pub kind: u64,
    /// One human line: "Sunny room, 10 min from the station".
    pub title: String,
    /// Human words for the area — never coordinates, never an address.
    pub area: String,
    /// The board this sits on: a geohash no finer than precision 5 (~5 km).
    pub cell: Option<String>,
    /// Per night, or per day, in piconero.
    pub price_pxmr: u64,
    /// What *each* side stakes (§15.12), stated so the reader sees the whole
    /// cost before they ask rather than after.
    pub deposit_pxmr: u64,
    /// Unix seconds. A reader MUST drop an expired listing unrendered, and
    /// an owner who has stopped renting simply stops refreshing it.
    pub expiry: u64,
    // A vehicle's searchable shape. MALFORMED on a place.
    pub make: Option<String>,
    pub model: Option<String>,
    pub year: Option<u64>,
    /// 1 = manual, 2 = automatic.
    pub gearbox: Option<u64>,
    /// 1 = petrol, 2 = diesel, 3 = electric, 4 = hybrid.
    pub fuel: Option<u64>,
    pub seats: Option<u64>,
    pub color: Option<String>,
    /// The variant a renter actually cares about: "Sport", "GLX", "Long bed".
    pub trim: Option<String>,
    // A place's searchable shape. MALFORMED on a vehicle.
    pub rooms: Option<u64>,
    pub sleeps: Option<u64>,
    /// Floor area in square metres — the number people search on after price.
    pub size_m2: Option<u64>,
    /// Place: 1 = the whole place, 2 = a private room. Vehicle: 1 = car,
    /// 2 = van, 3 = motorbike.
    pub subtype: Option<u64>,
    /// A few short tags for what no field will ever cover.
    pub features: Vec<String>,
    /// How many of this the poster has. Always at least one.
    ///
    /// Almost every listing is a single thing — a bike, a room, an
    /// afternoon — and this exists for the shop with six identical kayaks:
    /// somebody deciding whether to ask wants to know they are not
    /// competing for the last one. One is written as *absent*, so the
    /// ordinary listing costs nothing on a board that is expensive to read,
    /// and every listing that means "one" has the same bytes.
    pub quantity: u64,
}

pub const RENTAL_PLACE: u64 = 1;
pub const RENTAL_VEHICLE: u64 = 2;
/// A thing for sale. Nothing comes back, so the escrow's deposits are
/// stakes: each side posts one and gets it back on handover.
pub const RENTAL_SALE: u64 = 3;
/// Equipment let by the day — a kayak, a bike, skis, a pressure washer.
/// A vehicle without a make or a gearbox, and priced the same way.
pub const RENTAL_GEAR: u64 = 4;
/// Somebody's time, by the hour. The price is a rate, not a total.
pub const RENTAL_SKILL: u64 = 5;

/// How many top-level categories each kind of listing recognises.
///
/// Deliberately small and flat rather than a tree: the categories are a
/// coarse filter on a board that is expensive to read, they have to be
/// translated everywhere this ships, and a taxonomy fine enough to be
/// accurate is one nobody fits — most tradespeople are the handyman who
/// also does electrics. What somebody actually does goes in `features`.
pub const fn rental_subtype_top(kind: u64) -> u64 {
    match kind {
        RENTAL_PLACE => 2,
        RENTAL_VEHICLE => 3,
        // Sale: goods, furniture, tools, sport, garden, electronics, music,
        // vehicles-as-goods, other.
        RENTAL_SALE => 9,
        // Gear: sport, tools, outdoor, party, other.
        RENTAL_GEAR => 5,
        // Skill: trades and services, the flat set. See the spec's table.
        RENTAL_SKILL => 12,
        _ => 0,
    }
}
const MAX_RENTAL_TITLE_CHARS: usize = 60;
const MAX_RENTAL_AREA_CHARS: usize = 40;
const MAX_RENTAL_WORD_CHARS: usize = 24;
const MAX_RENTAL_FEATURES: usize = 8;
const MAX_RENTAL_FEATURE_CHARS: usize = 16;
/// A shop with six kayaks, not a warehouse. A board slot is a scarce, shared
/// thing and a listing is an advertisement for what somebody has to hand; a
/// count past this is describing inventory that wants its own listing.
const MAX_RENTAL_QUANTITY: u64 = 999;

/// §16.18.2: a publication on a public board.
///
/// The board name carries the where — `topic:<category>[.<lang>]` for the
/// worldwide shelf, `local:<cell>` for the town paper, and cross-posting is
/// two stamps, paid honestly. The notice carries what a stranger needs to
/// decide, and claiming its card IS subscribing (§16.20): after the claim,
/// everything is the sealed machinery that already exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubNotice {
    pub version: u64,
    /// A `ducat:` card URI, purpose `publish`, claim-once.
    pub card: String,
    pub title: String,
    /// A sentence about it, when the title does not already say everything.
    pub blurb: Option<String>,
    /// Piconero a period. `None` is free — the only spelling of free.
    pub price_pxmr: Option<u64>,
    pub expiry: u64,
}

const MAX_PUB_TITLE_CHARS: usize = 60;
const MAX_PUB_BLURB_CHARS: usize = 280;

impl PubNotice {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::PN_VERSION, Value::Uint(self.version));
        m.insert(f::PN_CARD, Value::Text(self.card.clone()));
        m.insert(f::PN_TITLE, Value::Text(self.title.clone()));
        if let Some(b) = &self.blurb {
            m.insert(f::PN_BLURB, Value::Text(b.clone()));
        }
        if let Some(p) = self.price_pxmr {
            m.insert(f::PN_PRICE, Value::Uint(p));
        }
        m.insert(f::PN_EXPIRY, Value::Uint(self.expiry));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        let version = r.uint(f::PN_VERSION)?;
        if version != 1 {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "unknown publication notice version",
            ));
        }
        let card = r.opt_text(f::PN_CARD, MAX_HAIL_CARD_CHARS)?.ok_or_else(|| {
            Reject::with_detail(RejectCode::Malformed, "a publication listing needs a card")
        })?;
        if !card.starts_with("ducat:") {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a listing card must be a ducat: URI",
            ));
        }
        let title = r.opt_text(f::PN_TITLE, MAX_PUB_TITLE_CHARS)?.ok_or_else(|| {
            Reject::with_detail(RejectCode::Malformed, "a publication listing needs a title")
        })?;
        // Empty text is refused inside the reader itself — "omit the key
        // instead" — so absence stays the one spelling of nothing to say.
        let blurb = r.opt_text(f::PN_BLURB, MAX_PUB_BLURB_CHARS)?;
        let price_pxmr = r.opt_uint(f::PN_PRICE)?;
        if price_pxmr == Some(0) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "free is spelled by omission",
            ));
        }
        let expiry = r.uint(f::PN_EXPIRY)?;
        r.finish()?;
        Ok(Self { version, card, title, blurb, price_pxmr, expiry })
    }
}

const MAX_SITE_TITLE_CHARS: usize = 80;

/// §16.22: a site's head — the mutable pointer its `ducat:site/` URI names.
///
/// Lives in the site record's subkey 0, rewritten in place by its owner:
/// the record key is the site's stable identity, the head is whatever it
/// currently points at. The bundle itself travels as a multi-file swarm
/// share and renders in a sealed room — nothing on the page can reach the
/// network, which is what makes the trust story one sentence long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteHead {
    pub version: u64,
    pub title: String,
    /// The current bundle's swarm share key.
    pub share: String,
    /// The current bundle's index digest.
    pub digest: [u8; 32],
    /// When the head was last rewritten, epoch seconds. Advisory — a
    /// reader shows it, nothing enforces it.
    pub updated: u64,
}

impl SiteHead {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::SITE_VERSION, Value::Uint(self.version));
        m.insert(f::SITE_TITLE, Value::Text(self.title.clone()));
        m.insert(f::SITE_SHARE, Value::Text(self.share.clone()));
        m.insert(f::SITE_DIGEST, Value::Bytes(self.digest.to_vec()));
        m.insert(f::SITE_UPDATED, Value::Uint(self.updated));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        let version = r.uint(f::SITE_VERSION)?;
        if version != 1 {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "unknown site head version",
            ));
        }
        let title = r.opt_text(f::SITE_TITLE, MAX_SITE_TITLE_CHARS)?.ok_or_else(|| {
            Reject::with_detail(RejectCode::Malformed, "a site head needs a title")
        })?;
        let share = r.opt_text(f::SITE_SHARE, MAX_SHARE_KEY_CHARS)?.ok_or_else(|| {
            Reject::with_detail(RejectCode::Malformed, "a site head names its bundle's share")
        })?;
        let digest = r
            .opt_bytes(f::SITE_DIGEST, Some(32))?
            .ok_or_else(|| {
                Reject::with_detail(
                    RejectCode::Malformed,
                    "a site head carries its bundle's digest",
                )
            })?;
        let updated = r.uint(f::SITE_UPDATED)?;
        r.finish()?;
        Ok(Self {
            version,
            title,
            share,
            digest: digest.try_into().unwrap(),
            updated,
        })
    }
}

impl RentalNotice {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::RN_VERSION, Value::Uint(self.version));
        m.insert(f::RN_CARD, Value::Text(self.card.clone()));
        m.insert(f::RN_KIND, Value::Uint(self.kind));
        m.insert(f::RN_TITLE, Value::Text(self.title.clone()));
        m.insert(f::RN_AREA, Value::Text(self.area.clone()));
        if let Some(c) = &self.cell {
            m.insert(f::RN_CELL, Value::Text(c.clone()));
        }
        m.insert(f::RN_PRICE, Value::Uint(self.price_pxmr));
        m.insert(f::RN_DEPOSIT, Value::Uint(self.deposit_pxmr));
        m.insert(f::RN_EXPIRY, Value::Uint(self.expiry));
        for (id, v) in [
            (f::RN_MAKE, &self.make),
            (f::RN_MODEL, &self.model),
            (f::RN_COLOR, &self.color),
            (f::RN_TRIM, &self.trim),
        ] {
            if let Some(t) = v {
                m.insert(id, Value::Text(t.clone()));
            }
        }
        for (id, v) in [
            (f::RN_YEAR, self.year),
            (f::RN_GEARBOX, self.gearbox),
            (f::RN_FUEL, self.fuel),
            (f::RN_SEATS, self.seats),
            (f::RN_ROOMS, self.rooms),
            (f::RN_SLEEPS, self.sleeps),
            (f::RN_SIZE_M2, self.size_m2),
            (f::RN_SUBTYPE, self.subtype),
        ] {
            if let Some(n) = v {
                m.insert(id, Value::Uint(n));
            }
        }
        if !self.features.is_empty() {
            m.insert(
                f::RN_FEATURES,
                Value::Array(self.features.iter().cloned().map(Value::Text).collect()),
            );
        }
        // Only when it says something. One is the default and the absent
        // case, so "I have one of these" has exactly one encoding — which
        // matters here more than it usually would, because the signature is
        // over these bytes and over the slot they went into.
        if self.quantity > 1 {
            m.insert(f::RN_QUANTITY, Value::Uint(self.quantity));
        }
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        let version = r.uint(f::RN_VERSION)?;
        // 2, not 1 — see the hail's note above, and board.rs.
        if version != 2 {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "unknown rental notice version",
            ));
        }
        let card = r.opt_text(f::RN_CARD, MAX_HAIL_CARD_CHARS)?.ok_or_else(|| {
            Reject::with_detail(RejectCode::Malformed, "a listing needs a card")
        })?;
        if !card.starts_with("ducat:") {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a listing card must be a ducat: URI",
            ));
        }
        let kind = r.uint(f::RN_KIND)?;
        if !(RENTAL_PLACE..=RENTAL_SKILL).contains(&kind) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "unknown listing kind",
            ));
        }
        let title = r.opt_text(f::RN_TITLE, MAX_RENTAL_TITLE_CHARS)?.ok_or_else(|| {
            Reject::with_detail(RejectCode::Malformed, "a listing needs a title")
        })?;
        if title.is_empty() {
            return Err(Reject::with_detail(RejectCode::Malformed, "an empty title says nothing"));
        }
        let area = r.opt_text(f::RN_AREA, MAX_RENTAL_AREA_CHARS)?.unwrap_or_default();
        let cell = r.opt_text(f::RN_CELL, crate::geo::MAX_LISTING_GEOHASH_CHARS)?;
        if let Some(c) = &cell {
            if !crate::geo::valid_listing_cell(c) {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a listing cell is a geohash no finer than precision 5",
                ));
            }
        }
        let price_pxmr = r.uint(f::RN_PRICE)?;
        if price_pxmr == 0 {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a listing with no price is not an offer",
            ));
        }
        let deposit_pxmr = r.opt_uint(f::RN_DEPOSIT)?.unwrap_or(0);
        let expiry = r.uint(f::RN_EXPIRY)?;

        let make = r.opt_text(f::RN_MAKE, MAX_RENTAL_WORD_CHARS)?;
        let model = r.opt_text(f::RN_MODEL, MAX_RENTAL_WORD_CHARS)?;
        let color = r.opt_text(f::RN_COLOR, MAX_RENTAL_WORD_CHARS)?;
        let trim = r.opt_text(f::RN_TRIM, MAX_RENTAL_WORD_CHARS)?;
        let year = r.opt_uint(f::RN_YEAR)?;
        let gearbox = r.opt_uint(f::RN_GEARBOX)?;
        let fuel = r.opt_uint(f::RN_FUEL)?;
        let seats = r.opt_uint(f::RN_SEATS)?;
        let rooms = r.opt_uint(f::RN_ROOMS)?;
        let sleeps = r.opt_uint(f::RN_SLEEPS)?;
        let size_m2 = r.opt_uint(f::RN_SIZE_M2)?;
        let subtype = r.opt_uint(f::RN_SUBTYPE)?;
        let quantity = r.opt_uint(f::RN_QUANTITY)?;

        // A place has no gearbox and a car has no bedrooms. Refusing the
        // mismatch keeps a reader from having to guess which fields it is
        // allowed to believe, and keeps a listing from describing two things.
        let vehicle_only = make.is_some()
            || model.is_some()
            || year.is_some()
            || gearbox.is_some()
            || fuel.is_some()
            || seats.is_some()
            || color.is_some()
            || trim.is_some();
        let place_only = rooms.is_some() || sleeps.is_some() || size_m2.is_some();
        if kind == RENTAL_PLACE && vehicle_only {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a place does not have a make, a trim, a gearbox or a fuel",
            ));
        }
        if kind == RENTAL_VEHICLE && place_only {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "a vehicle does not have bedrooms or floor area",
            ));
        }
        // The three kinds added in 0.89 carry no typed extras at all: a
        // kayak has no gearbox, a bike for sale has no bedrooms, and an
        // electrician has neither. Everything they want to say is a title,
        // a price, an area, a subtype and free-text features — which is
        // also why they needed no new fields on the wire.
        if kind > RENTAL_VEHICLE && (vehicle_only || place_only) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "this listing kind has no typed extras",
            ));
        }
        if let Some(g) = gearbox {
            if g == 0 || g > 2 {
                return Err(Reject::with_detail(RejectCode::Malformed, "gearbox is manual or automatic"));
            }
        }
        if let Some(fl) = fuel {
            if fl == 0 || fl > 4 {
                return Err(Reject::with_detail(RejectCode::Malformed, "unknown fuel"));
            }
        }
        // A count is only meaningful for something you can have more than one
        // of. An hourly rate is one person's time, and a skill saying "3
        // available" would be describing staffing it does not have.
        if quantity.is_some() && kind == RENTAL_SKILL {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "somebody's time is not stock",
            ));
        }
        let quantity = match quantity {
            // One is the absent case, and writing it explicitly would be a
            // second spelling of the same listing.
            Some(1) | Some(0) => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a quantity is written only when it is more than one",
                ))
            }
            Some(q) if q > MAX_RENTAL_QUANTITY => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "more than a listing is for",
                ))
            }
            Some(q) => q,
            None => 1,
        };
        if let Some(st) = subtype {
            let top = rental_subtype_top(kind);
            if st == 0 || st > top {
                return Err(Reject::with_detail(RejectCode::Malformed, "unknown subtype"));
            }
        }
        if let Some(y) = year {
            if !(1900..=2200).contains(&y) {
                return Err(Reject::with_detail(RejectCode::Malformed, "implausible year"));
            }
        }

        let features = match r.opt_array(f::RN_FEATURES)? {
            None => Vec::new(),
            Some(items) => {
                if items.len() > MAX_RENTAL_FEATURES {
                    return Err(Reject::with_detail(
                        RejectCode::Malformed,
                        "too many features to be a summary",
                    ));
                }
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    let t = match item {
                        Value::Text(t) => t,
                        _ => {
                            return Err(Reject::with_detail(
                                RejectCode::Malformed,
                                "a feature is a short word",
                            ))
                        }
                    };
                    if t.is_empty() || t.chars().count() > MAX_RENTAL_FEATURE_CHARS {
                        return Err(Reject::with_detail(
                            RejectCode::Malformed,
                            "a feature is a short word",
                        ));
                    }
                    out.push(t);
                }
                out
            }
        };
        r.finish()?;
        Ok(RentalNotice {
            version, card, kind, title, area, cell, price_pxmr, deposit_pxmr, expiry,
            make, model, year, gearbox, fuel, seats, color, trim,
            rooms, sleeps, size_m2, subtype, features, quantity,
        })
    }
}

#[cfg(test)]
mod rental_tests {
    use super::*;

    /// The three kinds added in 0.89 carry no typed extras: a title, a
    /// price, an area, a subtype and free-text features. Everything each of
    /// them wants to say fits fields that already existed, which is why they
    /// cost no new field numbers.
    fn a_plain(kind: u64, subtype: Option<u64>) -> RentalNotice {
        RentalNotice {
            version: 2,
            card: "ducat:card/abc".into(),
            kind,
            title: "A thing".into(),
            area: "north side".into(),
            cell: Some("dqcjq".into()),
            price_pxmr: 40_000_000_000,
            deposit_pxmr: 4_000_000_000,
            expiry: 1_800_000_000,
            make: None, model: None, year: None, gearbox: None, fuel: None,
            seats: None, color: None, trim: None,
            rooms: None, sleeps: None, size_m2: None,
            subtype,
            features: vec!["good condition".into()],
            quantity: 1,
        }
    }

    #[test]
    fn the_new_kinds_round_trip() {
        for kind in [RENTAL_SALE, RENTAL_GEAR, RENTAL_SKILL] {
            let n = a_plain(kind, Some(1));
            let back = RentalNotice::from_value(n.clone().to_value())
                .unwrap_or_else(|e| panic!("kind {kind} was refused: {e:?}"));
            assert_eq!(back.kind, kind);
            assert_eq!(back.title, n.title);
            assert_eq!(back.price_pxmr, n.price_pxmr);
            assert_eq!(back.subtype, Some(1));
            assert_eq!(back.features, n.features);
        }
    }

    #[test]
    fn a_kind_nobody_has_defined_is_refused() {
        let mut n = a_plain(RENTAL_SKILL, None);
        n.kind = 6;
        assert!(RentalNotice::from_value(n.to_value()).is_err());
        let mut z = a_plain(RENTAL_SALE, None);
        z.kind = 0;
        assert!(RentalNotice::from_value(z.to_value()).is_err());
    }

    #[test]
    fn the_new_kinds_have_no_typed_extras() {
        // A kayak has no gearbox and a bike for sale has no bedrooms. Saying
        // otherwise on the wire is malformed rather than ignored, so a second
        // implementation cannot quietly disagree about what a listing is.
        let mut geared = a_plain(RENTAL_GEAR, None);
        geared.gearbox = Some(2);
        assert!(RentalNotice::from_value(geared.to_value()).is_err());

        let mut roomed = a_plain(RENTAL_SALE, None);
        roomed.rooms = Some(3);
        assert!(RentalNotice::from_value(roomed.to_value()).is_err());
    }

    #[test]
    fn each_kind_bounds_its_own_categories() {
        // The top is per kind, so a subtype legal for a trade is not
        // automatically legal for a kayak.
        for (kind, top) in [
            (RENTAL_PLACE, 2u64), (RENTAL_VEHICLE, 3), (RENTAL_SALE, 9),
            (RENTAL_GEAR, 5), (RENTAL_SKILL, 12),
        ] {
            assert_eq!(rental_subtype_top(kind), top);
            let ok = a_plain(kind, Some(top));
            // Places and vehicles carry typed extras of their own; the plain
            // shape is only valid for the three new kinds.
            if kind > RENTAL_VEHICLE {
                assert!(RentalNotice::from_value(ok.to_value()).is_ok());
                let over = a_plain(kind, Some(top + 1));
                assert!(RentalNotice::from_value(over.to_value()).is_err());
                let zero = a_plain(kind, Some(0));
                assert!(RentalNotice::from_value(zero.to_value()).is_err());
            }
        }
    }

    fn a_car() -> RentalNotice {
        RentalNotice {
            version: 2,
            card: "ducat:card/abc".into(),
            kind: RENTAL_VEHICLE,
            title: "2019 Corolla, automatic".into(),
            area: "north side".into(),
            cell: Some("dqcjq".into()),
            price_pxmr: 40_000_000_000,
            deposit_pxmr: 12_000_000_000,
            expiry: 1_800_000_000,
            make: Some("Toyota".into()),
            model: Some("Corolla".into()),
            year: Some(2019),
            gearbox: Some(2),
            fuel: Some(1),
            seats: Some(5),
            color: Some("silver".into()),
            trim: Some("Hybrid LE".into()),
            rooms: None,
            sleeps: None,
            size_m2: None,
            subtype: Some(1),
            features: vec!["child seat".into(), "roof box".into()],
            quantity: 1,
        }
    }

    fn a_room() -> RentalNotice {
        RentalNotice {
            version: 2,
            card: "ducat:card/xyz".into(),
            kind: RENTAL_PLACE,
            title: "Sunny room near the park".into(),
            area: "Kreuzberg".into(),
            cell: Some("u33db".into()),
            price_pxmr: 25_000_000_000,
            deposit_pxmr: 5_000_000_000,
            expiry: 1_800_000_000,
            make: None, model: None, year: None, gearbox: None, fuel: None,
            seats: None, color: None, trim: None,
            rooms: Some(1),
            sleeps: Some(2),
            size_m2: Some(28),
            subtype: Some(2),
            features: vec!["wifi".into()],
            quantity: 1,
        }
    }

    fn round_trip(n: &RentalNotice) -> Result<RentalNotice, Reject> {
        RentalNotice::from_value(crate::cbor::decode(&n.to_value().encode()).unwrap())
    }

    #[test]
    fn a_listing_survives_the_wire() {
        assert_eq!(round_trip(&a_car()).unwrap(), a_car());
        assert_eq!(round_trip(&a_room()).unwrap(), a_room());
    }

    /// The rule that keeps a reader from guessing which half to believe.
    #[test]
    fn a_place_has_no_gearbox_and_a_car_has_no_bedrooms() {
        let mut bad = a_room();
        bad.gearbox = Some(2);
        assert!(round_trip(&bad).is_err(), "a room with a gearbox");
        let mut bad = a_room();
        bad.make = Some("Toyota".into());
        assert!(round_trip(&bad).is_err(), "a room with a make");
        let mut bad = a_car();
        bad.rooms = Some(3);
        assert!(round_trip(&bad).is_err(), "a car with bedrooms");
    }

    /// §16.18's whole privacy argument in one assertion: a listing outlives
    /// the day it was posted, so it may not pin the thing any closer than a
    /// city. Precision 6 is fine for a person waiting at a kerb and wrong
    /// for a home that will still be there next week.
    #[test]
    fn a_listing_cell_is_coarser_than_a_hail_cell() {
        let mut n = a_room();
        n.cell = Some("u33dbc".into()); // precision 6 — legal on a hail
        assert!(round_trip(&n).is_err(), "precision 6 must be refused on a listing");
        n.cell = Some("u33db".into());
        assert!(round_trip(&n).is_ok());
    }

    #[test]
    fn a_listing_states_a_price_and_a_card() {
        let mut n = a_car();
        n.price_pxmr = 0;
        assert!(round_trip(&n).is_err(), "a listing with no price");
        let mut n = a_car();
        n.card = "https://example.com".into();
        assert!(round_trip(&n).is_err(), "a card that is not a ducat: URI");
        let mut n = a_car();
        n.title = String::new();
        assert!(round_trip(&n).is_err(), "an empty title");
    }

    #[test]
    fn nonsense_enumerations_are_refused() {
        for (f, v) in [("gearbox", 3u64), ("fuel", 9)] {
            let mut n = a_car();
            match f {
                "gearbox" => n.gearbox = Some(v),
                _ => n.fuel = Some(v),
            }
            assert!(round_trip(&n).is_err(), "{f} = {v}");
        }
        let mut n = a_car();
        n.year = Some(1750);
        assert!(round_trip(&n).is_err(), "a car from 1750");
    }

    /// One is the absent case, and the only spelling of it.
    ///
    /// A board slot is scarce and a signature is over these exact bytes, so a
    /// listing that means "I have one of these" — which is almost all of them
    /// — writes nothing, and there is no second encoding that means the same.
    #[test]
    fn one_of_a_thing_is_written_as_nothing() {
        let mut n = a_plain(RENTAL_SALE, Some(1));
        assert_eq!(n.quantity, 1);
        let Value::Map(m) = n.to_value() else { panic!() };
        assert!(!m.contains_key(&f::RN_QUANTITY), "one wrote a field");
        assert_eq!(round_trip(&n).unwrap().quantity, 1);

        n.quantity = 6;
        let Value::Map(m) = n.to_value() else { panic!() };
        assert!(m.contains_key(&f::RN_QUANTITY), "six wrote nothing");
        assert_eq!(round_trip(&n).unwrap().quantity, 6);
    }

    /// The two values that must not arrive: a listing of nothing, and the
    /// second spelling of one.
    #[test]
    fn a_quantity_of_zero_or_one_is_refused_on_the_wire() {
        for q in [0u64, 1] {
            let mut m = match a_plain(RENTAL_SALE, Some(1)).to_value() {
                Value::Map(m) => m,
                _ => panic!(),
            };
            m.insert(f::RN_QUANTITY, Value::Uint(q));
            assert!(
                RentalNotice::from_value(Value::Map(m)).is_err(),
                "quantity {q} was accepted",
            );
        }
    }

    /// A shop, not a warehouse.
    #[test]
    fn a_quantity_has_a_ceiling() {
        let mut n = a_plain(RENTAL_SALE, Some(1));
        n.quantity = MAX_RENTAL_QUANTITY;
        assert!(round_trip(&n).is_ok());
        n.quantity = MAX_RENTAL_QUANTITY + 1;
        assert!(round_trip(&n).is_err(), "a warehouse got onto a board");
    }

    /// Somebody's time is not stock. An hourly rate saying "3 available"
    /// would be describing staffing the listing does not have.
    #[test]
    fn a_skill_cannot_be_stocked() {
        let mut n = a_plain(RENTAL_SKILL, Some(1));
        n.quantity = 3;
        assert!(round_trip(&n).is_err(), "an hour was sold three at a time");
        // And the kinds that can be counted still can.
        for kind in [RENTAL_PLACE, RENTAL_VEHICLE, RENTAL_SALE, RENTAL_GEAR] {
            let mut ok = a_plain(kind, Some(1));
            ok.quantity = 3;
            // a_plain carries no typed extras, which the two shaped kinds
            // also accept — they only refuse each *other's*.
            assert_eq!(round_trip(&ok).unwrap().quantity, 3, "kind {kind}");
        }
    }

    #[test]
    fn features_are_a_summary_not_an_essay() {
        let mut n = a_room();
        n.features = (0..20).map(|i| format!("f{i}")).collect();
        assert!(round_trip(&n).is_err(), "twenty features is a description");
        let mut n = a_room();
        n.features = vec!["a".repeat(40)];
        assert!(round_trip(&n).is_err(), "a feature that is a sentence");
    }
}

#[cfg(test)]
mod hail_tests {
    use super::*;

    fn ok_notice() -> HailNotice {
        HailNotice {
            version: 2,
            card: "ducat:abc123".into(),
            dest: "terminal B".into(),
            fare_pxmr: Some(5_000_000_000),
            expiry: 1_800_000_000,
            origin_cell: Some("dqcjq8".into()),
            dest_cell: Some("dqcjnb".into()),
        }
    }

    #[test]
    fn hail_round_trips() {
        let n = ok_notice();
        let v = crate::cbor::decode(&n.to_value().encode()).unwrap();
        assert_eq!(HailNotice::from_value(v).unwrap(), n);
    }

    #[test]
    fn hail_rejects_the_malformed_cases() {
        // Wrong scheme, empty dest, zero fare, wrong version — each refused.
        let mut bad = ok_notice();
        bad.card = "https://not-a-card".into();
        assert!(HailNotice::from_value(bad.to_value()).is_err());
        let mut bad = ok_notice();
        bad.dest = "".into();
        assert!(HailNotice::from_value(bad.to_value()).is_err());
        let mut bad = ok_notice();
        bad.fare_pxmr = Some(0);
        assert!(HailNotice::from_value(bad.to_value()).is_err());
        let mut bad = ok_notice();
        bad.version = 1;
        assert!(HailNotice::from_value(bad.to_value()).is_err());
        let mut bad = ok_notice();
        bad.version = 3;
        assert!(HailNotice::from_value(bad.to_value()).is_err());
        // "Quote me" — fare absent — is fine.
        let mut ok = ok_notice();
        ok.fare_pxmr = None;
        assert!(HailNotice::from_value(ok.to_value()).is_ok());
    }
}

#[cfg(test)]
mod attachment_tests {
    use super::*;

    #[test]
    fn attachment_round_trips_and_tamper_fails() {
        let key = [7u8; 32];
        let nonce = [9u8; 24];
        let pt = b"a small picture's worth of bytes".to_vec();
        let ct = attachment_seal(&key, &nonce, &pt);
        assert_eq!(attachment_open(&key, &nonce, &ct).unwrap(), pt);
        let mut bad = ct.clone();
        bad[0] ^= 1;
        assert!(attachment_open(&key, &nonce, &bad).is_err());
        let wrong = [8u8; 32];
        assert!(attachment_open(&wrong, &nonce, &ct).is_err());
    }
}

#[cfg(test)]
mod position_ref_tests {
    use super::*;

    fn base() -> Message {
        Message {
            version: 1, suite: 1, seq: 3, prev: [0u8; 32],
            body: "sharing my position".into(), timestamp: 1_800_000_000,
            kind: MessageKind::PositionRef,
            amount_pxmr: None, txid: None, payto: None, items: vec![], tax_pxmr: None,
            re_seq: None, re_own: false, eta_secs: None,
            payload: None, round: None, ceremony_id: None, attachment: None,
            position: Some(PositionRef {
                record_key: "VLD0:positionrecord".into(),
                stream_key: [0x5au8; 32],
            }), publication: None,
        call: None,
            group_id: None, group_seq: None,
            group_re_sender: None, group_re_seq: None,
        }
    }

    #[test]
    fn a_position_ref_round_trips() {
        let m = base();
        let got = Message::from_value(m.to_value()).expect("opens");
        assert_eq!(got.position, m.position);
        assert_eq!(got.kind, MessageKind::PositionRef);
    }

    #[test]
    fn a_position_kind_without_a_reference_is_refused() {
        let mut m = base();
        m.position = None;
        assert!(Message::from_value(m.to_value()).is_err());
    }

    #[test]
    fn a_reference_on_another_kind_is_refused() {
        let mut m = base();
        m.kind = MessageKind::Text;
        // A text body is required for a Text kind; give it one.
        assert!(Message::from_value(m.to_value()).is_err());
    }

    #[test]
    fn a_reference_needs_both_halves() {
        // Encode by hand: a record with no key, then a key with no record.
        let m = base();
        let Value::Map(mut map) = m.to_value() else { unreachable!() };
        map.remove(&f::MSG_POS_STREAM);
        assert!(Message::from_value(Value::Map(map)).is_err());

        let Value::Map(mut map) = m.to_value() else { unreachable!() };
        map.remove(&f::MSG_POS_RECORD);
        assert!(Message::from_value(Value::Map(map)).is_err());
    }

    #[test]
    fn a_stream_key_is_thirty_two_bytes() {
        let m = base();
        let Value::Map(mut map) = m.to_value() else { unreachable!() };
        map.insert(f::MSG_POS_STREAM, Value::Bytes(vec![0x5au8; 31]));
        assert!(Message::from_value(Value::Map(map)).is_err());
    }
}
