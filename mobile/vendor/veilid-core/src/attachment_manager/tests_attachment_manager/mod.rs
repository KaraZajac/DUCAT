pub mod test_attachment_level_calculator;

use super::*;

#[expect(clippy::unused_async)]
pub async fn test_all() {
    test_attachment_level_calculator::test_attaching_when_no_live_peers();
    test_attachment_level_calculator::test_weak_floor_when_live_but_untested();
    test_attachment_level_calculator::test_well_attached_server();
    test_attachment_level_calculator::test_minimal_test_node();
    test_attachment_level_calculator::test_partial_attached_no_latency_penalty();
    test_attachment_level_calculator::test_latency_penalty_drops_one_bar();
    test_attachment_level_calculator::test_severe_latency_drops_three_bars();
    test_attachment_level_calculator::test_no_latency_samples_no_penalty();
    test_attachment_level_calculator::test_bar_count_mapping();
}
