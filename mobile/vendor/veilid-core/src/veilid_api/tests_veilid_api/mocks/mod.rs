use super::*;
use crate::crypto::tests_crypto::*;

// Mocks used by various tests

pub fn fake_latency_stats() -> LatencyStats {
    LatencyStats {
        fastest: TimestampDuration::from(1234),
        average: TimestampDuration::from(2345),
        slowest: TimestampDuration::from(3456),
        tm90: TimestampDuration::from(4567),
        tm75: TimestampDuration::from(5678),
        p90: TimestampDuration::from(6789),
        p75: TimestampDuration::from(7890),
    }
}

pub fn fake_transfer_stats() -> TransferStats {
    TransferStats {
        total: ByteCount::from(1_000_000),
        maximum: ByteCount::from(3456),
        average: ByteCount::from(2345),
        minimum: ByteCount::from(1234),
    }
}

pub fn fake_transfer_stats_down_up() -> TransferStatsDownUp {
    TransferStatsDownUp {
        down: fake_transfer_stats(),
        up: fake_transfer_stats(),
    }
}

pub fn fake_peer_stats() -> PeerStats {
    PeerStats {
        latency: Some(fake_latency_stats()),
        transfer: fake_transfer_stats_down_up(),
    }
}

pub fn fake_peer_table_data() -> PeerTableData {
    PeerTableData {
        node_ids: vec![fake_node_id()],
        peer_address: "123 Main St.".to_string(),
        peer_stats: fake_peer_stats(),
    }
}

pub fn fake_veilid_value_change() -> VeilidValueChange {
    VeilidValueChange {
        key: fake_record_key(),
        subkeys: ValueSubkeyRangeSet::new(),
        count: 5,
        value: Some(
            ValueData::new_with_seq(23.into(), b"ValueData".to_vec(), fake_public_key()).unwrap(),
        ),
    }
}
