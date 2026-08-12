//! End-to-end transcripts (§18.9(4)) over the wire objects in `wire`.
//!
//! A completed transaction is a chain of commitments held only by the two
//! parties (§6). These tests build one, verify it, and then break each link in
//! turn — because a transcript that cannot detect tampering is not evidence.

use ducat_core::cbor::{decode, Value};
use ducat_core::commit::{commit, Purpose};
use ducat_core::reject::RejectCode;
use ducat_core::sig::{ObjectType, SecretKey, SignedBytes};
use ducat_core::wire::*;

const FARE: u64 = 2_500_000_000_000; // 2.5 XMR in piconero
const NONCE: [u8; 16] = [0xA5; 16];

fn payee_key() -> SecretKey {
    SecretKey::ed25519_from_bytes(&[1u8; 32])
}
fn payer_key() -> SecretKey {
    SecretKey::ed25519_from_bytes(&[2u8; 32])
}

fn offer() -> FullOffer {
    FullOffer {
        version: 1,
        suite: 1,
        profile: 2, // pos/1
        payto: vec![0x42; 69],
        amount_pxmr: FARE,
        supported_versions: vec![1],
        supported_suites: vec![1, 2],
        settle_mode: 0, // direct
        fee_policy: FeePolicy::PayerPays,
        nonce_echo: NONCE,
        terms: Terms {
            refund_window_s: 86_400 * 14,
            ..Terms::default()
        },
    }
}

fn tap(o: &FullOffer) -> TapPresent {
    TapPresent {
        version: 1,
        suite: 1,
        profile: 2,
        presenter_role: PresenterRole::Payee,
        amount_authority: AmountAuthority::Fixed,
        intent: Intent::Oneshot,
        rmode: ReachMode::Token,
        nonce: NONCE,
        expiry: 1_800_000_030,
        session_pk: payee_key().public().to_bytes(),
        route: vec![0x11; 32],
        offer_commit: o.commitment(),
        dest: None,
        session_ref: None,
    }
}

fn accept(o: &FullOffer) -> Accept {
    Accept {
        version: 1,
        suite: 1,
        nonce: NONCE,
        offer_hash: o.commitment(),
        amount_final: FARE,
        dest: None,
        reader_session_pk: payer_key().public().to_bytes(),
        timestamp: 1_800_000_005,
        chosen_version: 1,
        chosen_suite: 1,
    }
}

fn receipt_for(accept_bytes: &[u8], amount: u64, unilateral: bool) -> Receipt {
    let h = commit(Purpose::ChainLink, accept_bytes);
    Receipt {
        version: 1,
        suite: 1,
        accept_hash: h,
        prev: h,
        amount_final: amount,
        timestamp: 1_800_000_010,
        unilateral,
    }
}

/// The whole chain, built and checked the way two clients would.
#[test]
fn full_pos_transcript_verifies() {
    let o = offer();
    let t = tap(&o);
    let a = accept(&o);
    let a_bytes = a.to_value().encode();
    let r = receipt_for(&a_bytes, FARE, false);

    verify_transcript(&t, &o, &a, &a_bytes, &r).expect("honest transcript must verify");
}

/// Every object must survive a round trip through the wire exactly, or the
/// hashes computed by the two parties differ.
#[test]
fn every_object_round_trips_byte_identically() {
    let o = offer();
    let t = tap(&o);
    let a = accept(&o);
    let a_bytes = a.to_value().encode();
    let r = receipt_for(&a_bytes, FARE, false);

    for (name, enc) in [
        ("TapPresent", t.to_value().encode()),
        ("FullOffer", o.to_value().encode()),
        ("Accept", a_bytes.clone()),
        ("Receipt", r.to_value().encode()),
    ] {
        let back = decode(&enc).unwrap_or_else(|e| panic!("{}: {:?}", name, e));
        assert_eq!(back.encode(), enc, "{} is not canonical", name);
    }

    assert_eq!(TapPresent::from_value(decode(&t.to_value().encode()).unwrap()).unwrap(), t);
    assert_eq!(FullOffer::from_value(decode(&o.to_value().encode()).unwrap()).unwrap(), o);
    assert_eq!(Accept::from_value(decode(&a_bytes).unwrap()).unwrap(), a);
    assert_eq!(Receipt::from_value(decode(&r.to_value().encode()).unwrap()).unwrap(), r);
}

/// The envelope carries the body opaquely, so a signature can be checked
/// without parsing — and the object type comes from inside the signed body.
#[test]
fn envelope_seals_and_opens() {
    let o = offer();
    let body = SignedBytes::from_value(o.to_value());
    let env = seal(&body, ObjectType::FullOffer, &payee_key());

    let (kind, opened) = open(&env, &payee_key().public()).expect("must open");
    assert_eq!(kind, ObjectType::FullOffer);
    assert_eq!(opened.bytes(), body.bytes());
    assert_eq!(FullOffer::from_value(opened.value().clone()).unwrap(), o);
}

#[test]
fn envelope_from_the_wrong_signer_is_refused() {
    let body = SignedBytes::from_value(offer().to_value());
    let env = seal(&body, ObjectType::FullOffer, &payee_key());
    assert_eq!(
        open(&env, &payer_key().public()).unwrap_err().code,
        RejectCode::BadSig
    );
}

// -- tampering ---------------------------------------------------------------

/// The attack §15.3's commitment exists to stop: swap the offer after the tap.
#[test]
fn swapping_the_offer_after_the_tap_is_caught() {
    let honest = offer();
    let t = tap(&honest);

    let mut dearer = offer();
    dearer.amount_pxmr = FARE * 10;

    let a = accept(&dearer);
    let a_bytes = a.to_value().encode();
    let r = receipt_for(&a_bytes, dearer.amount_pxmr, false);

    assert_eq!(
        verify_transcript(&t, &dearer, &a, &a_bytes, &r).unwrap_err().code,
        RejectCode::CommitMismatch
    );
}

/// A payee that reports a larger figure than the payer signed is rewriting
/// history; the ACCEPT is the authoritative record (§15.5).
#[test]
fn receipt_cannot_inflate_the_amount() {
    let o = offer();
    let t = tap(&o);
    let a = accept(&o);
    let a_bytes = a.to_value().encode();
    let r = receipt_for(&a_bytes, FARE * 2, false);

    assert_eq!(
        verify_transcript(&t, &o, &a, &a_bytes, &r).unwrap_err().code,
        RejectCode::PriceMismatch
    );
}

/// A receipt from some other transaction must not attach to this one.
#[test]
fn receipt_must_chain_to_its_own_accept() {
    let o = offer();
    let t = tap(&o);
    let a = accept(&o);
    let a_bytes = a.to_value().encode();

    let mut other = accept(&o);
    other.timestamp += 1;
    let r = receipt_for(&other.to_value().encode(), FARE, false);

    assert_eq!(
        verify_transcript(&t, &o, &a, &a_bytes, &r).unwrap_err().code,
        RejectCode::CommitMismatch
    );
}

/// Replaying an old offer under a fresh tap must fail even if the amounts agree.
#[test]
fn nonce_must_match_the_bootstrap() {
    let o = offer();
    let t = tap(&o);
    let mut a = accept(&o);
    a.nonce = [0xFF; 16];
    let a_bytes = a.to_value().encode();
    let r = receipt_for(&a_bytes, FARE, false);

    assert_eq!(
        verify_transcript(&t, &o, &a, &a_bytes, &r).unwrap_err().code,
        RejectCode::Replay
    );
}

/// §6.2's single-sided receipt is still a valid transcript — it simply records
/// that the counterparty never co-signed.
#[test]
fn single_sided_receipt_is_a_valid_transcript() {
    let o = offer();
    let t = tap(&o);
    let a = accept(&o);
    let a_bytes = a.to_value().encode();
    let r = receipt_for(&a_bytes, FARE, true);

    verify_transcript(&t, &o, &a, &a_bytes, &r).expect("unilateral receipt must still verify");
    assert!(r.unilateral, "and must say so");
}

// -- strictness --------------------------------------------------------------

/// §18.8: a field the implementation does not recognise is a rejection, not
/// something to ignore. Tolerating it means signing what you did not display.
#[test]
fn unknown_fields_are_rejected() {
    let mut v = offer().to_value();
    if let Value::Map(m) = &mut v {
        m.insert(99, Value::Uint(1));
    }
    assert_eq!(
        FullOffer::from_value(v).unwrap_err().code,
        RejectCode::UnknownField
    );
}

#[test]
fn missing_and_mistyped_fields_are_rejected() {
    // Missing a required field.
    let mut v = offer().to_value();
    if let Value::Map(m) = &mut v {
        m.remove(&f::AMOUNT_PXMR);
    }
    assert_eq!(FullOffer::from_value(v).unwrap_err().code, RejectCode::Malformed);

    // Right field, wrong type — an amount as a byte string.
    let mut v = offer().to_value();
    if let Value::Map(m) = &mut v {
        m.insert(f::AMOUNT_PXMR, Value::Bytes(vec![1, 2, 3]));
    }
    assert_eq!(FullOffer::from_value(v).unwrap_err().code, RejectCode::Malformed);
}

#[test]
fn an_object_cannot_be_parsed_as_another_type() {
    // A FullOffer body handed to the ACCEPT parser must be refused on its type
    // field alone, before any field-shape confusion can arise.
    let v = offer().to_value();
    assert_eq!(Accept::from_value(v).unwrap_err().code, RejectCode::Malformed);
}

/// §15.7: `session_ref` ties a `stop` to the meter it started. Present without
/// a stop, or absent with one, means a confused or hostile presenter.
#[test]
fn session_ref_must_accompany_stop_exactly() {
    let o = offer();

    let mut t = tap(&o);
    t.intent = Intent::Stop; // stop without a reference
    assert_eq!(
        TapPresent::from_value(t.to_value()).unwrap_err().code,
        RejectCode::Malformed
    );

    let mut t = tap(&o);
    t.session_ref = Some([7u8; 32]); // reference without a stop
    assert_eq!(
        TapPresent::from_value(t.to_value()).unwrap_err().code,
        RejectCode::Malformed
    );

    // Both together is fine.
    let mut t = tap(&o);
    t.intent = Intent::Stop;
    t.session_ref = Some([7u8; 32]);
    assert!(TapPresent::from_value(t.to_value()).is_ok());
}

// -- size, against the §15.3 budget -----------------------------------------

/// Phase 0a measured route blobs; this measures the rest of the object. The
/// spec's 158-byte fixed estimate predates any implementation, so the real
/// figure belongs in §15.3.1.
#[test]
fn report_measured_object_sizes() {
    let o = offer();
    let t_token = tap(&o);

    let mut t_inline = tap(&o);
    t_inline.rmode = ReachMode::Inline;
    t_inline.route = vec![0x11; 728]; // smallest 1-hop blob measured in Phase 0a

    let body = SignedBytes::from_value(t_token.to_value());
    let sealed_token = seal(&body, ObjectType::TapPresent, &payee_key()).len();

    let body_i = SignedBytes::from_value(t_inline.to_value());
    let sealed_inline = seal(&body_i, ObjectType::TapPresent, &payee_key()).len();

    println!("TapPresent token mode : body {} B, sealed {} B", body.bytes().len(), sealed_token);
    println!("TapPresent inline 1hop: body {} B, sealed {} B", body_i.bytes().len(), sealed_inline);
    println!("FullOffer             : {} B", o.to_value().encode().len());
    println!("Accept                : {} B", accept(&o).to_value().encode().len());

    // Token mode must clear an NTAG215 (504 B) with room to spare, since that
    // is the chip §15.3.2 says tags should use.
    assert!(sealed_token < 504, "token mode no longer fits an NTAG215: {} B", sealed_token);
}

// -- terms, and the meter rules that had nowhere to live --------------------

/// §15.7 requires a metered offer to declare a cap and a maximum duration.
/// Until `Terms` existed there was no field to put them in, so the requirement
/// could not be obeyed by any conforming client — a rule about a field that
/// does not exist.
#[test]
fn a_rated_offer_must_declare_a_cap_and_a_limit() {
    let mut o = offer();
    let mut t = tap(&o);
    t.amount_authority = AmountAuthority::Rated;

    // No cap: refused.
    assert_eq!(
        check_meter_terms(&t, &o).unwrap_err().code,
        RejectCode::Malformed
    );

    // Cap but no duration limit: still refused — an unbounded meter with a
    // ceiling is still unbounded in time, and the payer confirmed neither.
    o.terms.meter_cap_pxmr = FARE;
    assert_eq!(
        check_meter_terms(&t, &o).unwrap_err().code,
        RejectCode::Malformed
    );

    o.terms.meter_max_s = 3600;
    assert!(check_meter_terms(&t, &o).is_ok());

    // A fixed-price offer needs neither.
    let fixed = tap(&offer());
    assert!(check_meter_terms(&fixed, &offer()).is_ok());
}

/// §15.7: an abandoned meter accrues to the cap and no further. The customer
/// who walks out owes what the meter says, bounded by what they agreed to —
/// and whether any of it is collectable is a separate question about collateral.
#[test]
fn an_abandoned_meter_is_bounded_by_what_was_agreed() {
    let mut o = offer();
    o.terms.meter_cap_pxmr = 1_000_000_000_000; // 1 XMR
    o.terms.meter_max_s = 3600;
    let rate = 100_000_000; // 0.0001 XMR/s — low enough that time binds first

    // Ordinary case: rate x time, under both limits.
    assert_eq!(abandoned_meter_claim(&o, rate, 100), rate * 100);

    // Past the duration limit: time is clamped, and the result stays under the
    // cap so this isolates the time clamp.
    assert_eq!(abandoned_meter_claim(&o, rate, 10_000), rate * 3600);
    assert!(rate * 3600 < o.terms.meter_cap_pxmr);

    // Both clamps at once: a high rate over a long abandonment. Time clamps to
    // 3600 and the resulting 3.6 XMR then clamps to the 1 XMR cap — the cap
    // binds second, and the payer owes only what they agreed to.
    assert_eq!(
        abandoned_meter_claim(&o, 1_000_000_000, 10_000),
        o.terms.meter_cap_pxmr
    );

    // A rate high enough to blow the cap is clamped by the cap.
    assert_eq!(
        abandoned_meter_claim(&o, 500_000_000_000, 3600),
        o.terms.meter_cap_pxmr
    );

    // Absurd inputs must not overflow into a small number, which would let a
    // hostile rate wrap around into a trivial claim.
    assert_eq!(
        abandoned_meter_claim(&o, u64::MAX, u64::MAX),
        o.terms.meter_cap_pxmr
    );
}

/// Terms are part of the signed offer, so altering them after the fact breaks
/// the commitment exactly as altering a price does.
#[test]
fn altering_terms_breaks_the_commitment() {
    let honest = offer();
    let t = tap(&honest);

    let mut sneaky = offer();
    // Silently shorten the refund window the payer thought they were getting.
    sneaky.terms.refund_window_s = 60;

    let a = accept(&sneaky);
    let a_bytes = a.to_value().encode();
    let r = receipt_for(&a_bytes, FARE, false);

    assert_eq!(
        verify_transcript(&t, &sneaky, &a, &a_bytes, &r).unwrap_err().code,
        RejectCode::CommitMismatch
    );
}

#[test]
fn terms_round_trip_and_reject_unknown_fields() {
    let mut o = offer();
    o.terms = Terms {
        cancellation_pxmr: 5_000_000_000,
        refund_window_s: 86_400 * 30,
        meter_cap_pxmr: 2 * FARE,
        meter_max_s: 7200,
        min_fee_tier: 2,
    };
    let enc = o.to_value().encode();
    let back = FullOffer::from_value(decode(&enc).unwrap()).unwrap();
    assert_eq!(back, o);
    assert_eq!(back.to_value().encode(), enc);

    // An unrecognised field inside terms is rejected like any other (§18.8).
    let mut v = o.to_value();
    if let Value::Map(m) = &mut v {
        if let Some(Value::Map(t)) = m.get_mut(&f::TERMS) {
            t.insert(99, Value::Uint(1));
        }
    }
    assert_eq!(
        FullOffer::from_value(v).unwrap_err().code,
        RejectCode::UnknownField
    );
}
