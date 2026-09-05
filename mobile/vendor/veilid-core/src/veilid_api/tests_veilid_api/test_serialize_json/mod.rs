mod test_types;
mod test_types_dht;
mod test_types_dht_schema;

use test_types::*;
use test_types_dht::*;
use test_types_dht_schema::*;

use super::*;

pub fn test_serialize_json() {
    // test_types
    test_aligned_u64();
    test_veilid_app_message();
    test_veilid_app_call();
    test_crypto_kind();
    test_sequencing();
    test_stability();
    test_safety_selection();
    test_safety_spec();
    test_latency_stats();
    test_transfer_stats();
    test_transfer_stats_down_up();
    test_peer_stats();
    #[cfg(feature = "unstable-tunnels")]
    test_tunnel_mode();
    #[cfg(feature = "unstable-tunnels")]
    test_tunnel_error();
    #[cfg(feature = "unstable-tunnels")]
    test_tunnel_endpoint();
    #[cfg(feature = "unstable-tunnels")]
    test_full_tunnel();
    #[cfg(feature = "unstable-tunnels")]
    test_partial_tunnel();
    test_veilid_log_level();
    test_veilid_log();
    test_attachment_state();
    test_veilid_state_attachment();
    test_peer_table_data();
    test_veilid_state_network();
    test_veilid_route_change();
    test_veilid_state_config();
    test_veilid_value_change();
    test_veilid_update();
    test_veilid_state();
    // test_types_dht
    test_dht_record_descriptor();
    test_value_data();
    test_value_subkey_range_set();
    // test_types_dht_schema
    test_dht_schema_dflt();
    test_dht_schema();
    test_dht_schema_smpl_member();
    test_dht_schema_smpl();
}
