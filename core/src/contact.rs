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

/// An out-of-band offer of contact (§16.9).
///
/// Carried by NFC, by QR, or as a `ducat:` URI through any channel the two
/// people already trust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactInvite {
    pub version: u64,
    pub suite: u8,
    /// The persona being offered.
    pub persona: Vec<u8>,
    /// Where to reach it (§16.4).
    pub rendezvous: Vec<u8>,
    /// **Self-asserted, and worth exactly what the channel that carried it is
    /// worth.** A petname the receiver assigns locally is the real name (§7.5).
    pub display_name: Option<String>,
    /// Commitment to the claim secret. The secret travels with the card; this is
    /// what the issuer stores, so a stolen invitation list is not a set of
    /// usable claims.
    pub claim_commit: [u8; 32],
    /// Absolute expiry. An invitation that never expires is a credential the
    /// issuer has forgotten they published.
    pub expiry: u64,
}

impl ContactInvite {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::ContactOffer)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::INV_PERSONA, Value::Bytes(self.persona.clone()));
        m.insert(f::INV_RENDEZVOUS, Value::Bytes(self.rendezvous.clone()));
        if let Some(n) = &self.display_name {
            m.insert(f::INV_DISPLAY_NAME, Value::Text(n.clone()));
        }
        m.insert(f::INV_CLAIM_COMMIT, Value::Bytes(self.claim_commit.to_vec()));
        m.insert(f::INV_EXPIRY, Value::Uint(self.expiry));
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
        let out = ContactInvite {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            persona: r.bytes(f::INV_PERSONA, None)?,
            rendezvous: r.bytes(f::INV_RENDEZVOUS, None)?,
            display_name: r.opt_text(f::INV_DISPLAY_NAME, MAX_DISPLAY_NAME_CHARS)?,
            claim_commit: r.bytes(f::INV_CLAIM_COMMIT, Some(32))?.try_into().unwrap(),
            expiry: r.uint(f::INV_EXPIRY)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// Claiming an invitation: the other half of the mutual exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactClaim {
    pub version: u64,
    pub suite: u8,
    /// The claimant's persona and reach, so contact is mutual rather than a
    /// subscription. §16.3's rule holds: nothing about you persists unless you
    /// affirmatively hand it over.
    pub persona: Vec<u8>,
    pub rendezvous: Vec<u8>,
    pub display_name: Option<String>,
    /// The secret from the card. Presenting it is what proves the claimant
    /// actually received the invitation rather than guessed at a persona.
    pub claim_secret: [u8; 32],
    pub timestamp: u64,
}

impl ContactClaim {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::ContactAccept)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::CLM_PERSONA, Value::Bytes(self.persona.clone()));
        m.insert(f::CLM_RENDEZVOUS, Value::Bytes(self.rendezvous.clone()));
        if let Some(n) = &self.display_name {
            m.insert(f::CLM_DISPLAY_NAME, Value::Text(n.clone()));
        }
        m.insert(f::CLM_SECRET, Value::Bytes(self.claim_secret.to_vec()));
        m.insert(f::CLM_TS, Value::Uint(self.timestamp));
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
        let out = ContactClaim {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            persona: r.bytes(f::CLM_PERSONA, None)?,
            rendezvous: r.bytes(f::CLM_RENDEZVOUS, None)?,
            display_name: r.opt_text(f::CLM_DISPLAY_NAME, MAX_DISPLAY_NAME_CHARS)?,
            claim_secret: r.bytes(f::CLM_SECRET, Some(32))?.try_into().unwrap(),
            timestamp: r.uint(f::CLM_TS)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// Whether a claim may be honoured.
///
/// `already_claimed` is the issuer's own record. It is a parameter rather than
/// state held here because single-use is only meaningful if somebody remembers,
/// and the thing that remembers is the issuer's store — not a check that can be
/// satisfied by asking the claimant.
pub fn check_claim(
    invite: &ContactInvite,
    claim: &ContactClaim,
    now: u64,
    already_claimed: bool,
) -> Result<(), Reject> {
    if already_claimed {
        return Err(Reject::with_detail(
            RejectCode::Replay,
            "this invitation has already been used",
        ));
    }
    if now > invite.expiry {
        return Err(Reject::with_detail(
            RejectCode::Expired,
            "invitation has expired",
        ));
    }
    // The secret is what distinguishes someone who was given the card from
    // someone who merely knows the persona, which is public by design.
    let expect = commit(Purpose::ChainLink, &claim.claim_secret);
    if !commit_eq(&invite.claim_commit, &expect) {
        return Err(Reject::with_detail(
            RejectCode::BadSig,
            "claim secret does not match this invitation",
        ));
    }
    // A contact with itself is not a contact, and accepting one would let a
    // stolen card be "claimed" by its own issuer to burn it.
    if claim.persona == invite.persona {
        return Err(Reject::with_detail(
            RejectCode::PolicyRefused,
            "an invitation cannot be claimed by its own persona",
        ));
    }
    Ok(())
}

/// Compute the commitment an issuer stores for a claim secret.
pub fn claim_commitment(secret: &[u8; 32]) -> [u8; 32] {
    commit(Purpose::ChainLink, secret)
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
        };
        r.finish()?;
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
