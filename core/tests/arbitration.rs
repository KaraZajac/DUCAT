//! Arbitration (§9.3), the surface §2.5 says drained the closest comparable
//! system. It had never been executed.

use ducat_core::cbor::{decode, Value};
use ducat_core::commit::{commit, Purpose};
use ducat_core::reject::RejectCode;
use ducat_core::wire::*;

const T0: u64 = 1_800_000_000;
const CLAIM: u64 = 5_000_000_000;

fn arbiter_a() -> Vec<u8> { vec![0xA1; 32] }
fn arbiter_b() -> Vec<u8> { vec![0xB2; 32] }
fn arbiter_set() -> Vec<Vec<u8>> { vec![arbiter_a(), arbiter_b()] }

fn dispute() -> Dispute {
    Dispute {
        version: 1,
        suite: 1,
        class: DisputeClass::Mechanical,
        transcript: [0x33; 32],
        claim_pxmr: CLAIM,
        timestamp: T0,
    }
}

fn ruling_for(d_bytes: &[u8], outcome: Outcome, award: u64) -> Ruling {
    Ruling {
        version: 1,
        suite: 1,
        dispute: commit(Purpose::ChainLink, d_bytes),
        outcome,
        award_pxmr: award,
        timestamp: T0 + 3600,
    }
}

#[test]
fn a_ruling_from_the_market_arbiter_set_is_accepted() {
    let d = dispute();
    let db = d.to_value().encode();
    let r = ruling_for(&db, Outcome::ForClaimant, CLAIM);
    check_ruling(&r, &d, &db, &arbiter_set(), &arbiter_a()).expect("valid ruling");
}

/// §2.5 in one check. RetoSwap was drained by a client accepting an
/// arbitrator's address from a message rather than from a signed set.
#[test]
fn a_ruling_from_outside_the_signed_set_is_refused() {
    let d = dispute();
    let db = d.to_value().encode();
    let r = ruling_for(&db, Outcome::ForClaimant, CLAIM);
    let stranger = vec![0xEE; 32];
    assert_eq!(
        check_ruling(&r, &d, &db, &arbiter_set(), &stranger).unwrap_err().code,
        RejectCode::UntrustedArbiterSet
    );
}

#[test]
fn a_ruling_must_answer_the_dispute_it_claims_to() {
    let d = dispute();
    let db = d.to_value().encode();
    let mut other = dispute();
    other.timestamp += 1;
    let r = ruling_for(&other.to_value().encode(), Outcome::ForClaimant, CLAIM);
    assert_eq!(
        check_ruling(&r, &d, &db, &arbiter_set(), &arbiter_a()).unwrap_err().code,
        RejectCode::CommitMismatch
    );
}

/// An arbiter has no authority to invent an obligation neither party asserted.
#[test]
fn an_award_cannot_exceed_the_claim() {
    let d = dispute();
    let db = d.to_value().encode();
    let r = ruling_for(&db, Outcome::ForClaimant, CLAIM + 1);
    assert_eq!(
        check_ruling(&r, &d, &db, &arbiter_set(), &arbiter_a()).unwrap_err().code,
        RejectCode::PriceMismatch
    );
}

/// An outcome that disagrees with its own award is malformed, not merely odd.
#[test]
fn only_a_ruling_for_the_claimant_may_carry_an_award() {
    let d = dispute();
    let db = d.to_value().encode();
    for outcome in [Outcome::ForRespondent, Outcome::Dismissed] {
        let r = ruling_for(&db, outcome, 1);
        assert_eq!(
            check_ruling(&r, &d, &db, &arbiter_set(), &arbiter_a()).unwrap_err().code,
            RejectCode::Malformed
        );
        // The same outcome awarding nothing is fine.
        let r = ruling_for(&db, outcome, 0);
        assert!(check_ruling(&r, &d, &db, &arbiter_set(), &arbiter_a()).is_ok());
    }
}

/// §9.3.4 said an abandoned dispute returns funds to "the pre-dispute
/// allocation". Under escrow that is a **deadlock**, not a resolution: the
/// pre-dispute allocation is funds sitting in a 2-of-3 awaiting a RELEASE that
/// two disagreeing parties will never co-sign. Doing nothing freezes them
/// permanently — the exact outcome the section claims to prevent.
///
/// Expiry must therefore produce a real ruling, which is a co-signature that
/// moves funds rather than an absence of one.
#[test]
fn an_abandoned_dispute_produces_a_ruling_not_a_deadlock() {
    let d = dispute();
    let db = d.to_value().encode();
    let r = expired_dispute_ruling(&d, &db, T0 + 14 * 86_400);

    assert_eq!(r.outcome, Outcome::ForRespondent);
    assert_eq!(r.award_pxmr, 0);
    // Crucially it is a *valid* ruling, so it can be co-signed and acted upon.
    check_ruling(&r, &d, &db, &arbiter_set(), &arbiter_a())
        .expect("an expiry ruling must be actionable, or the funds stay frozen");
}

#[test]
fn dispute_and_ruling_round_trip_and_reject_unknown_fields() {
    let d = dispute();
    let enc = d.to_value().encode();
    assert_eq!(Dispute::from_value(decode(&enc).unwrap()).unwrap(), d);
    assert_eq!(decode(&enc).unwrap().encode(), enc);

    let r = ruling_for(&enc, Outcome::Dismissed, 0);
    let renc = r.to_value().encode();
    assert_eq!(Ruling::from_value(decode(&renc).unwrap()).unwrap(), r);

    for (mut v, name) in [(d.to_value(), "dispute"), (r.to_value(), "ruling")] {
        if let Value::Map(m) = &mut v {
            m.insert(200, Value::Uint(1));
        }
        let e = if name == "dispute" {
            Dispute::from_value(v).unwrap_err()
        } else {
            Ruling::from_value(v).unwrap_err()
        };
        assert_eq!(e.code, RejectCode::UnknownField, "{}", name);
    }
}

#[test]
fn unknown_class_and_outcome_values_are_refused() {
    let mut v = dispute().to_value();
    if let Value::Map(m) = &mut v { m.insert(52, Value::Uint(9)); }
    assert_eq!(Dispute::from_value(v).unwrap_err().code, RejectCode::Malformed);

    let d = dispute();
    let mut v = ruling_for(&d.to_value().encode(), Outcome::Dismissed, 0).to_value();
    if let Value::Map(m) = &mut v { m.insert(57, Value::Uint(9)); }
    assert_eq!(Ruling::from_value(v).unwrap_err().code, RejectCode::Malformed);
}
