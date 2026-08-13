//! §7.5 — advisory text on a transaction.
//!
//! "£4.20" in a list of twelve is not a memory. "coffee, Tuesday" is. The whole
//! feature is that, and the whole risk is letting a display string become
//! something a decision depends on.

use ducat_core::cbor::{decode, Value};
use ducat_core::reject::RejectCode;
use ducat_core::wire::*;

fn offer(memo: Option<&str>) -> FullOffer {
    FullOffer {
        version: 1, suite: 1, profile: 2,
        payto: b"53address".to_vec(),
        amount_pxmr: 420_000_000,
        supported_versions: vec![1], supported_suites: vec![1],
        settle_mode: 0, fee_policy: FeePolicy::PayerPays,
        nonce_echo: [0x5A; 16], terms: Terms::default(),
        memo: memo.map(|m| m.to_string()),
    }
}

fn accept(memo: Option<&str>) -> Accept {
    Accept {
        version: 1, suite: 1, nonce: [0x22; 16], offer_hash: [0x11; 32],
        amount_final: 420_000_000, dest: None,
        reader_session_pk: vec![0x33; 32], timestamp: 1_800_000_000,
        chosen_version: 1, chosen_suite: 1, refund_to: None,
        memo: memo.map(|m| m.to_string()),
    }
}

#[test]
fn a_memo_round_trips_on_both_objects() {
    let o = offer(Some("flat white and a pastry"));
    let back = FullOffer::from_value(decode(&o.to_value().encode()).unwrap()).unwrap();
    assert_eq!(back.memo.as_deref(), Some("flat white and a pastry"));

    let a = accept(Some("reimbursed by work"));
    let back = Accept::from_value(decode(&a.to_value().encode()).unwrap()).unwrap();
    assert_eq!(back.memo.as_deref(), Some("reimbursed by work"));
}

/// The two memos are different claims by different parties, and neither may
/// overwrite the other: a payee writing "consulting, March" and a payer
/// recording "reimbursed by work" are both true.
#[test]
fn the_two_memos_are_independent() {
    let o = offer(Some("consulting, March"));
    let a = accept(Some("reimbursed by work"));
    let ob = FullOffer::from_value(decode(&o.to_value().encode()).unwrap()).unwrap();
    let ab = Accept::from_value(decode(&a.to_value().encode()).unwrap()).unwrap();
    assert_ne!(ob.memo, ab.memo);
}

/// There is exactly one way to say "no memo", and it is omitting the key.
///
/// This test previously asserted the opposite — that `Some("")` decoded as an
/// empty memo, distinct from absent. That gave one meaning two encodings, which
/// is the thing §18.1 refuses everywhere else, and it would have made
/// `H(FullOffer)` depend on whether a client wrote an empty string or nothing
/// into a field the user left blank. Nobody writes a blank memo on purpose.
#[test]
fn a_memo_has_exactly_one_spelling_for_nothing() {
    let none = FullOffer::from_value(decode(&offer(None).to_value().encode()).unwrap()).unwrap();
    assert_eq!(none.memo, None);

    let enc = offer(Some("")).to_value().encode();
    assert_eq!(
        FullOffer::from_value(decode(&enc).unwrap()).unwrap_err().code,
        RejectCode::Malformed,
        "a present-but-empty memo must be refused, not accepted as a second None"
    );
}

/// An unbounded text field inside a signed object is a covert channel with a
/// signature on it.
#[test]
fn an_oversized_memo_is_refused() {
    let long = "a".repeat(MAX_MEMO_CHARS + 1);
    let enc = offer(Some(&long)).to_value().encode();
    assert_eq!(
        FullOffer::from_value(decode(&enc).unwrap()).unwrap_err().code,
        RejectCode::Malformed
    );
    let at_limit = "a".repeat(MAX_MEMO_CHARS);
    assert!(FullOffer::from_value(
        decode(&offer(Some(&at_limit)).to_value().encode()).unwrap()
    ).is_ok());
}

/// The bound is in characters, not bytes: counting bytes silently shortens every
/// language that does not fit one character per byte.
#[test]
fn the_bound_counts_characters_not_bytes() {
    // Each of these is multiple bytes in UTF-8.
    let text = "咖啡".repeat(MAX_MEMO_CHARS / 2);
    assert_eq!(text.chars().count(), MAX_MEMO_CHARS);
    assert!(text.len() > MAX_MEMO_CHARS, "and more bytes than that");
    assert!(FullOffer::from_value(
        decode(&offer(Some(&text)).to_value().encode()).unwrap()
    ).is_ok(), "a memo in Chinese must not be shorter than one in English");
}

/// §18.1: text is UTF-8 and NFC-normalized. A memo is the first text on the
/// wire, so it is the first thing that can carry a second encoding of one
/// string — and two encodings of one value is a transcript-divergence bug.
#[test]
fn a_non_canonical_memo_is_refused() {
    // "é" as e + combining acute (NFD) rather than the composed form.
    let decomposed = "cafe\u{0301}";
    let mut m = match offer(None).to_value() { Value::Map(m) => m, _ => unreachable!() };
    m.insert(145u64, Value::Text(decomposed.to_string()));
    let enc = Value::Map(m).encode();
    assert!(
        decode(&enc).is_err(),
        "non-NFC text must be refused by the decoder, or one string has two encodings"
    );
}

/// A memo changes the offer, so it changes the commitment the tap makes to it.
/// A payee that could edit the memo after the payer saw it would be editing what
/// the payer agreed to (§15.5).
#[test]
fn a_memo_is_covered_by_the_offer_commitment() {
    let a = offer(Some("coffee")).commitment();
    let b = offer(Some("coffee ")).commitment();
    let none = offer(None).commitment();
    assert_ne!(a, b);
    assert_ne!(a, none);
}
