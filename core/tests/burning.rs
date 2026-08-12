//! O15 — the burning bug, which is an arithmetic attack on the recipient.

use ducat_core::burning::*;
use ducat_core::reject::RejectCode;

fn out(key: u8, amount: u64) -> ReceivedOutput {
    ReceivedOutput { one_time_key: [key; 32], amount_pxmr: amount, txid: [0x77; 32] }
}

/// The attack, exactly. A merchant expecting 1.0 receives two outputs of 0.5 to
/// the same one-time key. `sum()` says 1.0 and the goods go out the door; one
/// coin is spendable and the merchant is out half the price.
#[test]
fn two_halves_to_one_key_are_worth_one_half() {
    let outs = vec![out(0xAA, 500_000_000_000), out(0xAA, 500_000_000_000)];
    let c = creditable(&outs);
    assert_eq!(c.naive_sum_pxmr, 1_000_000_000_000, "what sum() would have said");
    assert_eq!(c.total_pxmr, 500_000_000_000, "what is actually spendable");
    assert!(c.burn_detected());
    assert_eq!(
        check_payment(&outs, 1_000_000_000_000).unwrap_err().code,
        RejectCode::PriceMismatch
    );
}

/// Distinct keys are summed normally — the mitigation must not cost anything on
/// the honest path, or nobody will keep it.
#[test]
fn distinct_keys_are_summed() {
    let outs = vec![out(0x01, 600_000_000), out(0x02, 400_000_000)];
    let c = creditable(&outs);
    assert_eq!(c.total_pxmr, 1_000_000_000);
    assert_eq!(c.total_pxmr, c.naive_sum_pxmr);
    assert!(!c.burn_detected());
    assert!(check_payment(&outs, 1_000_000_000).is_ok());
}

/// Monero's own wallet keeps the largest of a duplicate set. Matching that is
/// the only choice that is both safe and not self-punishing.
#[test]
fn duplicates_count_once_at_the_maximum() {
    let outs = vec![out(0xBB, 100), out(0xBB, 900), out(0xBB, 50)];
    assert_eq!(creditable(&outs).total_pxmr, 900);
}

/// A burn that still covers the price is not a non-event. A duplicate one-time
/// key does not happen by accident.
#[test]
fn a_burn_is_surfaced_even_when_the_payment_still_covers_the_price() {
    let outs = vec![out(0xCC, 1_000), out(0xCC, 1), out(0x02, 5)];
    let c = check_payment(&outs, 1_000).expect("1000 + 5 is spendable, price is met");
    assert!(c.burn_detected(), "the customer constructed this; the merchant should know");
    assert_eq!(c.total_pxmr, 1_005);
    assert_eq!(c.naive_sum_pxmr, 1_006);
}

/// A merchant told "underpaid" argues about the price. One told the payment
/// carried duplicate keys knows it was built that way.
#[test]
fn a_burn_shortfall_is_reported_as_a_burn_not_a_shortfall() {
    let burned = vec![out(0xAA, 500), out(0xAA, 500)];
    let msg = format!("{:?}", check_payment(&burned, 1_000).unwrap_err());
    assert!(msg.contains("burning-bug"), "got: {msg}");

    let honest = vec![out(0x01, 900)];
    let msg = format!("{:?}", check_payment(&honest, 1_000).unwrap_err());
    assert!(!msg.contains("burning-bug"), "an ordinary shortfall must not be blamed on a burn");
}

#[test]
fn an_empty_payment_is_worth_nothing_and_does_not_panic() {
    let c = creditable(&[]);
    assert_eq!(c.total_pxmr, 0);
    assert!(!c.burn_detected());
    assert_eq!(check_payment(&[], 1).unwrap_err().code, RejectCode::PriceMismatch);
    assert!(check_payment(&[], 0).is_ok());
}

/// Saturating arithmetic: a hostile set of outputs must not wrap the total into
/// something small and acceptable.
#[test]
fn absurd_amounts_saturate_rather_than_wrap() {
    let outs = vec![out(0x01, u64::MAX), out(0x02, u64::MAX)];
    let c = creditable(&outs);
    assert_eq!(c.total_pxmr, u64::MAX);
    assert_eq!(c.naive_sum_pxmr, u64::MAX);
}
