use super::*;
use crate::attachment_manager::attachment_level_calculator::*;

fn inputs(
    reliable_count: usize,
    live_count: usize,
    responsive_count: usize,
    est_net: u64,
    p75_ms: Option<u64>,
    excess_kickable: usize,
) -> AttachmentLevelInputs {
    AttachmentLevelInputs {
        reliable_count,
        live_count,
        responsive_count,
        estimated_network_size: est_net,
        median_latency: p75_ms.map(TimestampDuration::new_ms),
        excess_kickable,
    }
}

pub fn test_attaching_when_no_live_peers() {
    info!("--- test_attaching_when_no_live_peers ---");
    assert_eq!(
        compute_attachment_state(&inputs(0, 0, 0, 0, None, 0)),
        AttachmentState::Attaching
    );
    assert_eq!(
        compute_attachment_state(&inputs(0, 0, 0, 100, Some(20), 0)),
        AttachmentState::Attaching
    );
}

pub fn test_weak_floor_when_live_but_untested() {
    info!("--- test_weak_floor_when_live_but_untested ---");
    // Live nodes but none contacted yet (tested=0): floor at AttachedWeak.
    let state = compute_attachment_state(&inputs(0, 8, 0, 10, Some(20), 0));
    assert_eq!(state, AttachmentState::AttachedWeak);
    // Even with a latency penalty, the floor holds while live.
    let state = compute_attachment_state(&inputs(0, 8, 1, 60, Some(600), 0));
    assert_eq!(state, AttachmentState::AttachedWeak);
}

pub fn test_well_attached_server() {
    info!("--- test_well_attached_server ---");
    // From real veilid-server-vt log: tested=153, live=164, p75≈20ms.
    // Saturation target = min(32, 300/2) = 32. raw_bars = 153*5/32 = 23 → clamped to 5.
    // No latency penalty. Expected: AttachedFull.
    let state = compute_attachment_state(&inputs(153, 164, 153, 300, Some(20), 0));
    assert_eq!(state, AttachmentState::AttachedFull);
}

pub fn test_minimal_test_node() {
    info!("--- test_minimal_test_node ---");
    // Our integration tests: tested=5, live=7, est_net=10, p75=50ms.
    // target = min(32, 10/2) = 5. raw_bars = 5*5/5 = 5. No latency penalty.
    // Expected: AttachedFull (small but saturated for its network).
    let state = compute_attachment_state(&inputs(5, 7, 5, 10, Some(50), 0));
    assert_eq!(state, AttachmentState::AttachedFull);
}

pub fn test_partial_attached_no_latency_penalty() {
    info!("--- test_partial_attached_no_latency_penalty ---");
    // tested=16, est_net=128 (target=32), raw_bars=16*5/32=2. AttachedFair.
    let state = compute_attachment_state(&inputs(16, 20, 16, 128, Some(50), 0));
    assert_eq!(state, AttachmentState::AttachedFair);
}

pub fn test_latency_penalty_drops_one_bar() {
    info!("--- test_latency_penalty_drops_one_bar ---");
    // tested=20, est_net=60 (target=30), raw_bars=20*5/30=3 → AttachedGood.
    // p75=200ms applies -1 penalty → 2 bars → AttachedFair.
    let state = compute_attachment_state(&inputs(20, 25, 20, 60, Some(200), 0));
    assert_eq!(state, AttachmentState::AttachedFair);
}

pub fn test_severe_latency_drops_three_bars() {
    info!("--- test_severe_latency_drops_three_bars ---");
    // tested=32, est_net=100 (target=32), raw_bars=5. p75=600ms → -3 → 2 bars.
    let state = compute_attachment_state(&inputs(32, 35, 32, 100, Some(600), 0));
    assert_eq!(state, AttachmentState::AttachedFair);
}

pub fn test_no_latency_samples_no_penalty() {
    info!("--- test_no_latency_samples_no_penalty ---");
    // tested=16, est_net=50 (target=25), raw_bars=16*5/25=3. AttachedGood.
    let state = compute_attachment_state(&inputs(16, 20, 16, 50, None, 0));
    assert_eq!(state, AttachmentState::AttachedGood);
}

pub fn test_bar_count_mapping() {
    info!("--- test_bar_count_mapping ---");
    assert_eq!(AttachmentState::Detached.bar_count(), 0);
    assert_eq!(AttachmentState::Detaching.bar_count(), 0);
    assert_eq!(AttachmentState::Attaching.bar_count(), 0);
    assert_eq!(AttachmentState::AttachedWeak.bar_count(), 1);
    assert_eq!(AttachmentState::AttachedFair.bar_count(), 2);
    assert_eq!(AttachmentState::AttachedGood.bar_count(), 3);
    assert_eq!(AttachmentState::AttachedStrong.bar_count(), 4);
    assert_eq!(AttachmentState::AttachedFull.bar_count(), 5);
}
