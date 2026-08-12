//! State machine conformance, per §18.4 with deadlines from §6.2.
//! Covers §18.9(5): every timeout, and the single-sided receipt.

use ducat_core::reject::RejectCode;
use ducat_core::state::*;
use std::time::Duration;

/// Drive a sequence, asserting each step lands where expected.
fn run(mode: SettleMode, role: Role, steps: &[(Event, State)]) {
    let mut s = State::Idle;
    for (i, (ev, want)) in steps.iter().enumerate() {
        let t = transition(s, role, mode, ev)
            .unwrap_or_else(|e| panic!("step {} ({:?}) rejected: {:?}", i, ev, e));
        assert_eq!(t.next, *want, "step {} ({:?})", i, ev);
        s = t.next;
    }
}

#[test]
fn happy_path_direct() {
    run(
        SettleMode::Direct,
        Role::Payer,
        &[
            (Event::TapPresent, State::Offered),
            (Event::FullOffer, State::Quoted),
            (Event::Accept { from: Role::Payer }, State::Accepted),
            (Event::Fund, State::Funded),
            (Event::Proof, State::Delivered),
            (Event::Receipt, State::Closed),
        ],
    );
}

#[test]
fn happy_path_fast_reaches_settled() {
    run(
        SettleMode::Fast,
        Role::Payer,
        &[
            (Event::TapPresent, State::Offered),
            (Event::FullOffer, State::Quoted),
            (Event::Accept { from: Role::Payer }, State::Accepted),
            (Event::Fund, State::Funded),
            (Event::TxId, State::Provisional),
            (Event::Proof, State::Delivered),
            (Event::Receipt, State::Closed),
            (Event::ConfirmationsReached, State::Settled),
        ],
    );
}

#[test]
fn settling_releases_bond_capacity() {
    let t = transition(
        State::Closed,
        Role::Payer,
        SettleMode::Fast,
        &Event::ConfirmationsReached,
    )
    .unwrap();
    assert_eq!(t.effect, Effect::ReleaseCapacity);
}

#[test]
fn cure_window_expiry_makes_a_slash_claim_fileable() {
    let t = transition(
        State::Closed,
        Role::Payee,
        SettleMode::Fast,
        &Event::CureWindowExpired,
    )
    .unwrap();
    assert_eq!(t.next, State::Claimed);
    assert_eq!(t.effect, Effect::FileSlashClaim);
}

// -- the security-relevant timeout ------------------------------------------

/// A tap that never delivers its offer must leave no trace and — critically —
/// must never put a screen in front of the human. The confirm screen is the
/// security boundary (§15.5); a hostile tap that can summon one has already won
/// half the battle.
#[test]
fn abandoned_tap_discards_silently() {
    let t = transition(
        State::Offered,
        Role::Payer,
        SettleMode::Direct,
        &Event::Elapsed(Duration::from_secs(10)),
    )
    .unwrap();
    assert_eq!(t.next, State::Idle);
    assert_eq!(t.effect, Effect::DiscardSilently);
}

#[test]
fn timeouts_fire_at_their_documented_deadlines() {
    let cases = [
        (State::Offered, SettleMode::Direct, 10, State::Idle),
        (State::Quoted, SettleMode::Direct, 30, State::Aborted),
        (State::Accepted, SettleMode::Direct, 60, State::Aborted),
        (State::Accepted, SettleMode::Escrow, 300, State::Aborted),
        (State::Funded, SettleMode::Fast, 30, State::Aborted),
        (State::Delivered, SettleMode::Direct, 120, State::Closed),
    ];
    for (state, mode, secs, want) in cases {
        // One second short: nothing happens, state holds.
        let early = transition(
            state,
            Role::Payer,
            mode,
            &Event::Elapsed(Duration::from_secs(secs - 1)),
        )
        .unwrap();
        assert_eq!(early.next, state, "{:?}/{:?} fired early", state, mode);

        // Exactly at the deadline: fires.
        let due = transition(
            state,
            Role::Payer,
            mode,
            &Event::Elapsed(Duration::from_secs(secs)),
        )
        .unwrap();
        assert_eq!(due.next, want, "{:?}/{:?} at deadline", state, mode);
    }
}

/// Escrow's ACCEPT window is five minutes rather than sixty seconds because
/// multisig setup is multi-round (§8.2), and its expiry must run the recovery
/// path so funds committed mid-setup are not stranded.
#[test]
fn escrow_setup_timeout_triggers_fund_recovery() {
    let t = transition(
        State::Accepted,
        Role::Payer,
        SettleMode::Escrow,
        &Event::Elapsed(Duration::from_secs(300)),
    )
    .unwrap();
    assert_eq!(t.next, State::Aborted);
    assert_eq!(t.effect, Effect::RecoverEscrowFunds);

    // The direct-mode 60 s deadline must not apply to escrow.
    let early = transition(
        State::Accepted,
        Role::Payer,
        SettleMode::Escrow,
        &Event::Elapsed(Duration::from_secs(60)),
    )
    .unwrap();
    assert_eq!(early.next, State::Accepted);
}

/// The dangerous window: money gone, no co-signed record, counterparty silent.
#[test]
fn vanishing_counterparty_yields_a_single_sided_receipt() {
    let t = transition(
        State::Delivered,
        Role::Payer,
        SettleMode::Direct,
        &Event::Elapsed(Duration::from_secs(120)),
    )
    .unwrap();
    assert_eq!(t.next, State::Closed);
    assert_eq!(t.effect, Effect::EmitPaymentEvidence);
}

// -- strictness --------------------------------------------------------------

/// §18.4's headline rule. Silent ignores are how two implementations diverge
/// invisibly, so every unlisted pairing must be a typed refusal.
#[test]
fn unexpected_messages_are_state_violations_not_ignores() {
    let bad = [
        (State::Idle, Event::Accept { from: Role::Payer }),
        (State::Idle, Event::Fund),
        (State::Idle, Event::Receipt),
        (State::Offered, Event::Accept { from: Role::Payer }),
        (State::Quoted, Event::Fund),
        (State::Accepted, Event::Receipt),
        (State::Funded, Event::Accept { from: Role::Payer }),
        (State::Closed, Event::Fund),
    ];
    for (state, ev) in bad {
        let err = transition(state, Role::Payer, SettleMode::Direct, &ev)
            .expect_err(&format!("{:?} in {:?} should be refused", ev, state));
        assert_eq!(err.code, RejectCode::StateViolation);
    }
}

/// Only the payer may *originate* an ACCEPT. A payee able to accept its own
/// offer could drive the whole flow with no human checkpoint.
///
/// The constraint is on the originator, not the evaluator — both parties must
/// reach the same verdict about the same message. An earlier version guarded on
/// the local role, so a payee refused every ACCEPT it received and no
/// transaction could ever complete; the simulator caught it on its first run.
#[test]
fn accept_is_constrained_by_originator_not_evaluator() {
    // A payer-originated ACCEPT is accepted by BOTH parties.
    for who in [Role::Payer, Role::Payee] {
        assert!(
            transition(State::Quoted, who, SettleMode::Direct,
                       &Event::Accept { from: Role::Payer }).is_ok(),
            "{:?} must be able to process a payer-originated ACCEPT", who
        );
    }
    // A payee-originated one is refused by BOTH.
    for who in [Role::Payer, Role::Payee] {
        let err = transition(State::Quoted, who, SettleMode::Direct,
                             &Event::Accept { from: Role::Payee })
            .expect_err("payee-originated ACCEPT must be refused");
        assert_eq!(err.code, RejectCode::StateViolation);
    }
}

/// Under direct and fast settlement there is nothing for an arbiter to move —
/// the money is already gone, and fast/1's recourse is the slash path.
#[test]
fn disputes_require_escrow() {
    for mode in [SettleMode::Direct, SettleMode::Fast] {
        let err = transition(State::Funded, Role::Payer, mode, &Event::Dispute)
            .expect_err("dispute should need escrow");
        assert_eq!(err.code, RejectCode::StateViolation);
    }
    let t = transition(State::Funded, Role::Payer, SettleMode::Escrow, &Event::Dispute).unwrap();
    assert_eq!(t.next, State::Disputed);
}

#[test]
fn txproof_is_meaningless_outside_fast_mode() {
    for mode in [SettleMode::Direct, SettleMode::Escrow] {
        let err = transition(State::Funded, Role::Payer, mode, &Event::TxId)
            .expect_err("TXPROOF should be fast/1 only");
        assert_eq!(err.code, RejectCode::StateViolation);
    }
}

#[test]
fn cancellation_is_legal_only_between_lock_and_funding() {
    // Before a price is locked, ABORT is the free exit; CANCEL has no terms yet.
    assert!(transition(State::Quoted, Role::Payer, SettleMode::Direct, &Event::Cancel).is_err());
    // After the lock, CANCEL invokes terms the payer already signed.
    assert_eq!(
        transition(State::Accepted, Role::Payer, SettleMode::Direct, &Event::Cancel)
            .unwrap()
            .next,
        State::Cancelled
    );
    // Once funds have moved, cancelling is not a thing that exists.
    assert!(transition(State::Funded, Role::Payer, SettleMode::Direct, &Event::Cancel).is_err());
}

#[test]
fn terminal_states_absorb_everything() {
    let terminals = [
        State::Aborted,
        State::Cancelled,
        State::Disputed,
        State::Settled,
        State::Claimed,
    ];
    let events = [
        Event::Accept { from: Role::Payer },
        Event::Fund,
        Event::Receipt,
        Event::Proof,
        Event::Elapsed(Duration::from_secs(9999)),
    ];
    for s in terminals {
        assert!(s.is_terminal());
        for ev in &events {
            let err = transition(s, Role::Payer, SettleMode::Direct, ev)
                .expect_err(&format!("{:?} must absorb {:?}", s, ev));
            assert_eq!(err.code, RejectCode::StateViolation);
        }
    }
}

// -- the contact coda --------------------------------------------------------

/// Identity exchange happens after the money, and must not reopen the
/// transaction (§16.3).
#[test]
fn contact_exchange_does_not_reopen_the_transaction() {
    for ev in [Event::ContactOffer, Event::ContactAccept] {
        let t = transition(State::Closed, Role::Payer, SettleMode::Direct, &ev).unwrap();
        assert_eq!(t.next, State::Closed);
        assert_eq!(t.effect, Effect::None);
    }
}

/// Declining a contact requires doing nothing: the window simply expires and
/// the session tears down leaving no persistent trace.
#[test]
fn contact_window_expiry_leaves_no_trace() {
    let t = transition(
        State::Closed,
        Role::Payer,
        SettleMode::Direct,
        &Event::Elapsed(Duration::from_secs(120)),
    )
    .unwrap();
    assert_eq!(t.next, State::Closed);
    assert_eq!(t.effect, Effect::None);
}

/// A contact offer must not be possible before the transaction closes — the
/// deal never depends on identifying yourself (§16.3).
#[test]
fn contact_cannot_precede_closure() {
    for s in [State::Quoted, State::Accepted, State::Funded, State::Delivered] {
        assert!(
            transition(s, Role::Payer, SettleMode::Direct, &Event::ContactOffer).is_err(),
            "contact must not be offered in {:?}",
            s
        );
    }
}
