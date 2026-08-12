//! Escrow (§8.2) and bonded fast settlement (§17.4, §17.5).
//!
//! These were the two highest-value paths in the protocol and the two with no
//! implementation and no vectors — the manifest said so. Everything covered
//! before this module was direct settlement.
//!
//! # TXID is not TXPROOF, and the difference is load-bearing
//!
//! Draft 0.17 established that under `fast/1` the payee **is** the recipient, so
//! it can scan the mempool with its own view key. A proof exists to convince
//! someone who is *not* the recipient — and that is an arbiter, and nobody else.
//! So there are two objects here that an implementer will be tempted to merge:
//!
//! - [`TxId`] rides the happy path. It is a **pointer**, not evidence. Its
//!   `amount_pxmr` is a claim by the party who owes money, and a payee that
//!   accepts it without scanning has verified nothing at all.
//! - [`TxProof`] appears only inside a [`SlashClaim`]. It carries a Monero
//!   transaction proof, which the arbiter checks against the chain because it
//!   cannot be handed the payee's view key — that would expose their entire
//!   income (§17.5).
//!
//! The §6 message table and §18.4's transition table both said `TXPROOF` drove
//! acceptance, three drafts after §17.4 said `TXID` did. That inconsistency is
//! resolved here and in the spec at 0.47.
//!
//! # The escrow ceremony is where §2.5 happened
//!
//! RetoSwap — a Haveno-derived Monero DEX running exactly this structure — was
//! drained of ~$2.7M in May 2026 by a **forged, out-of-order ACK that overwrote
//! the arbitrator's address**, with no check against a known key. Both halves of
//! that attack are refused here by construction: [`RoundTracker`] accepts only
//! the round it is expecting, and [`check_escrow_ready`] takes the trusted
//! arbiter set as an argument so an arbiter can never arrive in a message.

use std::collections::BTreeMap;

use crate::cbor::Value;
use crate::commit::{commit, commit_eq, Purpose};
use crate::reject::{Reject, RejectCode};
use crate::sig::ObjectType;
use crate::wire::{f, type_code, Accept, Reader, Receipt};

// ---------------------------------------------------------------------------
// TXID — fast/1's mempool pointer
// ---------------------------------------------------------------------------

/// Payer → payee under `fast/1`: "the transaction is out, here is where to look".
///
/// **Nothing here is evidence.** A payee that reads `amount_pxmr` and accepts
/// has trusted the counterparty's arithmetic about the counterparty's own debt.
/// The fields exist so the payee knows *what to scan for*; the answer comes from
/// its own view key (§17.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxId {
    pub version: u64,
    pub suite: u8,
    /// Chain link to the ACCEPT this settles.
    pub accept_link: [u8; 32],
    pub txid: [u8; 32],
    /// What the payer says it sent. Checked against the ACCEPT here, and
    /// against the chain by the payee.
    pub amount_pxmr: u64,
    pub timestamp: u64,
}

impl TxId {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::TxId)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::TXID_ACCEPT_LINK, Value::Bytes(self.accept_link.to_vec()));
        m.insert(f::TXID_TXID, Value::Bytes(self.txid.to_vec()));
        m.insert(f::TXID_AMOUNT, Value::Uint(self.amount_pxmr));
        m.insert(f::TXID_TS, Value::Uint(self.timestamp));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::TxId) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not TXID",
            ));
        }
        let out = TxId {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            accept_link: r.bytes(f::TXID_ACCEPT_LINK, Some(32))?.try_into().unwrap(),
            txid: r.bytes(f::TXID_TXID, Some(32))?.try_into().unwrap(),
            amount_pxmr: r.uint(f::TXID_AMOUNT)?,
            timestamp: r.uint(f::TXID_TS)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// Structural checks a payee runs *before* spending effort scanning.
///
/// Returns the amount the payee must find on the chain. It deliberately returns
/// the **ACCEPT's** figure rather than the TXID's, so a caller that follows this
/// API cannot accidentally scan for the amount the payer asserted.
pub fn check_txid(txid: &TxId, accept: &Accept, accept_bytes: &[u8]) -> Result<u64, Reject> {
    let link = commit(Purpose::ChainLink, accept_bytes);
    if !commit_eq(&txid.accept_link, &link) {
        return Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "TXID does not reference this ACCEPT",
        ));
    }
    // An underpayment announced honestly is still an underpayment. Catching it
    // here saves a mempool scan, but the scan is what actually decides.
    if txid.amount_pxmr != accept.amount_final {
        return Err(Reject::with_detail(
            RejectCode::PriceMismatch,
            "TXID names an amount other than the one accepted",
        ));
    }
    Ok(accept.amount_final)
}

// ---------------------------------------------------------------------------
// TXPROOF — for an arbiter, who cannot scan
// ---------------------------------------------------------------------------

/// A Monero transaction proof, bound to one dispute.
///
/// `proof` is opaque to DUCAT: verifying it is Monero's job, done by the arbiter
/// against a node. What DUCAT specifies is the **binding**, and that is the part
/// an implementer will leave out.
///
/// Monero's proof covers an arbitrary `message` chosen at generation time. Set
/// it to the transcript's chain link and the proof becomes non-transferable
/// between disputes; leave it empty — the obvious implementation — and any proof
/// the payer ever generated for that transaction can be replayed into an
/// unrelated claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxProof {
    pub version: u64,
    pub suite: u8,
    pub txid: [u8; 32],
    /// The Monero `OutProofV2`/`InProofV2` blob.
    pub proof: Vec<u8>,
    /// Address the proof is *about*. A proof without a stated destination
    /// proves that a transaction exists, which nobody disputed.
    pub destination: Vec<u8>,
    /// The message signed into the proof. MUST be the transcript chain link.
    pub proof_message: [u8; 32],
    pub amount_pxmr: u64,
    pub timestamp: u64,
}

impl TxProof {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::TxProof)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::PRF_TXID, Value::Bytes(self.txid.to_vec()));
        m.insert(f::PRF_PROOF, Value::Bytes(self.proof.clone()));
        m.insert(f::PRF_DESTINATION, Value::Bytes(self.destination.clone()));
        m.insert(f::PRF_MESSAGE, Value::Bytes(self.proof_message.to_vec()));
        m.insert(f::PRF_AMOUNT, Value::Uint(self.amount_pxmr));
        m.insert(f::PRF_TS, Value::Uint(self.timestamp));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::TxProof) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not TXPROOF",
            ));
        }
        let out = TxProof {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            txid: r.bytes(f::PRF_TXID, Some(32))?.try_into().unwrap(),
            proof: r.bytes(f::PRF_PROOF, None)?,
            destination: r.bytes(f::PRF_DESTINATION, None)?,
            proof_message: r.bytes(f::PRF_MESSAGE, Some(32))?.try_into().unwrap(),
            amount_pxmr: r.uint(f::PRF_AMOUNT)?,
            timestamp: r.uint(f::PRF_TS)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// Everything an arbiter can check without touching Monero.
///
/// Returns the `(txid, destination, message)` triple to hand to
/// `check_tx_proof` on a node. Passing this does **not** mean the proof is
/// valid — it means the proof is *about the right thing*, which is the question
/// the chain cannot answer.
pub fn check_tx_proof_binding<'a>(
    proof: &'a TxProof,
    accept_bytes: &[u8],
    expected_destination: &[u8],
    expected_amount: u64,
) -> Result<(&'a [u8; 32], &'a [u8], &'a [u8; 32]), Reject> {
    let link = commit(Purpose::ChainLink, accept_bytes);
    if !commit_eq(&proof.proof_message, &link) {
        return Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "proof is not bound to this transcript; it could have been generated \
             for another dispute",
        ));
    }
    if proof.destination != expected_destination {
        return Err(Reject::with_detail(
            RejectCode::PolicyRefused,
            "proof is about payment to a different address",
        ));
    }
    if proof.amount_pxmr < expected_amount {
        return Err(Reject::with_detail(
            RejectCode::PriceMismatch,
            "proof claims less than the amount at issue",
        ));
    }
    if proof.proof.is_empty() {
        return Err(Reject::with_detail(
            RejectCode::Malformed,
            "empty proof",
        ));
    }
    Ok((&proof.txid, &proof.destination, &proof.proof_message))
}

// ---------------------------------------------------------------------------
// SLASH_CLAIM (§17.5)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SlashReason {
    /// The transaction never confirmed and the cure window has run out.
    CureWindowExpired = 1,
    /// A conflicting key image is on chain. Unambiguous, self-authenticating,
    /// and therefore exempt from the cure window (§17.5).
    ConflictingKeyImage = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashClaim {
    pub version: u64,
    pub suite: u8,
    pub accept_link: [u8; 32],
    pub receipt_link: [u8; 32],
    pub txid: [u8; 32],
    pub reason: SlashReason,
    /// Required for `ConflictingKeyImage`, forbidden otherwise.
    pub key_image: Option<[u8; 32]>,
    pub claim_pxmr: u64,
    pub timestamp: u64,
}

impl SlashClaim {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::SlashClaim)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::SLC_ACCEPT_LINK, Value::Bytes(self.accept_link.to_vec()));
        m.insert(f::SLC_RECEIPT_LINK, Value::Bytes(self.receipt_link.to_vec()));
        m.insert(f::SLC_TXID, Value::Bytes(self.txid.to_vec()));
        m.insert(f::SLC_REASON, Value::Uint(self.reason as u64));
        if let Some(ki) = &self.key_image {
            m.insert(f::SLC_KEY_IMAGE, Value::Bytes(ki.to_vec()));
        }
        m.insert(f::SLC_AMOUNT, Value::Uint(self.claim_pxmr));
        m.insert(f::SLC_TS, Value::Uint(self.timestamp));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::SlashClaim) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not SLASH_CLAIM",
            ));
        }
        let version = r.uint(f::VERSION)?;
        let suite = r.uint(f::SUITE)? as u8;
        let accept_link = r.bytes(f::SLC_ACCEPT_LINK, Some(32))?.try_into().unwrap();
        let receipt_link = r.bytes(f::SLC_RECEIPT_LINK, Some(32))?.try_into().unwrap();
        let txid = r.bytes(f::SLC_TXID, Some(32))?.try_into().unwrap();
        let reason = match r.uint(f::SLC_REASON)? {
            1 => SlashReason::CureWindowExpired,
            2 => SlashReason::ConflictingKeyImage,
            other => {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    format!("unknown slash reason {}", other),
                ))
            }
        };
        let key_image = r
            .opt_bytes(f::SLC_KEY_IMAGE, Some(32))?
            .map(|b| <[u8; 32]>::try_from(b).unwrap());
        let out = SlashClaim {
            version,
            suite,
            accept_link,
            receipt_link,
            txid,
            reason,
            key_image,
            claim_pxmr: r.uint(f::SLC_AMOUNT)?,
            timestamp: r.uint(f::SLC_TS)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// Whether an arbiter should entertain this claim.
///
/// `elapsed_blocks` is how many blocks have passed since the transaction was
/// announced; `cure_blocks` is the market's cure window (§17.5, default 20).
pub fn check_slash_claim(
    claim: &SlashClaim,
    accept: &Accept,
    accept_bytes: &[u8],
    receipt: &Receipt,
    receipt_bytes: &[u8],
    elapsed_blocks: u64,
    cure_blocks: u64,
) -> Result<(), Reject> {
    if !commit_eq(&claim.accept_link, &commit(Purpose::ChainLink, accept_bytes)) {
        return Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "claim does not reference this ACCEPT",
        ));
    }
    if !commit_eq(&claim.receipt_link, &commit(Purpose::ChainLink, receipt_bytes)) {
        return Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "claim does not reference this RECEIPT",
        ));
    }
    // The receipt is what fixes the amount owed; the ACCEPT is what fixes what
    // was agreed. A claim exceeding either is a claimant helping themselves.
    if claim.claim_pxmr > accept.amount_final || claim.claim_pxmr > receipt.amount_final {
        return Err(Reject::with_detail(
            RejectCode::PriceMismatch,
            "claim exceeds the amount agreed",
        ));
    }

    match claim.reason {
        SlashReason::CureWindowExpired => {
            // Non-confirmation is usually a fee problem, not fraud (§17.5). The
            // window exists so an honest payer can re-broadcast or bump.
            if elapsed_blocks < cure_blocks {
                return Err(Reject::with_detail(
                    RejectCode::PolicyRefused,
                    "cure window has not expired",
                ));
            }
            if claim.key_image.is_some() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a cure-window claim carries no key image",
                ));
            }
        }
        SlashReason::ConflictingKeyImage => {
            // This reason skips the waiting period, which makes it precisely the
            // one worth forging. It is therefore the one that must carry its
            // evidence: an assertion of double-spend with nothing to check is
            // a claim that the cure window does not apply, made by the party
            // who benefits from that.
            if claim.key_image.is_none() {
                return Err(Reject::with_detail(
                    RejectCode::Malformed,
                    "a double-spend claim skips the cure window and must carry \
                     the conflicting key image",
                ));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// ESCROW_SETUP — the multisig ceremony
// ---------------------------------------------------------------------------

pub const BUYER: u8 = 0;
pub const SELLER: u8 = 1;
pub const ARBITER: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscrowSetup {
    pub version: u64,
    pub suite: u8,
    pub escrow_id: [u8; 32],
    /// Ceremony round, starting at 0. Strictly sequential — see [`RoundTracker`].
    pub round: u64,
    /// Opaque multisig info for this round.
    pub info: Vec<u8>,
    /// `BUYER`, `SELLER`, or `ARBITER`.
    pub from_index: u8,
    pub timestamp: u64,
}

impl EscrowSetup {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::EscrowSetup)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::ESC_ID, Value::Bytes(self.escrow_id.to_vec()));
        m.insert(f::ESC_ROUND, Value::Uint(self.round));
        m.insert(f::ESC_INFO, Value::Bytes(self.info.clone()));
        m.insert(f::ESC_FROM, Value::Uint(self.from_index as u64));
        m.insert(f::ESC_TS, Value::Uint(self.timestamp));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::EscrowSetup) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not ESCROW_SETUP",
            ));
        }
        let out = EscrowSetup {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            escrow_id: r.bytes(f::ESC_ID, Some(32))?.try_into().unwrap(),
            round: r.uint(f::ESC_ROUND)?,
            info: r.bytes(f::ESC_INFO, None)?,
            from_index: r.uint(f::ESC_FROM)? as u8,
            timestamp: r.uint(f::ESC_TS)?,
        };
        if out.from_index > ARBITER {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "participant index outside the 2-of-3 group",
            ));
        }
        if out.info.is_empty() {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "empty multisig info contributes nothing to the ceremony",
            ));
        }
        r.finish()?;
        Ok(out)
    }
}

/// Accepts ceremony messages in order and refuses everything else.
///
/// **This is §2.5's countermeasure, and it is the reason this type exists rather
/// than a `Vec<EscrowSetup>` the caller filters.** RetoSwap was drained by a
/// forged, *out-of-order* message that overwrote state the protocol had already
/// settled. A ceremony that accepts round *n* while expecting round *n+1* has
/// the same shape, whatever the payload.
///
/// The rules are deliberately unforgiving: exactly the expected round, exactly
/// once per participant, and no revisions.
#[derive(Debug, Clone)]
pub struct RoundTracker {
    escrow_id: [u8; 32],
    round: u64,
    seen: [bool; 3],
    rounds_required: u64,
}

impl RoundTracker {
    /// A 2-of-3 wallet2 ceremony converges in 2 rounds (measured, §O1).
    pub fn new(escrow_id: [u8; 32], rounds_required: u64) -> Self {
        RoundTracker {
            escrow_id,
            round: 0,
            seen: [false; 3],
            rounds_required,
        }
    }

    pub fn current_round(&self) -> u64 {
        self.round
    }

    pub fn complete(&self) -> bool {
        self.round >= self.rounds_required
    }

    /// Accept one message. Returns `true` when the round just closed.
    pub fn accept(&mut self, s: &EscrowSetup) -> Result<bool, Reject> {
        if self.complete() {
            return Err(Reject::with_detail(
                RejectCode::StateViolation,
                "the ceremony is finished; there is nothing left to contribute",
            ));
        }
        if s.escrow_id != self.escrow_id {
            return Err(Reject::with_detail(
                RejectCode::PolicyRefused,
                "setup message belongs to a different escrow",
            ));
        }
        if s.round != self.round {
            return Err(Reject::with_detail(
                RejectCode::StateViolation,
                format!(
                    "expected ceremony round {}, got {} — out-of-order setup \
                     messages are how §2.5's exploit worked",
                    self.round, s.round
                ),
            ));
        }
        if self.seen[s.from_index as usize] {
            return Err(Reject::with_detail(
                RejectCode::Replay,
                "this participant has already contributed to this round; a \
                 second contribution would revise settled state",
            ));
        }
        self.seen[s.from_index as usize] = true;
        if self.seen.iter().all(|x| *x) {
            self.round += 1;
            self.seen = [false; 3];
            return Ok(true);
        }
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// ESCROW_READY
// ---------------------------------------------------------------------------

/// One participant's report of what the ceremony produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscrowReady {
    pub version: u64,
    pub suite: u8,
    pub escrow_id: [u8; 32],
    /// The formed multisig address.
    pub ms_address: Vec<u8>,
    pub threshold: u8,
    pub total: u8,
    /// The arbiter's key, as this participant understands it.
    pub arbiter: Vec<u8>,
    pub from_index: u8,
    pub timestamp: u64,
}

impl EscrowReady {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::EscrowReady)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::RDY_ID, Value::Bytes(self.escrow_id.to_vec()));
        m.insert(f::RDY_ADDRESS, Value::Bytes(self.ms_address.clone()));
        m.insert(f::RDY_THRESHOLD, Value::Uint(self.threshold as u64));
        m.insert(f::RDY_TOTAL, Value::Uint(self.total as u64));
        m.insert(f::RDY_ARBITER, Value::Bytes(self.arbiter.clone()));
        m.insert(f::RDY_FROM, Value::Uint(self.from_index as u64));
        m.insert(f::RDY_TS, Value::Uint(self.timestamp));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::EscrowReady) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not ESCROW_READY",
            ));
        }
        let out = EscrowReady {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            escrow_id: r.bytes(f::RDY_ID, Some(32))?.try_into().unwrap(),
            ms_address: r.bytes(f::RDY_ADDRESS, None)?,
            threshold: r.uint(f::RDY_THRESHOLD)? as u8,
            total: r.uint(f::RDY_TOTAL)? as u8,
            arbiter: r.bytes(f::RDY_ARBITER, None)?,
            from_index: r.uint(f::RDY_FROM)? as u8,
            timestamp: r.uint(f::RDY_TS)?,
        };
        if out.from_index > ARBITER {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "participant index outside the 2-of-3 group",
            ));
        }
        r.finish()?;
        Ok(out)
    }
}

/// Agree that the ceremony produced one wallet, and that it is the right one.
///
/// `trusted_arbiters` comes from the market descriptor (§10.1) and is passed in
/// **as an argument on purpose**: §2.5's exploit installed an arbitrator address
/// that arrived in a message. An API that read the arbiter out of the reports
/// would reproduce that, and no amount of signature checking helps — the forged
/// message was well-formed.
///
/// Returns the agreed multisig address.
pub fn check_escrow_ready<'a>(
    reports: &'a [EscrowReady],
    escrow_id: &[u8; 32],
    trusted_arbiters: &[Vec<u8>],
) -> Result<&'a [u8], Reject> {
    if reports.len() != 3 {
        return Err(Reject::with_detail(
            RejectCode::PolicyRefused,
            "every participant must report what it formed; a silent one has \
             not agreed to anything",
        ));
    }
    let mut seen = [false; 3];
    for rep in reports {
        if &rep.escrow_id != escrow_id {
            return Err(Reject::with_detail(
                RejectCode::PolicyRefused,
                "report belongs to a different escrow",
            ));
        }
        if seen[rep.from_index as usize] {
            return Err(Reject::with_detail(
                RejectCode::Replay,
                "two reports from the same participant",
            ));
        }
        seen[rep.from_index as usize] = true;
        if rep.threshold != 2 || rep.total != 3 {
            return Err(Reject::with_detail(
                RejectCode::PolicyRefused,
                "escrow must be 2-of-3",
            ));
        }
        // The comparison that matters. Three parties can each complete a
        // ceremony successfully and end up in different groups; the funds then
        // go to a wallet the payer does not control a share of.
        if rep.ms_address != reports[0].ms_address {
            return Err(Reject::with_detail(
                RejectCode::CommitMismatch,
                "participants formed different multisig wallets",
            ));
        }
        if rep.arbiter != reports[0].arbiter {
            return Err(Reject::with_detail(
                RejectCode::UntrustedArbiterSet,
                "participants disagree about who the arbiter is",
            ));
        }
    }
    if reports[0].ms_address.is_empty() {
        return Err(Reject::with_detail(
            RejectCode::Malformed,
            "empty multisig address",
        ));
    }
    if !trusted_arbiters.iter().any(|a| *a == reports[0].arbiter) {
        return Err(Reject::with_detail(
            RejectCode::UntrustedArbiterSet,
            "arbiter is not in the market's signed set (§2.5)",
        ));
    }
    Ok(&reports[0].ms_address)
}

// ---------------------------------------------------------------------------
// RELEASE
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: u64,
    pub suite: u8,
    pub escrow_id: [u8; 32],
    /// Chain link to the ESCROW_READY this releases against.
    pub ready_link: [u8; 32],
    pub to: Vec<u8>,
    pub amount_pxmr: u64,
    pub timestamp: u64,
}

impl Release {
    pub fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(f::TYPE, Value::Uint(type_code(ObjectType::Release)));
        m.insert(f::VERSION, Value::Uint(self.version));
        m.insert(f::SUITE, Value::Uint(self.suite as u64));
        m.insert(f::REL_ID, Value::Bytes(self.escrow_id.to_vec()));
        m.insert(f::REL_READY_LINK, Value::Bytes(self.ready_link.to_vec()));
        m.insert(f::REL_TO, Value::Bytes(self.to.clone()));
        m.insert(f::REL_AMOUNT, Value::Uint(self.amount_pxmr));
        m.insert(f::REL_TS, Value::Uint(self.timestamp));
        Value::Map(m)
    }

    pub fn from_value(v: Value) -> Result<Self, Reject> {
        let mut r = Reader::new(v)?;
        if r.uint(f::TYPE)? != type_code(ObjectType::Release) {
            return Err(Reject::with_detail(
                RejectCode::Malformed,
                "object type is not RELEASE",
            ));
        }
        let out = Release {
            version: r.uint(f::VERSION)?,
            suite: r.uint(f::SUITE)? as u8,
            escrow_id: r.bytes(f::REL_ID, Some(32))?.try_into().unwrap(),
            ready_link: r.bytes(f::REL_READY_LINK, Some(32))?.try_into().unwrap(),
            to: r.bytes(f::REL_TO, None)?,
            amount_pxmr: r.uint(f::REL_AMOUNT)?,
            timestamp: r.uint(f::REL_TS)?,
        };
        r.finish()?;
        Ok(out)
    }
}

/// Whether this release may be co-signed.
///
/// `allowed_destinations` is the set of addresses the escrow may pay out to —
/// in practice the buyer's refund address and the seller's payout address.
/// **An escrow that will pay any address on request is not an escrow**, and this
/// is the check that a rushed implementation drops, because the happy path never
/// exercises it: both parties co-signing a release to the seller looks identical
/// whether or not the destination was ever constrained.
pub fn check_release(
    rel: &Release,
    ready: &EscrowReady,
    ready_bytes: &[u8],
    escrowed_pxmr: u64,
    allowed_destinations: &[Vec<u8>],
) -> Result<(), Reject> {
    if rel.escrow_id != ready.escrow_id {
        return Err(Reject::with_detail(
            RejectCode::PolicyRefused,
            "release names a different escrow than the one it links to",
        ));
    }
    if !commit_eq(&rel.ready_link, &commit(Purpose::ChainLink, ready_bytes)) {
        return Err(Reject::with_detail(
            RejectCode::CommitMismatch,
            "release does not reference this escrow's formation",
        ));
    }
    if rel.amount_pxmr == 0 {
        return Err(Reject::with_detail(
            RejectCode::PriceMismatch,
            "a release of zero moves nothing and closes nothing",
        ));
    }
    if rel.amount_pxmr > escrowed_pxmr {
        return Err(Reject::with_detail(
            RejectCode::PriceMismatch,
            "release exceeds what is held",
        ));
    }
    if !allowed_destinations.iter().any(|d| *d == rel.to) {
        return Err(Reject::with_detail(
            RejectCode::PolicyRefused,
            "release destination is not a party to this escrow",
        ));
    }
    Ok(())
}
