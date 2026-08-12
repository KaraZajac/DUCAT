//! §17.2 float sizing and O9's exposure floor.

use ducat_core::float::*;

/// The point of the module: you cannot choose an exposure below what your usage
/// requires. A client offering a risk slider without showing this is offering a
/// choice the protocol does not provide.
#[test]
fn exposure_has_a_floor_set_by_usage_not_by_preference() {
    let typical = 2_000_000_000u64; // 0.002 XMR
    let p = plan(10, typical);
    assert!(p.outputs >= 10, "ten payments need at least ten outputs");
    assert_eq!(p.outputs, 15, "and more, because a payment can consume more than one");
    assert_eq!(p.total_pxmr, 15 * typical);

    // Wanting ten payments while capping exposure at five payments' worth is a
    // contradiction, and it must surface before the user is at a counter.
    let shortfall = reconcile(5 * typical, 10, typical).unwrap_err();
    assert_eq!(shortfall, 10 * typical);
}

#[test]
fn a_compatible_pair_reconciles() {
    let typical = 1_000_000_000u64;
    let p = reconcile(20 * typical, 10, typical).expect("20 covers 15");
    assert_eq!(p.outputs, 15);
}

/// §17.2 forbids promising an exact count, so the inverse rounds down.
#[test]
fn capacity_is_a_bound_not_a_promise() {
    assert_eq!(payments_supported(6), 4, "the drain test: 6 outputs bought 4 payments");
    assert_eq!(payments_supported(1), 0, "one output may not even buy one payment");
    assert_eq!(payments_supported(15), 10);
    // Never optimistic.
    for outs in 0..60u32 {
        let claimed = payments_supported(outs);
        assert!(
            (claimed as f64) * OUTPUTS_PER_PAYMENT <= outs as f64 + f64::EPSILON,
            "claimed {claimed} payments from {outs} outputs"
        );
    }
}

#[test]
fn a_float_is_never_zero_outputs() {
    assert_eq!(plan(0, 1_000).outputs, 1, "a float with no outputs cannot transact at all");
    assert_eq!(plan(1, 1_000).outputs, 2);
}

/// A user with an absurd typical payment must not wrap the total into something
/// that looks affordable.
#[test]
fn absurd_inputs_saturate() {
    let p = plan(1000, u64::MAX);
    assert_eq!(p.total_pxmr, u64::MAX);
}
