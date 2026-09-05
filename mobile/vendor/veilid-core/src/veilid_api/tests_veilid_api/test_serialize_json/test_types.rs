use super::*;
use crate::crypto::tests_crypto::*;
use crate::tests::*;

// aligned_u64

pub fn test_aligned_u64() {
    let orig = AlignedU64::new(0x0123456789abcdef);
    let copy = deserialize_json(&serialize_json(orig)).unwrap();

    assert_eq!(orig, copy);
}

// app_messsage_call

pub fn test_veilid_app_message() {
    let orig = VeilidAppMessage::new(
        Some(fake_node_id()),
        Some(fake_route_id()),
        b"Hi there!".to_vec(),
    );
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_veilid_app_call() {
    let orig = VeilidAppCall::new(
        Some(fake_node_id()),
        Some(fake_route_id()),
        b"Well, hello!".to_vec(),
        OperationId::from(123),
    );
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

// fourcc

pub fn test_crypto_kind() {
    let orig = CryptoKind::from_str("D34D").unwrap();
    let copy = deserialize_json(&serialize_json(orig)).unwrap();

    assert_eq!(orig, copy);
}

// safety

pub fn test_sequencing() {
    let orig = Sequencing::PreferOrdered;
    let copy = deserialize_json(&serialize_json(orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_stability() {
    let orig = Stability::Reliable;
    let copy = deserialize_json(&serialize_json(orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_safety_selection() {
    let orig = SafetySelection::Unsafe(Sequencing::EnsureOrdered);
    let copy = deserialize_json(&serialize_json(orig.clone())).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_safety_spec() {
    let orig = SafetySpec {
        preferred_route: Some(fake_route_id()),
        hop_count: 23,
        stability: Stability::default(),
        sequencing: Sequencing::default(),
    };
    let copy = deserialize_json(&serialize_json(orig.clone())).unwrap();

    assert_eq!(orig, copy);
}

// stats

pub fn test_latency_stats() {
    let orig = fake_latency_stats();
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_transfer_stats() {
    let orig = fake_transfer_stats();
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_transfer_stats_down_up() {
    let orig = fake_transfer_stats_down_up();
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_peer_stats() {
    let orig = fake_peer_stats();
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

//  tunnel

#[cfg(feature = "unstable-tunnels")]
pub fn test_tunnel_mode() {
    let orig = TunnelMode::Raw;
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

#[cfg(feature = "unstable-tunnels")]
pub fn test_tunnel_error() {
    let orig = TunnelError::NoCapacity;
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

#[cfg(feature = "unstable-tunnels")]
pub fn test_tunnel_endpoint() {
    let orig = TunnelEndpoint {
        mode: TunnelMode::Raw,
        description: "Here there be tygers.".to_string(),
    };
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

#[cfg(feature = "unstable-tunnels")]
pub fn test_full_tunnel() {
    let orig = FullTunnel {
        id: AlignedU64::from(42),
        timeout: AlignedU64::from(3_000_000),
        local: TunnelEndpoint {
            mode: TunnelMode::Turn,
            description: "Left end.".to_string(),
        },
        remote: TunnelEndpoint {
            mode: TunnelMode::Turn,
            description: "Right end.".to_string(),
        },
    };
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

#[cfg(feature = "unstable-tunnels")]
pub fn test_partial_tunnel() {
    let orig = PartialTunnel {
        id: AlignedU64::from(42),
        timeout: AlignedU64::from(3_000_000),
        local: TunnelEndpoint {
            mode: TunnelMode::Turn,
            description: "I'm so lonely.".to_string(),
        },
    };
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

// veilid_log

pub fn test_veilid_log_level() {
    let orig = VeilidLogLevel::Info;
    let copy = deserialize_json(&serialize_json(orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_veilid_log() {
    let orig = VeilidLog {
        log_level: VeilidLogLevel::Debug,
        message: "A log! A log!".to_string(),
        backtrace: Some("Func1 -> Func2 -> Func3".to_string()),
    };
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

// veilid_state

pub fn test_attachment_state() {
    let orig = AttachmentState::AttachedFull;
    let copy = deserialize_json(&serialize_json(orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_veilid_state_attachment() {
    let orig = VeilidStateAttachment {
        state: AttachmentState::AttachedFair,
        public_internet_ready: true,
        local_network_ready: false,
        uptime: TimestampDuration::new_secs(10),
        attached_uptime: Some(TimestampDuration::new_secs(10)),
        reliable_peer_count: NodeCount::from(12),
        live_peer_count: NodeCount::from(18),
        estimated_network_size: NodeCount::from(60),
        median_latency: Some(TimestampDuration::new_ms(120)),
        over_attached_nodes: NodeCount::from(3),
    };
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_peer_table_data() {
    let orig = fake_peer_table_data();
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_veilid_state_network() {
    let orig = VeilidStateNetwork {
        started: true,
        bps_down: ByteCount::from(14_400),
        bps_up: ByteCount::from(1200),
        peers: vec![fake_peer_table_data()],
        node_ids: vec![fake_node_id()],
    };
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_veilid_route_change() {
    let orig = VeilidRouteChange {
        dead_routes: vec![fake_route_id()],
        dead_remote_routes: vec![fake_route_id()],
    };
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_veilid_state_config() {
    let orig = VeilidStateConfig {
        config: fake_veilid_config(),
    };
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_veilid_value_change() {
    let orig = fake_veilid_value_change();
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_veilid_update() {
    let orig = VeilidUpdate::ValueChange(Box::new(fake_veilid_value_change()));
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}

pub fn test_veilid_state() {
    let orig = VeilidState {
        attachment: Box::new(VeilidStateAttachment {
            state: AttachmentState::AttachedFull,
            public_internet_ready: true,
            local_network_ready: false,
            uptime: TimestampDuration::new_secs(900),
            attached_uptime: Some(TimestampDuration::new_secs(600)),
            reliable_peer_count: NodeCount::from(35),
            live_peer_count: NodeCount::from(48),
            estimated_network_size: NodeCount::from(120),
            median_latency: Some(TimestampDuration::new_ms(45)),
            over_attached_nodes: NodeCount::from(0),
        }),
        network: Box::new(VeilidStateNetwork {
            started: true,
            bps_down: ByteCount::from(14_400),
            bps_up: ByteCount::from(1200),
            peers: vec![fake_peer_table_data()],
            node_ids: vec![fake_node_id()],
        }),
        config: Box::new(VeilidStateConfig {
            config: fake_veilid_config(),
        }),
    };
    let copy = deserialize_json(&serialize_json(&orig)).unwrap();

    assert_eq!(orig, copy);
}
