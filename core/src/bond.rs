//! Bond capacity, published coarsely (O10).
//!
//! §17.4's `bond_proof` carries `capacity_remaining`, and §17.8 flagged the
//! consequence: an exact figure, shown to every provider a rider taps, is a
//! running meter on that rider's spending. Two merchants comparing notes recover
//! what was spent between the taps; one merchant seen twice recovers it alone.
//!
//! The payee's actual question is narrower than the field answers. They need to
//! know **capacity ≥ fare** — a predicate — and are handed an integer.
//!
//! A predicate cannot simply be signed per-fare, because the attestation is
//! signed by the bond key ahead of time and the fare is not known then. Buckets
//! are the compromise: publish the floor of a coarse ladder, so the leak is
//! bounded by which rung rather than by an exact balance.

use crate::reject::{Reject, RejectCode};

/// The ladder, in piconero. A 1–2–5 progression per decade: familiar, and coarse
/// enough that a rung covers a wide range of balances.
///
/// **Ladder membership is part of the wire format.** Two clients bucketing
/// differently would disagree about whether the same bond covers the same fare.
pub const CAPACITY_BUCKETS: &[u64] = &[
    0,
    1_000_000_000,          // 0.001 XMR
    2_000_000_000,
    5_000_000_000,
    10_000_000_000,         // 0.01
    20_000_000_000,
    50_000_000_000,
    100_000_000_000,        // 0.1
    200_000_000_000,
    500_000_000_000,
    1_000_000_000_000,      // 1
    2_000_000_000_000,
    5_000_000_000_000,
    10_000_000_000_000,     // 10
    20_000_000_000_000,
    50_000_000_000_000,
    100_000_000_000_000,    // 100
];

/// The largest ladder value not exceeding `capacity_pxmr`.
///
/// **Rounds down, always.** Rounding to nearest would let a bond claim capacity
/// it does not have, which converts a privacy feature into a solvency lie — and
/// the party who benefits from the overstatement is the one publishing it.
pub fn bucket_floor(capacity_pxmr: u64) -> u64 {
    let mut floor = 0;
    for b in CAPACITY_BUCKETS {
        if *b <= capacity_pxmr {
            floor = *b;
        } else {
            break;
        }
    }
    floor
}

/// Whether a published bucket covers a fare.
///
/// One-directional on purpose: a `true` means the bond definitely covers it, and
/// a `false` means only that it cannot be *proven* at this granularity.
pub fn covers(published_bucket: u64, fare_pxmr: u64) -> bool {
    published_bucket >= fare_pxmr
}

/// Check a bucket a counterparty published.
///
/// A payee cannot verify the bucket against a balance it cannot see, but it can
/// verify the bucket is *a bucket* — an arbitrary integer in this field would
/// defeat the whole mechanism by letting a rider publish `fare - 1 + 1` and leak
/// their balance exactly, which is what the field used to do.
pub fn check_published_bucket(published: u64, fare_pxmr: u64) -> Result<(), Reject> {
    if !CAPACITY_BUCKETS.contains(&published) {
        return Err(Reject::with_detail(
            RejectCode::Malformed,
            "capacity must be published as a ladder value, not an exact balance",
        ));
    }
    if !covers(published, fare_pxmr) {
        return Err(Reject::with_detail(
            RejectCode::InsufficientCapacity,
            "published bond capacity does not cover this fare",
        ));
    }
    Ok(())
}

/// How much a bucket reveals, in bits — the honest measure of what this buys.
///
/// With 17 rungs the answer is under 4.1 bits, against 64 for an exact `u64`.
/// Worth computing rather than asserting, because the argument for bucketing is
/// quantitative and a ladder that grew carelessly would erode it silently.
pub fn leaked_bits() -> f64 {
    (CAPACITY_BUCKETS.len() as f64).log2()
}
