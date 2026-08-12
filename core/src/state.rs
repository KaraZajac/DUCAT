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

/// Which side of the transaction a party is on.
///
/// Used two ways, and conflating them is a bug the simulator caught: `role` in
/// `transition` is *who is running this machine*, while `Event::Accept { from }`
/// is *who originated the message*. Directional rules constrain the originator,
/// never the evaluator — both parties must reach the same verdict about the
/// same message, and a payee that refused every ACCEPT because it was not the
/// payer could never complete a transaction.
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
    /// §15.7: a meter is running. The payer confirmed a rate and a cap at
    /// `start`; the total is not known until `stop`.
    ///
    /// This state exists because §15.7's two-tap flow and §6.2's deadlines were
    /// written independently and disagreed: without it a metered session sits in
    /// `Accepted` and its 60-second deadline aborts a bar tab after one minute.
    Metering,
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
    /// `from` is the originator, established by signature before the machine
    /// sees it. §18.4.1 constrains who may *emit* an ACCEPT, not who may
    /// process one.
    Accept {
        from: Role,
    },
    Fund,
    TxProof,
    Proof,
    Receipt,
    Cancel,
    Dispute,
    /// `from` is the originator. Direction is unconstrained before value
    /// accrues, and payee-only once a meter is running — see the `METERING`
    /// arm, and §18.4.1.
    Abort {
        from: Role,
    },
    ContactOffer,
    ContactAccept,
    /// Monotonic time in the current state.
    Elapsed(Duration),
    /// `fast/1`: the funding transaction reached the finality threshold.
    ConfirmationsReached,
    /// `fast/1`: cure window elapsed with the transaction still unconfirmed, or
    /// a conflicting key image was observed on-chain (§17.5).
    CureWindowExpired,
    /// §15.7: the payer confirmed a rate and cap; the meter is now running.
    MeterStart,
    /// §15.7: a `stop` tap arrived carrying a matching `session_ref`, with the
    /// total derived from elapsed time or distance.
    MeterStop,
    /// The profile's delivery window elapsed with no `PROOF`.
    ///
    /// A backstop, and the audit in §6.2 exists because it was missing: before
    /// this, `FUNDED` had no deadline under `direct` or `escrow`, so a payer who
    /// had already paid and never received proof waited **forever** with no exit
    /// and no evidence. "Profile-defined" is a deferral, not an action, and an
    /// undefined profile meant unbounded.
    ///
    /// Signalled by the caller because the window belongs to the profile, which
    /// the machine does not hold — the same reason `MeterExpired` is an event.
    DeliveryWindowExpired,
    /// §15.7: the meter ran past `terms.meter_max_s` without a `stop`.
    ///
    /// Signalled by the caller rather than by a wall-clock deadline, because the
    /// limit lives in `terms` and the machine holds no terms — the same pattern
    /// as `ConfirmationsReached` and `CureWindowExpired`.
    MeterExpired,
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
    /// §6.2. The counterparty vanished *after taking payment* and before
    /// co-signing. The **payer** records `{ACCEPT, TXID, timestamp}`, which
    /// proves what they paid without claiming delivery occurred.
    EmitPaymentEvidence,
    /// §15.7. The payer walked away from a running meter. The **payee** records
    /// what accrued, capped by what was agreed.
    ///
    /// Deliberately distinct from `EmitPaymentEvidence` although both produce a
    /// unilateral receipt: they are opposite claims. One says *I paid and hold
    /// no co-signature*; the other says *you owe me and never stopped the
    /// meter*. A single effect covering both would leave a client to infer the
    /// direction from the state it just left, which defeats the point of
    /// returning an instruction at all.
    EmitDebtEvidence,
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
    _role: Role,
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
        // Both parties evaluate this identically: the constraint is on the
        // originator, which the signature establishes, not on who is asking.
        (S::Quoted, E::Accept { from: Role::Payer }) => go(S::Accepted),
        (S::Quoted, E::Accept { .. }) => Err(Reject::with_detail(
            RejectCode::StateViolation,
            "ACCEPT may only originate from the payer",
        )),
        (S::Quoted, E::Abort { .. }) => go(S::Aborted),
        (S::Quoted, E::Elapsed(d)) if past(*d, state, mode) => go(S::Aborted),

        // -- metering (§15.7) ---------------------------------------------
        // The rate and cap were confirmed at `start`; the total is not known
        // until `stop`, so this state is deliberately not wall-clock bounded.
        (S::Quoted, E::MeterStart) => go(S::Metering),
        (S::Metering, E::MeterStop) => go(S::Accepted),
        // Abandonment: the customer left without stopping the meter. The payee
        // computes what accrued, capped by what the payer agreed to, and emits
        // a unilateral record. Whether any of it is collectable depends
        // entirely on collateral (§15.7) — against an unbonded payer it is not.
        (S::Metering, E::MeterExpired) => {
            go_with(S::Closed, Effect::EmitDebtEvidence)
        }
        // Only the meter's operator may void it cleanly — a bartender comping a
        // tab is ordinary. A *payer* aborting a running meter would be a free
        // exit from an obligation that has been accruing in real time: start a
        // tab, consume, abort, owe nothing. That path is abandonment, and it
        // goes through `MeterExpired` so it leaves a single-sided receipt as
        // evidence rather than a clean exit with no record (§15.7).
        (S::Metering, E::Abort { from: Role::Payee }) => go(S::Aborted),
        (S::Metering, E::Abort { .. }) => Err(Reject::with_detail(
            RejectCode::StateViolation,
            "a payer cannot abort a running meter; leaving is abandonment (§15.7)",
        )),

        // -- funding -----------------------------------------------------
        (S::Accepted, E::Fund) => go(S::Funded),
        // Cancellation is legal only once a price is locked and before funds
        // move; its fee comes from terms the payer already signed (§7.3).
        (S::Accepted, E::Cancel) => go(S::Cancelled),
        (S::Accepted, E::Abort { .. }) => go(S::Aborted),
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
        // The backstop. The payer has paid; if delivery never comes they must
        // still be able to close out holding evidence of what they paid, rather
        // than waiting on a counterparty that may never return.
        (S::Funded, E::DeliveryWindowExpired) | (S::Provisional, E::DeliveryWindowExpired) => {
            go_with(S::Closed, Effect::EmitPaymentEvidence)
        }

        // -- closure -----------------------------------------------------
        (S::Delivered, E::Receipt) => go(S::Closed),
        // The counterparty went silent holding the money. The payer keeps a
        // signed record of what it paid; it proves payment, not delivery.
        (S::Delivered, E::Elapsed(d)) if past(*d, state, mode) => {
            go_with(S::Closed, Effect::EmitPaymentEvidence)
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
