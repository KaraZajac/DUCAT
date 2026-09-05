pub mod mocks;
pub mod test_bucket_entry_state;
pub mod test_network_estimator;
pub mod test_nodes_needed;
pub mod test_routing_metrics;
pub mod test_serialize_routing_table;
pub mod test_signed_node_info;

pub use mocks::*;

use super::*;

pub async fn test_all() {
    test_serialize_routing_table::test_kick_preserves_best_node_id().await;
    test_serialize_routing_table::test_load_rejects_entries_without_node_ids().await;
    test_serialize_routing_table::test_replace_node_ids_rejects_without_valid_id().await;
    test_serialize_routing_table::test_routingtable_buckets_round_trip().await;
    test_serialize_routing_table::test_round_trip_peerinfo().await;
    test_signed_node_info::test_signed_node_info().await;
    test_routing_metrics::test_bucket_depth();
    test_routing_metrics::test_practical_max_size_zero();
    test_routing_metrics::test_practical_max_size_small_network_holds_most_nodes();
    test_routing_metrics::test_practical_max_size_known_values();
    test_routing_metrics::test_practical_max_size_monotonic();
    test_network_estimator::test_network_estimator_empty();
    test_network_estimator::test_network_estimator_lowest_unsaturated_bucket();
    test_network_estimator::test_network_estimator_skips_saturated_bucket_0();
    test_network_estimator::test_network_estimator_high_water_across_slots();
    test_network_estimator::test_network_estimator_only_zeroes_entered_slot();
    test_network_estimator::test_network_estimator_entered_slot_zeros_old_data();
    test_network_estimator::test_network_estimator_combined_single_kind();
    test_network_estimator::test_network_estimator_clock_backward_ignored();
    test_nodes_needed::test_nodes_needed_empty_table_bootstraps();
    test_nodes_needed::test_nodes_needed_stale_table_inflation_capped();
    test_nodes_needed::test_nodes_needed_fresh_small_table_uses_fraction();
    test_nodes_needed::test_nodes_needed_low_water_mark_term_capped();
    test_nodes_needed::test_nodes_needed_fallback_bootstrap_timing();
    test_nodes_needed::test_nodes_needed_peer_minimum_refresh_scoped_to_attach();
    test_bucket_entry_state::test_state_punished();
    test_bucket_entry_state::test_state_dead_excessive_unreachable();
    test_bucket_entry_state::test_state_dead_excessive_send_failures();
    test_bucket_entry_state::test_state_dead_never_seen_lost_questions();
    test_bucket_entry_state::test_state_dead_steady_lost_questions();
    test_bucket_entry_state::test_state_missing_failed_to_send();
    test_bucket_entry_state::test_state_missing_lost_questions();
    test_bucket_entry_state::test_state_initial();
    test_bucket_entry_state::test_state_unreliable();
    test_bucket_entry_state::test_state_reliable();
    test_bucket_entry_state::test_state_precedence_dead_beats_reliable();
    test_bucket_entry_state::test_state_transition_reliable_degrades_to_missing();
    test_bucket_entry_state::test_state_offline_credit_steady_lost_questions();
    test_bucket_entry_state::test_state_offline_credit_unreliable_answer();
}
