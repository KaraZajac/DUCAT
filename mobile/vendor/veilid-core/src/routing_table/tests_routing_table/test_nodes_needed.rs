use crate::routing_table::*;

const MIN_PEER_COUNT: usize = 20;

fn needed(
    counts: KindNodeCounts,
    no_responsive_elapsed: Option<TimestampDuration>,
) -> KindNodesNeeded {
    nodes_needed_for_counts(counts, MIN_PEER_COUNT, no_responsive_elapsed)
}

pub fn test_nodes_needed_empty_table_bootstraps() {
    info!("--- test_nodes_needed_empty_table_bootstraps ---");

    let n = needed(
        KindNodeCounts::default(),
        Some(TimestampDuration::new_secs(0)),
    );
    assert!(n.needs_bootstrap);
    assert!(!n.needs_peer_minimum_refresh);
    assert!(!n.needs_more_tested_nodes);
}

pub fn test_nodes_needed_stale_table_inflation_capped() {
    info!("--- test_nodes_needed_stale_table_inflation_capped ---");

    // 200 loaded Initial entries: live but not responsive; bar must cap at 12, not 50
    let counts = KindNodeCounts {
        maybe_live: 200,
        responsive: 12,
        live_external: 200,
        low_water_mark: 0,
    };
    let n = needed(counts, None);
    assert!(!n.needs_bootstrap);
    assert!(!n.needs_peer_minimum_refresh);
    assert!(!n.needs_more_tested_nodes);

    // One short of the cap still needs testing, and refresh keeps gathering
    let n = needed(
        KindNodeCounts {
            responsive: 11,
            ..counts
        },
        None,
    );
    assert!(n.needs_more_tested_nodes);
    assert!(n.needs_peer_minimum_refresh);
}

pub fn test_nodes_needed_fresh_small_table_uses_fraction() {
    info!("--- test_nodes_needed_fresh_small_table_uses_fraction ---");

    // Fresh bootstrap: 24 live nodes, bar = 0.25 * 24 = 6
    let counts = KindNodeCounts {
        maybe_live: 24,
        responsive: 5,
        live_external: 24,
        low_water_mark: 0,
    };
    assert!(needed(counts, None).needs_more_tested_nodes);
    assert!(
        !needed(
            KindNodeCounts {
                responsive: 6,
                ..counts
            },
            None
        )
        .needs_more_tested_nodes
    );
}

pub fn test_nodes_needed_low_water_mark_term_capped() {
    info!("--- test_nodes_needed_low_water_mark_term_capped ---");

    // LWM 40 would demand 20 tested; cap holds it at 12
    let counts = KindNodeCounts {
        maybe_live: 40,
        responsive: 12,
        live_external: 10,
        low_water_mark: 40,
    };
    assert!(!needed(counts, None).needs_more_tested_nodes);
}

pub fn test_nodes_needed_fallback_bootstrap_timing() {
    info!("--- test_nodes_needed_fallback_bootstrap_timing ---");

    // Stale table: plenty maybe-live, zero responsive
    let counts = KindNodeCounts {
        maybe_live: 50,
        responsive: 0,
        live_external: 50,
        low_water_mark: 0,
    };
    // Within the grace period: no bootstrap, PMR gathers instead
    let n = needed(counts, Some(TimestampDuration::new_secs(5)));
    assert!(!n.needs_bootstrap);
    assert!(n.needs_peer_minimum_refresh);
    // Past the grace period: fall back to bootstrap, which suppresses PMR
    let n = needed(counts, Some(TimestampDuration::new_secs(10)));
    assert!(n.needs_bootstrap);
    assert!(!n.needs_peer_minimum_refresh);
    // Any responsive node clears the timer upstream (elapsed = None)
    let n = needed(
        KindNodeCounts {
            responsive: 1,
            ..counts
        },
        None,
    );
    assert!(!n.needs_bootstrap);
}

pub fn test_nodes_needed_peer_minimum_refresh_scoped_to_attach() {
    info!("--- test_nodes_needed_peer_minimum_refresh_scoped_to_attach ---");

    // Stale attach: below the tested bar fires refresh even with many maybe-live nodes
    let counts = KindNodeCounts {
        maybe_live: 100,
        responsive: 5,
        live_external: 100,
        low_water_mark: 0,
    };
    assert!(needed(counts, None).needs_peer_minimum_refresh);
    // Tested bar met: refresh goes quiet even though responsive < min_peer_count
    assert!(
        !needed(
            KindNodeCounts {
                responsive: 12,
                ..counts
            },
            None
        )
        .needs_peer_minimum_refresh
    );
    // Small known-node pool still fires refresh regardless of the tested bar
    let n = needed(
        KindNodeCounts {
            maybe_live: 10,
            responsive: 10,
            live_external: 10,
            low_water_mark: 0,
        },
        None,
    );
    assert!(n.needs_peer_minimum_refresh);
    assert!(!n.needs_more_tested_nodes);
}
