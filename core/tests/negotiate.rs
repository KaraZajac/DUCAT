//! Negotiation conformance (§18.6), including §18.9(6): a successful selection
//! plus a stripped-`supported` downgrade attempt that must fail verification.

use ducat_core::cbor::Value;
use ducat_core::cbor_map;
use ducat_core::commit::{commit, Purpose};
use ducat_core::negotiate::*;
use ducat_core::reject::RejectCode;
use ducat_core::sig::Suite;
use std::collections::BTreeSet;

const ED: Suite = Suite::Ed25519X25519;
const P256: Suite = Suite::P256;

/// A payer that prefers Ed25519 but will fall back to P-256.
fn payer_policy() -> Policy {
    Policy::new(vec![ED, P256], vec![1])
}

#[test]
fn selects_the_payers_preferred_suite_not_the_highest_number() {
    // Presenter lists P-256 first; both sides support both. The payer's
    // preference must win, and "highest identifier" would pick P-256 — the
    // fallback-forced-by-hardware suite — on every dual-capable pair.
    let offered = Supported {
        versions: vec![1],
        suites: vec![P256, ED],
    };
    let sel = negotiate(&offered, &payer_policy()).unwrap();
    assert_eq!(sel.suite, ED);
    assert_eq!(sel.version, 1);
}

#[test]
fn falls_back_when_the_preferred_suite_is_not_offered() {
    // An iOS presenter holding only Secure Enclave keys offers P-256 alone.
    let offered = Supported {
        versions: vec![1],
        suites: vec![P256],
    };
    assert_eq!(negotiate(&offered, &payer_policy()).unwrap().suite, P256);
}

#[test]
fn versions_do_use_highest_mutual() {
    let offered = Supported {
        versions: vec![1, 2, 3],
        suites: vec![ED],
    };
    let policy = Policy::new(vec![ED], vec![1, 2]);
    assert_eq!(negotiate(&offered, &policy).unwrap().version, 2);
}

#[test]
fn no_common_version_is_refused() {
    let offered = Supported {
        versions: vec![7],
        suites: vec![ED],
    };
    let err = negotiate(&offered, &payer_policy()).unwrap_err();
    assert_eq!(err.code, RejectCode::UnsupportedVersion);
}

#[test]
fn no_common_suite_is_refused() {
    let offered = Supported {
        versions: vec![1],
        suites: vec![P256],
    };
    // A client that will not speak P-256 at all.
    let policy = Policy::new(vec![ED], vec![1]);
    let err = negotiate(&offered, &policy).unwrap_err();
    assert_eq!(err.code, RejectCode::UnsupportedSuite);
}

/// §18.6: a suite below the permitted set is refused even when both sides
/// "support" it. Backward compatibility is not a reason to accept a suite the
/// operator has ruled out.
#[test]
fn a_mutually_supported_but_impermissible_suite_is_still_refused() {
    let offered = Supported {
        versions: vec![1],
        suites: vec![ED, P256],
    };
    let mut policy = payer_policy();
    policy.permitted = BTreeSet::from([ED]); // P-256 ruled out locally
    assert_eq!(negotiate(&offered, &policy).unwrap().suite, ED);

    // Now the presenter offers only the excluded suite.
    let only_p256 = Supported {
        versions: vec![1],
        suites: vec![P256],
    };
    assert_eq!(
        negotiate(&only_p256, &policy).unwrap_err().code,
        RejectCode::UnsupportedSuite
    );
}

/// §10.1: a market narrows what its participants accept and can never widen it.
#[test]
fn market_policy_narrows_but_cannot_widen() {
    // Client permits only Ed25519. Market says P-256 is fine too.
    let policy = Policy::new(vec![ED], vec![1])
        .restrict_to_market(&BTreeSet::from([ED, P256]));
    // The market's permissiveness must not re-enable what the client excluded.
    assert!(!policy.permitted.contains(&P256));

    // Market restricts to P-256 only; client prefers Ed25519 but permits both.
    let narrowed = Policy::new(vec![ED, P256], vec![1])
        .restrict_to_market(&BTreeSet::from([P256]));
    let offered = Supported {
        versions: vec![1],
        suites: vec![ED, P256],
    };
    assert_eq!(negotiate(&offered, &narrowed).unwrap().suite, P256);
}

// -- downgrade resistance ----------------------------------------------------

/// Build a FullOffer-shaped object carrying an advertised suite list.
fn full_offer(suites: &[Suite]) -> Value {
    cbor_map! {
        1 => Value::Uint(1),                                   // version
        2 => Value::Uint(2_500_000_000_000),                   // amount_pxmr
        3 => Value::Array(vec![Value::Uint(1)]),               // supported versions
        4 => Value::Array(suites.iter().map(|s| Value::Uint(*s as u64)).collect()),
    }
}

/// §18.9(6) — the attack, end to end. A MITM strips the strong suite from the
/// offer so the payer sees a menu of one. The commitment carried by the tap is
/// over the *original* offer, so the substitution is detected before any suite
/// is chosen.
#[test]
fn stripping_a_suite_from_the_offer_fails_the_commitment() {
    let genuine = full_offer(&[ED, P256]).encode();
    let tap_commit = commit(Purpose::Offer, &genuine);

    // Honest path.
    assert!(verify_no_downgrade(&genuine, &tap_commit).is_ok());

    // MITM removes Ed25519, leaving only the hardware-fallback suite.
    let stripped = full_offer(&[P256]).encode();
    let err = verify_no_downgrade(&stripped, &tap_commit).unwrap_err();
    assert_eq!(err.code, RejectCode::CommitMismatch);
}

/// The commitment must cover the whole offer, not merely the suite list — a
/// price change has to be caught by exactly the same check.
#[test]
fn altering_any_field_fails_the_commitment() {
    let genuine = full_offer(&[ED, P256]).encode();
    let tap_commit = commit(Purpose::Offer, &genuine);

    let dearer = cbor_map! {
        1 => Value::Uint(1),
        2 => Value::Uint(25_000_000_000_000),  // ten times the fare
        3 => Value::Array(vec![Value::Uint(1)]),
        4 => Value::Array(vec![Value::Uint(ED as u64), Value::Uint(P256 as u64)]),
    }
    .encode();

    assert_eq!(
        verify_no_downgrade(&dearer, &tap_commit).unwrap_err().code,
        RejectCode::CommitMismatch
    );
}

/// Commitments name their purpose, so a digest computed for one role does not
/// verify as another even over identical bytes (§18.3's reasoning, applied to
/// hashes).
#[test]
fn commitments_are_domain_separated_by_purpose() {
    let bytes = full_offer(&[ED]).encode();
    let as_offer = commit(Purpose::Offer, &bytes);
    let as_receipt = commit(Purpose::Receipt, &bytes);
    let as_chain = commit(Purpose::ChainLink, &bytes);
    let as_market = commit(Purpose::MarketGenesis, &bytes);

    assert_ne!(as_offer, as_receipt);
    assert_ne!(as_offer, as_chain);
    assert_ne!(as_offer, as_market);
    assert_ne!(as_receipt, as_chain);

    // And a receipt digest must not pass an offer check.
    assert_eq!(
        verify_no_downgrade(&bytes, &as_receipt).unwrap_err().code,
        RejectCode::CommitMismatch
    );
}
