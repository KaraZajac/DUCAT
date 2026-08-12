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
        Event::Abort,
        Event::ContactOffer,
        Event::ContactAccept,
        Event::ConfirmationsReached,
        Event::CureWindowExpired,
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
