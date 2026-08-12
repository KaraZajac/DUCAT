//! The contract state machine, per protocol §18.4 with deadlines from §6.2.
//!
//! Pure: no I/O, no clock, no network. Time enters as an explicit `Elapsed`
//! event carrying a monotonic duration, because §6.2 requires elapsed time be
//! measured monotonically — wall-clock timeouts break when a phone's clock
//! moves, and a payer must not lose a fare to a daylight-saving transition.
//!
//! The normative rule this module enforces above all others: **a message not
//! listed for the current state is a `STATE_VIOLATION`, never a silent ignore.**
//! Silently dropping unexpected messages is how two implementations diverge
//! invisibly — both "work", neither agrees, and nothing surfaces until money
//! moves.

use crate::reject::{Reject, RejectCode};
use std::time::Duration;

/// Which side of the transaction this client is running.
///
/// Both sides run the same machine over the same events, but not every message
/// is legal from every side — only the payer may ACCEPT, only the payee may
/// REFUND — so direction is checked rather than assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Payer,
    Payee,
}

/// Settlement mode named in ACCEPT (§8). Governs which post-FUND path is legal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleMode {
    Direct,
    Fast,
    Escrow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    /// Bootstrap verified; awaiting `FullOffer` over the channel it opened.
    Offered,
    /// Offer verified against `offer_commit`; confirm screen rendered.
    Quoted,
    /// Payer signed ACCEPT; price locked.
    Accepted,
    /// Payment broadcast, or escrow funded.
    Funded,
    /// `fast/1` only: TXPROOF verified, service may proceed, awaiting finality.
    Provisional,
    /// Profile-defined PROOF exchanged.
    Delivered,
    /// RECEIPT co-signed — the normal terminal state.
    Closed,
    /// `fast/1` only: finality observed, obligation cleared.
    Settled,
    /// `fast/1` only: cure window expired unconfirmed, or a conflicting key
    /// image landed. A slash claim is now fileable (§17.5).
    Claimed,
    Aborted,
    Cancelled,
    Disputed,
}

impl State {
    /// Terminal states accept no further events. Distinguished from `Closed`,
    /// which still admits the contact coda and `fast/1` finality.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            State::Aborted | State::Cancelled | State::Disputed | State::Settled | State::Claimed
        )
    }
}

/// Protocol events. Message variants mean "a well-formed, signature-verified
/// message of this type arrived" — parsing and signature checks happen before
/// the machine sees anything, so the machine reasons only about sequencing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    TapPresent,
    FullOffer,
    Accept,
    Fund,
    TxProof,
    Proof,
    Receipt,
    Cancel,
    Dispute,
    Abort,
    ContactOffer,
    ContactAccept,
    /// Monotonic time in the current state.
    Elapsed(Duration),
    /// `fast/1`: the funding transaction reached the finality threshold.
    ConfirmationsReached,
    /// `fast/1`: cure window elapsed with the transaction still unconfirmed, or
    /// a conflicting key image was observed on-chain (§17.5).
    CureWindowExpired,
}

/// Consequences a transition demands of the caller. The machine decides these;
/// it does not perform them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Discard without surfacing anything to the human. Load-bearing: a hostile
    /// or malfunctioning tap must never be able to put a screen in front of a
    /// user, since the confirm screen is the security boundary (§15.5).
    DiscardSilently,
    /// Emit a unilateral record of `{ACCEPT, TXPROOF, timestamp}` (§6.2). The
    /// counterparty vanished after taking payment and before co-signing; this
    /// keeps the payer's evidence intact without claiming delivery occurred.
    EmitSingleSidedReceipt,
    /// `fast/1`: obligation cleared, bond capacity restored (§17.2).
    ReleaseCapacity,
    /// `fast/1`: a slash claim is now fileable against the payer's bond.
    FileSlashClaim,
    /// Escrow: multisig setup timed out; run the fund-recovery path (§8.2).
    RecoverEscrowFunds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub next: State,
    pub effect: Effect,
}

fn go(next: State) -> Result<Transition, Reject> {
    Ok(Transition {
        next,
        effect: Effect::None,
    })
}

fn go_with(next: State, effect: Effect) -> Result<Transition, Reject> {
    Ok(Transition { next, effect })
}

fn violation(state: State, event: &Event) -> Reject {
    Reject::with_detail(
        RejectCode::StateViolation,
        format!("{:?} is not legal in {:?}", event, state),
    )
}

/// Per-state deadlines from §6.2.
///
/// `None` means either profile-defined (delivery, escrow release) or not
/// wall-clock bounded (`fast/1` finality is counted in blocks, not seconds).
pub fn deadline(state: State, mode: SettleMode) -> Option<Duration> {
    match state {
        State::Offered => Some(Duration::from_secs(10)),
        // Bounded by TapPresent.expiry, which is <= 30 s.
        State::Quoted => Some(Duration::from_secs(30)),
        State::Accepted => Some(Duration::from_secs(match mode {
            // Escrow spends this window on multisig setup, which is multi-round
            // and needs far longer than a direct broadcast (§8.2).
            SettleMode::Escrow => 300,
            _ => 60,
        })),
        // Only fast/1 awaits a TXPROOF here; other modes await profile-defined
        // delivery and are not wall-clock bounded.
        State::Funded if mode == SettleMode::Fast => Some(Duration::from_secs(30)),
        State::Delivered => Some(Duration::from_secs(120)),
        // Post-RECEIPT contact window (§4): session keys outlive RECEIPT by
        // this much so the optional CONTACT coda can run, then are destroyed.
        State::Closed => Some(Duration::from_secs(120)),
        _ => None,
    }
}

/// Apply one event. `role` is the side running this machine; `mode` is the
/// settlement mode named in ACCEPT (before ACCEPT it is the mode being offered).
pub fn transition(
    state: State,
    role: Role,
    mode: SettleMode,
    event: &Event,
) -> Result<Transition, Reject> {
    use Event as E;
    use State as S;

    // Terminal states are absorbing. Checked first so no later arm can
    // accidentally resurrect a cancelled or disputed transaction.
    if state.is_terminal() {
        return Err(violation(state, event));
    }

    match (state, event) {
        // -- bootstrap ---------------------------------------------------
        (S::Idle, E::TapPresent) => go(S::Offered),

        (S::Offered, E::FullOffer) => go(S::Quoted),
        // A tap that never delivers its offer leaves no trace and shows nothing.
        (S::Offered, E::Elapsed(d)) if past(*d, state, mode) => {
            go_with(S::Idle, Effect::DiscardSilently)
        }

        // -- quote and lock ----------------------------------------------
        // Only the payer signs ACCEPT. A payee "accepting" its own offer would
        // let a hostile terminal drive the flow without the human checkpoint.
        (S::Quoted, E::Accept) if role == Role::Payer => go(S::Accepted),
        (S::Quoted, E::Accept) => Err(Reject::with_detail(
            RejectCode::StateViolation,
            "ACCEPT may only originate from the payer",
        )),
        (S::Quoted, E::Abort) => go(S::Aborted),
        (S::Quoted, E::Elapsed(d)) if past(*d, state, mode) => go(S::Aborted),

        // -- funding -----------------------------------------------------
        (S::Accepted, E::Fund) => go(S::Funded),
        // Cancellation is legal only once a price is locked and before funds
        // move; its fee comes from terms the payer already signed (§7.3).
        (S::Accepted, E::Cancel) => go(S::Cancelled),
        (S::Accepted, E::Abort) => go(S::Aborted),
        (S::Accepted, E::Elapsed(d)) if past(*d, state, mode) => {
            if mode == SettleMode::Escrow {
                // Multisig setup stalled with funds possibly committed; the
                // recovery path exists precisely so they are not stranded.
                go_with(S::Aborted, Effect::RecoverEscrowFunds)
            } else {
                go(S::Aborted)
            }
        }

        // -- fast/1 zero-conf --------------------------------------------
        (S::Funded, E::TxProof) if mode == SettleMode::Fast => go(S::Provisional),
        (S::Funded, E::TxProof) => Err(Reject::with_detail(
            RejectCode::StateViolation,
            "TXPROOF is only meaningful under fast/1",
        )),
        // No proof inside the window: fall back to waiting for confirmations
        // rather than accepting unbacked risk.
        (S::Funded, E::Elapsed(d)) if mode == SettleMode::Fast && past(*d, state, mode) => {
            go(S::Aborted)
        }

        // -- delivery ----------------------------------------------------
        (S::Funded, E::Proof) | (S::Provisional, E::Proof) => go(S::Delivered),

        // -- closure -----------------------------------------------------
        (S::Delivered, E::Receipt) => go(S::Closed),
        // The counterparty went silent holding the money. The payer keeps a
        // signed record of what it paid; it proves payment, not delivery.
        (S::Delivered, E::Elapsed(d)) if past(*d, state, mode) => {
            go_with(S::Closed, Effect::EmitSingleSidedReceipt)
        }

        // -- fast/1 finality ---------------------------------------------
        (S::Closed, E::ConfirmationsReached) if mode == SettleMode::Fast => {
            go_with(S::Settled, Effect::ReleaseCapacity)
        }
        (S::Closed, E::CureWindowExpired) if mode == SettleMode::Fast => {
            go_with(S::Claimed, Effect::FileSlashClaim)
        }

        // -- optional identity coda (§16.3) ------------------------------
        // Contact is a side effect, not a state change: the transaction is
        // already complete and must not be reopened by it.
        (S::Closed, E::ContactOffer) | (S::Closed, E::ContactAccept) => go(S::Closed),
        // Contact window elapsed with no exchange: tear down, keys destroyed,
        // zero persistent trace. Declining requires doing nothing (§16.3).
        (S::Closed, E::Elapsed(d)) if past(*d, state, mode) => go(S::Closed),

        // -- disputes ----------------------------------------------------
        // Only escrow modes have funds an arbiter can move. Under direct or
        // fast settlement there is nothing to arbitrate: the money is gone, and
        // fast/1's recourse is the slash path, not a dispute.
        (S::Funded, E::Dispute) | (S::Provisional, E::Dispute) | (S::Delivered, E::Dispute)
            if mode == SettleMode::Escrow =>
        {
            go(S::Disputed)
        }

        // Timeouts in states with no wall-clock deadline are not errors; the
        // caller may poll freely.
        (_, E::Elapsed(_)) => go(state),

        // §18.4: anything unlisted is a violation, never a silent ignore.
        _ => Err(violation(state, event)),
    }
}

fn past(elapsed: Duration, state: State, mode: SettleMode) -> bool {
    match deadline(state, mode) {
        Some(d) => elapsed >= d,
        None => false,
    }
}
