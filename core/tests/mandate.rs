//! Standing mandates (§7.3).
//!
//! A mandate is the one place the protocol authorises payment without a
//! per-payment human checkpoint, which §15.5 otherwise makes mandatory. The
//! checkpoint moves rather than disappearing — the human confirms the cap and
//! the period once — so these tests are mostly about the cap actually binding.

use ducat_core::cbor::{decode, Value};
use ducat_core::reject::RejectCode;
use ducat_core::wire::*;

const DAY: u64 = 86_400;
const T0: u64 = 1_800_000_000;

fn payee() -> Vec<u8> {
    vec![0xAB; 32]
}

fn mandate() -> Mandate {
    Mandate {
        version: 1,
        suite: 1,
        payee_persona: payee(),
        cap_pxmr: 10_000_000_000, // 0.01 XMR per period
        period_s: DAY * 30,
        expiry: T0 + DAY * 365,
        nonce: [0x77; 16],
    }
}

#[test]
fn a_draw_within_the_cap_is_authorised() {
    let m = mandate();
    let u = MandateUsage::default();
    let next = check_mandate_draw(&m, &u, &payee(), 4_000_000_000, T0).unwrap();
    assert_eq!(next.drawn_pxmr, 4_000_000_000);
    assert_eq!(next.period_start, T0);
}

/// The cap is the whole security model. Without it a mandate is a blank cheque
/// signed once.
#[test]
fn draws_accumulate_and_the_cap_binds() {
    let m = mandate();
    let mut u = MandateUsage::default();

    u = check_mandate_draw(&m, &u, &payee(), 6_000_000_000, T0).unwrap();
    u = check_mandate_draw(&m, &u, &payee(), 4_000_000_000, T0 + 100).unwrap();
    assert_eq!(u.drawn_pxmr, m.cap_pxmr, "exactly at the cap is allowed");

    let err = check_mandate_draw(&m, &u, &payee(), 1, T0 + 200).unwrap_err();
    assert_eq!(err.code, RejectCode::PolicyRefused);
}

#[test]
fn the_cap_resets_when_the_period_rolls_over() {
    let m = mandate();
    let mut u = MandateUsage::default();
    u = check_mandate_draw(&m, &u, &payee(), m.cap_pxmr, T0).unwrap();
    assert!(check_mandate_draw(&m, &u, &payee(), 1, T0 + 100).is_err());

    // A period later, the allowance is fresh — and is anchored to the first
    // draw rather than to a calendar, so there is no timezone in the protocol.
    let after = check_mandate_draw(&m, &u, &payee(), m.cap_pxmr, T0 + m.period_s).unwrap();
    assert_eq!(after.drawn_pxmr, m.cap_pxmr);
    assert_eq!(after.period_start, T0 + m.period_s);
}

/// Without this a mandate is bearer paper: anyone holding it could draw.
#[test]
fn only_the_named_persona_may_draw() {
    let m = mandate();
    let u = MandateUsage::default();
    let err = check_mandate_draw(&m, &u, &[0xFF; 32], 1_000_000_000, T0).unwrap_err();
    assert_eq!(err.code, RejectCode::PolicyRefused);
}

#[test]
fn an_expired_mandate_authorises_nothing() {
    let m = mandate();
    let u = MandateUsage::default();
    let err = check_mandate_draw(&m, &u, &payee(), 1, m.expiry).unwrap_err();
    assert_eq!(err.code, RejectCode::Expired);
    assert!(check_mandate_draw(&m, &u, &payee(), 1, m.expiry - 1).is_ok());
}

/// A capless or periodless mandate is a blank cheque. Refusing at parse time
/// means one cannot exist in a client's store at all, rather than being caught
/// later by whichever code path happens to look.
#[test]
fn a_capless_or_periodless_mandate_cannot_be_parsed() {
    for (cap, period) in [(0u64, DAY), (10_000_000_000, 0), (0, 0)] {
        let mut m = mandate();
        m.cap_pxmr = cap;
        m.period_s = period;
        let enc = m.to_value().encode();
        assert_eq!(
            Mandate::from_value(decode(&enc).unwrap()).unwrap_err().code,
            RejectCode::Malformed,
            "cap {} period {} must be refused",
            cap,
            period
        );
    }
}

/// Overflow must not wrap a huge draw into a small one that fits under the cap.
#[test]
fn an_absurd_draw_cannot_wrap_under_the_cap() {
    let m = mandate();
    let u = MandateUsage {
        period_start: T0,
        drawn_pxmr: 1,
    };
    let err = check_mandate_draw(&m, &u, &payee(), u64::MAX, T0 + 1).unwrap_err();
    assert_eq!(err.code, RejectCode::PolicyRefused);
}

#[test]
fn mandate_round_trips_and_rejects_unknown_fields() {
    let m = mandate();
    let enc = m.to_value().encode();
    assert_eq!(Mandate::from_value(decode(&enc).unwrap()).unwrap(), m);
    assert_eq!(decode(&enc).unwrap().encode(), enc);

    let mut v = m.to_value();
    if let Value::Map(x) = &mut v {
        x.insert(123, Value::Uint(1));
    }
    assert_eq!(
        Mandate::from_value(v).unwrap_err().code,
        RejectCode::UnknownField
    );
}
