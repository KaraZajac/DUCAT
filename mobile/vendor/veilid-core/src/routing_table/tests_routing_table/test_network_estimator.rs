use crate::routing_table::network_estimator::NetworkEstimator;
use crate::routing_table::*;

fn one_kind_obs(bucket_zero: usize) -> BTreeMap<CryptoKind, Vec<usize>> {
    let mut m = BTreeMap::new();
    let mut counts = vec![0usize; BUCKET_COUNT];
    counts[0] = bucket_zero;
    m.insert(CRYPTO_KIND_VLD0, counts);
    m
}

fn ts_at_secs(secs: u64) -> Timestamp {
    Timestamp::new(secs * 1_000_000)
}

pub fn test_network_estimator_empty() {
    info!("--- test_network_estimator_empty ---");
    let est = NetworkEstimator::new();
    assert_eq!(est.estimate(CRYPTO_KIND_VLD0), 0);
    assert_eq!(est.estimate_combined(), 0);
}

pub fn test_network_estimator_lowest_unsaturated_bucket() {
    info!("--- test_network_estimator_lowest_unsaturated_bucket ---");
    let mut est = NetworkEstimator::new();
    est.record_observation(ts_at_secs(0), &one_kind_obs(100), &BTreeMap::new());
    assert_eq!(est.estimate(CRYPTO_KIND_VLD0), 200);
}

pub fn test_network_estimator_skips_saturated_bucket_0() {
    info!("--- test_network_estimator_skips_saturated_bucket_0 ---");
    let mut m = BTreeMap::new();
    let mut counts = vec![0usize; BUCKET_COUNT];
    counts[0] = 256;
    counts[1] = 50;
    m.insert(CRYPTO_KIND_VLD0, counts);

    let mut est = NetworkEstimator::new();
    est.record_observation(ts_at_secs(0), &m, &BTreeMap::new());
    assert_eq!(est.estimate(CRYPTO_KIND_VLD0), 200);
}

pub fn test_network_estimator_high_water_across_slots() {
    info!("--- test_network_estimator_high_water_across_slots ---");
    let mut est = NetworkEstimator::new();
    // Three observations spaced an hour apart at slots 100, 101, 102.
    est.record_observation(ts_at_secs(100 * 3600), &one_kind_obs(100), &BTreeMap::new());
    est.record_observation(ts_at_secs(101 * 3600), &one_kind_obs(50), &BTreeMap::new());
    est.record_observation(ts_at_secs(102 * 3600), &one_kind_obs(75), &BTreeMap::new());
    // High water across slots = 100, MLE = 200.
    assert_eq!(est.estimate(CRYPTO_KIND_VLD0), 200);
}

pub fn test_network_estimator_only_zeroes_entered_slot() {
    info!("--- test_network_estimator_only_zeroes_entered_slot ---");
    let mut est = NetworkEstimator::new();
    // Fill slot 100 with 200 nodes (high water 200 → estimate 400).
    est.record_observation(ts_at_secs(100 * 3600), &one_kind_obs(200), &BTreeMap::new());
    assert_eq!(est.estimate(CRYPTO_KIND_VLD0), 400);

    // Long gap: 30 hours later. Slot=130, idx=130%24=10. Slot 100 was at
    // idx=100%24=4. They're different idx, so slot 100's value is preserved.
    est.record_observation(ts_at_secs(130 * 3600), &one_kind_obs(10), &BTreeMap::new());
    // Slot 100's high water of 200 is still in the histogram at idx 4;
    // estimate remains 400 (max across all slots).
    assert_eq!(est.estimate(CRYPTO_KIND_VLD0), 400);
}

pub fn test_network_estimator_entered_slot_zeros_old_data() {
    info!("--- test_network_estimator_entered_slot_zeros_old_data ---");
    let mut est = NetworkEstimator::new();
    // Slot 100 gets 200 nodes.
    est.record_observation(ts_at_secs(100 * 3600), &one_kind_obs(200), &BTreeMap::new());
    // Exactly 24 hours later (slot 124, same idx 4 as slot 100).
    // Entering slot 124 should ZERO that idx, then record 5 nodes.
    est.record_observation(ts_at_secs(124 * 3600), &one_kind_obs(5), &BTreeMap::new());
    // Slot 100's old data at idx 4 has been wiped by slot 124 entering it.
    // The remaining slots are all 0 (no data was recorded in them).
    // High water = 5, estimate = 10.
    assert_eq!(est.estimate(CRYPTO_KIND_VLD0), 10);
}

pub fn test_network_estimator_combined_single_kind() {
    info!("--- test_network_estimator_combined_single_kind ---");
    let mut est = NetworkEstimator::new();
    est.record_observation(ts_at_secs(0), &one_kind_obs(75), &BTreeMap::new());
    assert_eq!(est.estimate_combined(), est.estimate(CRYPTO_KIND_VLD0));
}

pub fn test_network_estimator_clock_backward_ignored() {
    info!("--- test_network_estimator_clock_backward_ignored ---");
    let mut est = NetworkEstimator::new();
    est.record_observation(ts_at_secs(100 * 3600), &one_kind_obs(100), &BTreeMap::new());
    // Clock goes backward to slot 50.
    est.record_observation(ts_at_secs(50 * 3600), &one_kind_obs(200), &BTreeMap::new());
    // Backward observation ignored; estimate remains from the slot=100 record.
    assert_eq!(est.estimate(CRYPTO_KIND_VLD0), 200);
}
