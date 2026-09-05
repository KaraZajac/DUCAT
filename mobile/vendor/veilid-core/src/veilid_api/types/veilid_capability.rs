use super::*;

fourcc_type!(VeilidCapability);

/// Node can relay private routes for other nodes.
pub const VEILID_CAPABILITY_ROUTE: VeilidCapability = VeilidCapability::new(*b"ROUT");
/// Node can tunnel streams for other nodes.
#[cfg(feature = "unstable-tunnels")]
pub const VEILID_CAPABILITY_TUNNEL: VeilidCapability = VeilidCapability::new(*b"TUNL");
/// Node can perform reverse-connect signalling to help nodes behind NAT make inbound connections.
pub const VEILID_CAPABILITY_SIGNAL: VeilidCapability = VeilidCapability::new(*b"SGNL");
/// Node can act as an inbound relay for nodes that are not directly reachable.
pub const VEILID_CAPABILITY_RELAY: VeilidCapability = VeilidCapability::new(*b"RLAY");
/// Node can validate another node's claimed dial info by attempting to reach it.
pub const VEILID_CAPABILITY_VALIDATE_DIAL_INFO: VeilidCapability = VeilidCapability::new(*b"DIAL");
/// Node participates in the distributed hash table.
pub const VEILID_CAPABILITY_DHT: VeilidCapability = VeilidCapability::new(*b"DHTV");
/// Node accepts application messages and calls.
pub const VEILID_CAPABILITY_APPMESSAGE: VeilidCapability = VeilidCapability::new(*b"APPM");
/// Node offers block storage.
#[cfg(feature = "unstable-blockstore")]
pub const VEILID_CAPABILITY_BLOCKSTORE: VeilidCapability = VeilidCapability::new(*b"BLOC");

/// Capabilities that participate in the DHT distance metric, used when finding nodes close to a key.
pub const DISTANCE_METRIC_CAPABILITIES: &[VeilidCapability] = &[VEILID_CAPABILITY_DHT];
/// Capabilities that contribute to network connectivity, used when refreshing the routing table.
pub const CONNECTIVITY_CAPABILITIES: &[VeilidCapability] = &[
    VEILID_CAPABILITY_RELAY,
    VEILID_CAPABILITY_SIGNAL,
    VEILID_CAPABILITY_ROUTE,
    VEILID_CAPABILITY_VALIDATE_DIAL_INFO,
];

cfg_if! {
    if #[cfg(all(target_arch = "wasm32", target_os = "unknown"))] {
        /// Capabilities advertised by this node in the PublicInternet routing domain.
        pub const PUBLIC_INTERNET_CAPABILITIES: &[VeilidCapability] = &[
            VEILID_CAPABILITY_ROUTE,
            #[cfg(feature = "unstable-tunnels")]
            VEILID_CAPABILITY_TUNNEL,
            VEILID_CAPABILITY_SIGNAL,
            VEILID_CAPABILITY_DHT,
            VEILID_CAPABILITY_APPMESSAGE,
            #[cfg(feature = "unstable-blockstore")]
            VEILID_CAPABILITY_BLOCKSTORE,
        ];

        /// Capabilities advertised by this node in the LocalNetwork routing domain.
        pub const LOCAL_NETWORK_CAPABILITIES: &[VeilidCapability] = &[VEILID_CAPABILITY_APPMESSAGE];
    } else {
        /// Capabilities advertised by this node in the PublicInternet routing domain.
        pub const PUBLIC_INTERNET_CAPABILITIES: &[VeilidCapability] = &[
            VEILID_CAPABILITY_ROUTE,
            #[cfg(feature = "unstable-tunnels")]
            VEILID_CAPABILITY_TUNNEL,
            VEILID_CAPABILITY_SIGNAL,
            VEILID_CAPABILITY_RELAY,
            VEILID_CAPABILITY_VALIDATE_DIAL_INFO,
            VEILID_CAPABILITY_DHT,
            VEILID_CAPABILITY_APPMESSAGE,
            #[cfg(feature = "unstable-blockstore")]
            VEILID_CAPABILITY_BLOCKSTORE,
        ];

        /// Capabilities advertised by this node in the LocalNetwork routing domain.
        pub const LOCAL_NETWORK_CAPABILITIES: &[VeilidCapability] =
            &[VEILID_CAPABILITY_RELAY, VEILID_CAPABILITY_APPMESSAGE];
    }
}

/// Maximum number of capabilities a node may advertise.
pub const MAX_CAPABILITIES: usize = 64;
