//! O10 — bond capacity as a side channel.

use ducat_core::bond::*;
use ducat_core::reject::RejectCode;

/// Rounding to nearest would let a bond claim capacity it does not have, and the
/// party who benefits from the overstatement is the one publishing it.
#[test]
fn a_bucket_never_overstates_capacity() {
    for cap in [0u64, 1, 999_999_999, 1_000_000_000, 1_000_000_001,
                4_999_999_999, 5_000_000_000, 123_456_789_012_345, u64::MAX] {
        assert!(bucket_floor(cap) <= cap, "bucket overstated capacity {cap}");
    }
}

#[test]
fn a_bucket_is_the_largest_rung_that_fits() {
    assert_eq!(bucket_floor(0), 0);
    assert_eq!(bucket_floor(999_999_999), 0);
    assert_eq!(bucket_floor(1_000_000_000), 1_000_000_000);
    assert_eq!(bucket_floor(1_999_999_999), 1_000_000_000);
    assert_eq!(bucket_floor(2_000_000_000), 2_000_000_000);
    assert_eq!(bucket_floor(u64::MAX), *CAPACITY_BUCKETS.last().unwrap());
}

#[test]
fn bucketing_is_monotonic() {
    let mut prev = 0;
    for cap in (0..200_000_000_000u64).step_by(1_313_131_313) {
        let b = bucket_floor(cap);
        assert!(b >= prev, "bucket went backwards as capacity rose");
        prev = b;
    }
}

/// The cost of bucketing, stated rather than hidden: a payer just under a rung
/// is refused despite having the funds. They top up to cross it.
#[test]
fn the_honest_cost_is_a_false_negative_never_a_false_positive() {
    let real_capacity = 4_999_999_999u64;
    let fare = 4_500_000_000u64;
    assert!(real_capacity >= fare, "the rider can afford this");
    assert!(
        !covers(bucket_floor(real_capacity), fare),
        "and cannot prove it at this granularity — which is the trade"
    );
    // Never the other way around.
    for cap in [1u64, 1_500_000_000, 7_000_000_000, 99_000_000_000] {
        let b = bucket_floor(cap);
        for fare in [1u64, 999_999_999, 5_000_000_000, 100_000_000_000] {
            if covers(b, fare) {
                assert!(cap >= fare, "bucket claimed to cover a fare the bond cannot pay");
            }
        }
    }
}

/// An arbitrary integer in this field defeats the whole mechanism — a rider could
/// publish their balance exactly and call it a bucket.
#[test]
fn an_exact_balance_is_not_a_bucket() {
    assert_eq!(
        check_published_bucket(4_999_999_999, 1_000).unwrap_err().code,
        RejectCode::Malformed
    );
    assert!(check_published_bucket(5_000_000_000, 1_000).is_ok());
    assert_eq!(
        check_published_bucket(1_000_000_000, 5_000_000_000).unwrap_err().code,
        RejectCode::InsufficientCapacity
    );
}

/// The argument for bucketing is quantitative, so it is measured. A ladder that
/// grew carelessly would erode the benefit silently.
#[test]
fn the_leak_stays_small() {
    assert!(leaked_bits() < 5.0, "capacity leaks {:.2} bits", leaked_bits());
    assert!(
        CAPACITY_BUCKETS.len() <= 24,
        "a finer ladder is a bigger side channel, which is the thing being fixed"
    );
    // Ladder must ascend, or bucket_floor's early break is wrong.
    for w in CAPACITY_BUCKETS.windows(2) {
        assert!(w[0] < w[1], "ladder is not strictly ascending");
    }
}
