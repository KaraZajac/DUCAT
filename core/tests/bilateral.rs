//! Both parties, every message.
//!
//! The originator-versus-evaluator bug survived a 75-test suite because every
//! test drove the state machine from a single side: each happy path ran as
//! `Role::Payer`, so the payee's view of the same sequence was never checked.
//! A five-party simulation caught it in seconds.
//!
//! This file exists so that class of bug cannot recur. Every sequence is run
//! from *both* sides and the two must agree at every step — because in a real
//! transaction they are two machines that have to stay in lockstep with no
//! shared memory to fall back on.

use ducat_core::reject::RejectCode;
use ducat_core::state::*;
use std::time::Duration;

/// Drive one event sequence from both roles, asserting the states match.
fn both_agree(mode: SettleMode, steps: &[Event]) -> Vec<State> {
    let mut payer = State::Idle;
    let mut payee = State::Idle;
    let mut seen = Vec::new();

    for (i, ev) in steps.iter().enumerate() {
        let a = transition(payer, Role::Payer, mode, ev)
            .unwrap_or_else(|e| panic!("payer rejected step {} ({:?}): {:?}", i, ev, e));
        let b = transition(payee, Role::Payee, mode, ev)
            .unwrap_or_else(|e| panic!("payee rejected step {} ({:?}): {:?}", i, ev, e));
        assert_eq!(
            a.next, b.next,
            "step {} ({:?}): payer reached {:?}, payee reached {:?} — the two \
             machines have diverged, which no amount of message passing recovers from",
            i, ev, a.next, b.next
        );
        assert_eq!(a.effect, b.effect, "step {} ({:?}): effects differ", i, ev);
        payer = a.next;
        payee = b.next;
        seen.push(payer);
    }
    seen
}

#[test]
fn direct_settlement_agrees_on_both_sides() {
    let end = both_agree(
        SettleMode::Direct,
        &[
            Event::TapPresent,
            Event::FullOffer,
            Event::Accept { from: Role::Payer },
            Event::Fund,
            Event::Proof,
            Event::Receipt,
        ],
    );
    assert_eq!(end.last(), Some(&State::Closed));
}

#[test]
fn fast_settlement_agrees_on_both_sides() {
    let end = both_agree(
        SettleMode::Fast,
        &[
            Event::TapPresent,
            Event::FullOffer,
            Event::Accept { from: Role::Payer },
            Event::Fund,
            Event::TxProof,
            Event::Proof,
            Event::Receipt,
            Event::ConfirmationsReached,
        ],
    );
    assert_eq!(end.last(), Some(&State::Settled));
}

#[test]
fn escrow_dispute_agrees_on_both_sides() {
    let end = both_agree(
        SettleMode::Escrow,
        &[
            Event::TapPresent,
            Event::FullOffer,
            Event::Accept { from: Role::Payer },
            Event::Fund,
            Event::Dispute,
        ],
    );
    assert_eq!(end.last(), Some(&State::Disputed));
}

#[test]
fn cancellation_agrees_on_both_sides() {
    let end = both_agree(
        SettleMode::Direct,
        &[
            Event::TapPresent,
            Event::FullOffer,
            Event::Accept { from: Role::Payer },
            Event::Cancel,
        ],
    );
    assert_eq!(end.last(), Some(&State::Cancelled));
}

/// The contact coda must not reopen the transaction on either side (§16.3).
#[test]
fn contact_coda_agrees_on_both_sides() {
    let end = both_agree(
        SettleMode::Direct,
        &[
            Event::TapPresent,
            Event::FullOffer,
            Event::Accept { from: Role::Payer },
            Event::Fund,
            Event::Proof,
            Event::Receipt,
            Event::ContactOffer,
            Event::ContactAccept,
        ],
    );
    assert_eq!(end.last(), Some(&State::Closed));
}

/// Timeouts must fire identically for both parties. If they did not, one side
/// would abort while the other waited — and the protocol has no mechanism to
/// discover that, because a timeout produces no message.
#[test]
fn timeouts_agree_on_both_sides() {
    let cases: &[(State, SettleMode, u64)] = &[
        (State::Offered, SettleMode::Direct, 10),
        (State::Quoted, SettleMode::Direct, 30),
        (State::Accepted, SettleMode::Direct, 60),
        (State::Accepted, SettleMode::Escrow, 300),
        (State::Funded, SettleMode::Fast, 30),
        (State::Delivered, SettleMode::Direct, 120),
        (State::Closed, SettleMode::Direct, 120),
    ];
    for (state, mode, secs) in cases {
        let ev = Event::Elapsed(Duration::from_secs(*secs));
        let a = transition(*state, Role::Payer, *mode, &ev).unwrap();
        let b = transition(*state, Role::Payee, *mode, &ev).unwrap();
        assert_eq!(
            a.next, b.next,
            "{:?}/{:?} at {}s: payer → {:?}, payee → {:?}",
            state, mode, secs, a.next, b.next
        );
        assert_eq!(a.effect, b.effect, "{:?}/{:?}: effects differ", state, mode);
    }
}

/// Refusals must be symmetric too. A message one side rejects and the other
/// accepts is worse than one both reject: the transaction proceeds in one
/// machine and not the other, and nothing surfaces until money moves.
#[test]
fn refusals_agree_on_both_sides() {
    let cases: &[(State, Event)] = &[
        (State::Idle, Event::Accept { from: Role::Payer }),
        (State::Idle, Event::Fund),
        (State::Idle, Event::Receipt),
        (State::Offered, Event::Accept { from: Role::Payer }),
        (State::Quoted, Event::Fund),
        (State::Quoted, Event::Cancel),
        (State::Quoted, Event::Accept { from: Role::Payee }),
        (State::Accepted, Event::Receipt),
        (State::Funded, Event::Accept { from: Role::Payer }),
        (State::Funded, Event::Cancel),
        (State::Closed, Event::Fund),
        (State::Delivered, Event::ContactOffer),
        (State::Aborted, Event::Fund),
        (State::Settled, Event::Receipt),
    ];
    for (state, ev) in cases {
        let a = transition(*state, Role::Payer, SettleMode::Direct, ev);
        let b = transition(*state, Role::Payee, SettleMode::Direct, ev);
        match (a, b) {
            (Err(x), Err(y)) => assert_eq!(
                x.code, y.code,
                "{:?} + {:?}: payer said {:?}, payee said {:?}",
                state, ev, x.code, y.code
            ),
            (a, b) => panic!(
                "{:?} + {:?} must be refused by both: payer={:?} payee={:?}",
                state, ev, a, b
            ),
        }
    }
}

/// Mode-scoped rules must be scoped identically for both parties.
#[test]
fn mode_specific_rules_agree_on_both_sides() {
    // TXPROOF is fast/1 only.
    for mode in [SettleMode::Direct, SettleMode::Escrow] {
        for who in [Role::Payer, Role::Payee] {
            assert_eq!(
                transition(State::Funded, who, mode, &Event::TxProof)
                    .unwrap_err()
                    .code,
                RejectCode::StateViolation
            );
        }
    }
    // Dispute is escrow only.
    for mode in [SettleMode::Direct, SettleMode::Fast] {
        for who in [Role::Payer, Role::Payee] {
            assert_eq!(
                transition(State::Funded, who, mode, &Event::Dispute)
                    .unwrap_err()
                    .code,
                RejectCode::StateViolation
            );
        }
    }
}

/// Exhaustive sweep: every state against every event, from both roles. The two
/// must always reach the same verdict. This is the generalisation of the bug —
/// any future directional rule guarded on the local role fails here.
#[test]
fn every_state_event_pair_agrees_on_both_sides() {
    let states = [
        State::Idle,
        State::Offered,
        State::Quoted,
        State::Accepted,
        State::Funded,
        State::Provisional,
        State::Delivered,
        State::Metering,
        State::Closed,
        State::Settled,
        State::Claimed,
        State::Aborted,
        State::Cancelled,
        State::Disputed,
    ];
    let events = [
        Event::TapPresent,
        Event::FullOffer,
        Event::Accept { from: Role::Payer },
        Event::Accept { from: Role::Payee },
        Event::Fund,
        Event::TxProof,
        Event::Proof,
        Event::Receipt,
        Event::Cancel,
        Event::Dispute,
        Event::Abort { from: Role::Payer },
        Event::Abort { from: Role::Payee },
        Event::ContactOffer,
        Event::ContactAccept,
        Event::ConfirmationsReached,
        Event::CureWindowExpired,
        Event::MeterStart,
        Event::MeterStop,
        Event::MeterExpired,
        Event::Elapsed(Duration::from_secs(0)),
        Event::Elapsed(Duration::from_secs(600)),
    ];
    let modes = [SettleMode::Direct, SettleMode::Fast, SettleMode::Escrow];

    let mut checked = 0;
    for s in states {
        for e in &events {
            for m in modes {
                let a = transition(s, Role::Payer, m, e);
                let b = transition(s, Role::Payee, m, e);
                match (&a, &b) {
                    (Ok(x), Ok(y)) => {
                        assert_eq!(x.next, y.next, "{:?}/{:?}/{:?} state differs", s, e, m);
                        assert_eq!(x.effect, y.effect, "{:?}/{:?}/{:?} effect differs", s, e, m);
                    }
                    (Err(x), Err(y)) => {
                        assert_eq!(x.code, y.code, "{:?}/{:?}/{:?} reject differs", s, e, m)
                    }
                    _ => panic!(
                        "{:?}/{:?}/{:?}: one role accepted and the other refused — \
                         payer={:?} payee={:?}",
                        s, e, m, a, b
                    ),
                }
                checked += 1;
            }
        }
    }
    assert!(checked >= 600, "sweep covered only {} combinations", checked);
}

// -- metered sessions (§15.7) -----------------------------------------------

/// A meter runs for as long as the service lasts. Before `Metering` existed the
/// start leg landed in `Accepted`, whose 60-second deadline aborted a bar tab
/// after one minute — §15.7's two-tap flow and §6.2's deadlines had been
/// written independently and never checked against each other.
#[test]
fn a_metered_session_survives_longer_than_a_minute() {
    let mut s = State::Idle;
    for ev in [
        Event::TapPresent,
        Event::FullOffer,
        Event::MeterStart,
    ] {
        s = transition(s, Role::Payer, SettleMode::Direct, &ev)
            .unwrap_or_else(|e| panic!("{:?} rejected: {:?}", ev, e))
            .next;
    }
    assert_eq!(s, State::Metering);

    // Hours pass. A meter is not wall-clock bounded by §6.2, because its limit
    // lives in terms and is signalled explicitly.
    for secs in [60u64, 3600, 28_800] {
        let t = transition(s, Role::Payer, SettleMode::Direct,
                           &Event::Elapsed(Duration::from_secs(secs))).unwrap();
        assert_eq!(t.next, State::Metering, "meter died after {}s", secs);
    }
}

#[test]
fn a_metered_session_settles_on_stop() {
    let mut s = State::Idle;
    for ev in [
        Event::TapPresent,
        Event::FullOffer,
        Event::MeterStart,
        Event::MeterStop,
        Event::Fund,
        Event::Proof,
        Event::Receipt,
    ] {
        s = transition(s, Role::Payer, SettleMode::Direct, &ev)
            .unwrap_or_else(|e| panic!("{:?} rejected: {:?}", ev, e))
            .next;
    }
    assert_eq!(s, State::Closed);
}

/// §15.7: the customer walks out. The payee computes what accrued, capped by
/// what the payer agreed to, and emits a unilateral record — which proves what
/// was authorised and metered, not that the payer agreed to the total.
#[test]
fn an_abandoned_meter_yields_a_single_sided_receipt() {
    let t = transition(State::Metering, Role::Payee, SettleMode::Direct, &Event::MeterExpired)
        .unwrap();
    assert_eq!(t.next, State::Closed);
    assert_eq!(t.effect, Effect::EmitSingleSidedReceipt);
}

#[test]
fn metered_transitions_agree_on_both_sides() {
    both_agree(
        SettleMode::Direct,
        &[
            Event::TapPresent,
            Event::FullOffer,
            Event::MeterStart,
            Event::Elapsed(Duration::from_secs(7200)),
            Event::MeterStop,
            Event::Fund,
            Event::Proof,
            Event::Receipt,
        ],
    );
}

/// A `stop` for a meter that never started must be refused — §15.7's
/// `session_ref` binding says you can only be billed for a meter you began.
#[test]
fn a_stop_without_a_start_is_refused() {
    for s in [State::Idle, State::Offered, State::Quoted, State::Accepted, State::Funded] {
        for who in [Role::Payer, Role::Payee] {
            assert!(
                transition(s, who, SettleMode::Direct, &Event::MeterStop).is_err(),
                "MeterStop must be refused in {:?}",
                s
            );
        }
    }
}

/// §6 says ABORT is available to either party with no penalty. That is right
/// before value accrues and wrong once a meter is running: a payer who could
/// abort a live meter would start a tab, consume, abort, and owe nothing.
///
/// So from `METERING` only the operator may void cleanly. A payer leaving is
/// abandonment, which goes through `MeterExpired` and leaves a single-sided
/// receipt as evidence rather than a clean exit with no record.
#[test]
fn a_payer_cannot_abort_a_running_meter() {
    // The operator may void the tab — comping a drink is ordinary commerce.
    let t = transition(State::Metering, Role::Payee, SettleMode::Direct,
                       &Event::Abort { from: Role::Payee }).unwrap();
    assert_eq!(t.next, State::Aborted);

    // The payer may not, and both parties agree about that.
    for who in [Role::Payer, Role::Payee] {
        let err = transition(State::Metering, who, SettleMode::Direct,
                             &Event::Abort { from: Role::Payer })
            .expect_err("a payer must not be able to walk away for free");
        assert_eq!(err.code, RejectCode::StateViolation);
    }

    // Abandonment remains available and is *not* free — it produces evidence.
    let t = transition(State::Metering, Role::Payee, SettleMode::Direct,
                       &Event::MeterExpired).unwrap();
    assert_eq!(t.effect, Effect::EmitSingleSidedReceipt);
}

/// Before anything accrues, either party may walk away. The asymmetry above is
/// specific to a meter that is running, not a general rule about aborts.
#[test]
fn either_party_may_abort_before_value_accrues() {
    for state in [State::Quoted, State::Accepted] {
        for originator in [Role::Payer, Role::Payee] {
            for evaluator in [Role::Payer, Role::Payee] {
                let t = transition(state, evaluator, SettleMode::Direct,
                                   &Event::Abort { from: originator })
                    .unwrap_or_else(|e| panic!("{:?} abort by {:?}: {:?}", state, originator, e));
                assert_eq!(t.next, State::Aborted);
            }
        }
    }
}

/// A running meter cannot be CANCELled either. §7.3's cancellation fee is a
/// fixed schedule agreed in advance; a meter already has the right mechanism
/// for partial consumption, which is stopping it and paying what accrued.
#[test]
fn cancel_does_not_apply_to_a_running_meter() {
    for who in [Role::Payer, Role::Payee] {
        assert_eq!(
            transition(State::Metering, who, SettleMode::Direct, &Event::Cancel)
                .unwrap_err()
                .code,
            RejectCode::StateViolation
        );
    }
}
