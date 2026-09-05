pub mod fixture_veilid_core;
pub mod mock_registry;

use super::*;
pub use fixture_veilid_core::*;

pub fn fake_veilid_config() -> VeilidConfig {
    VeilidConfig {
        program_name: "Bob".to_string(),
        namespace: "Internets".to_string(),
        capabilities: VeilidConfigCapabilities {
            disable: Vec::new(),
        },
        protected_store: VeilidConfigProtectedStore {
            allow_insecure_fallback: true,
            always_use_insecure_storage: false,
            directory: "/root".to_string(),
            delete: true,
            device_encryption_key_password: "1234".to_string(),
            new_device_encryption_key_password: Some("5678".to_string()),
        },
        table_store: VeilidConfigTableStore {
            directory: "Yellow Pages".to_string(),
            delete: false,
            wipe_on_invalid_device_encryption_key: true,
            max_value_size_mb: 64,
        },
        block_store: VeilidConfigBlockStore {
            directory: "C:\\Program Files".to_string(),
            delete: true,
        },
        network: VeilidConfigNetwork {
            max_connections: 32,
            network_key_password: None,
            routing_table: VeilidConfigRoutingTable {
                public_keys: PublicKeyGroup::new(),
                secret_keys: SecretKeyGroup::new(),
                bootstrap: vec!["boots".to_string()],
                bootstrap_keys: vec![PublicKey::from_str(
                    "VLD0:qrxwD1-aM9xiUw4IAPVXE_4qgoIfyR4Y6MEPyaDl_GQ",
                )
                .unwrap()],
            },
            rpc: VeilidConfigRPC {
                default_route_hop_count: 8,
            },
            dht: VeilidConfigDHT {
                local_subkey_cache_size: 14,
                local_max_subkey_cache_memory_mb: 15,
                remote_subkey_cache_size: 16,
                remote_max_records: 17,
                remote_max_subkey_cache_memory_mb: 18,
                remote_max_storage_space_mb: 19,
                max_concurrent_operations: 16,
            },
            address_types: Vec::new(),
            upnp: true,
            detect_address_changes: Some(false),
            tls: VeilidConfigTLS {
                certificate_path: "/etc/ssl/certs/cert.pem".to_string(),
                private_key_path: "/etc/ssl/keys/key.pem".to_string(),
                connection_initial_timeout_ms: 1000,
            },
            protocol: VeilidConfigProtocol {
                udp: VeilidConfigUDP {
                    enabled: false,
                    listen_address: "10.0.0.2".to_string(),
                    public_address: Some("2.3.4.5".to_string()),
                },
                tcp: VeilidConfigTCP {
                    connect: true,
                    listen: false,
                    listen_address: "10.0.0.1".to_string(),
                    public_address: Some("1.2.3.4".to_string()),
                },
                ws: VeilidConfigWS {
                    connect: false,
                    listen: true,
                    listen_address: "127.0.0.1".to_string(),
                    path: "Straight".to_string(),
                    url: Some("https://veilid.com/ws".to_string()),
                },
                #[cfg(feature = "enable-protocol-wss")]
                wss: VeilidConfigWSS {
                    connect: true,
                    listen: false,
                    listen_address: "::1".to_string(),
                    path: "Curved".to_string(),
                    url: Some("https://veilid.com/wss".to_string()),
                },
            },
            privacy: VeilidConfigPrivacy {
                require_inbound_relay: false,
                #[cfg(feature = "geolocation")]
                country_code_denylist: vec![CountryCode::from_str("NZ").unwrap()],
            },
            #[cfg(feature = "virtual-network")]
            virtual_network: VeilidConfigVirtualNetwork {
                enabled: false,
                server_address: "".to_owned(),
            },
        },
        internal: Some(VeilidConfigInternal {
            network: VeilidConfigInternalNetwork {
                connection_initial_timeout_ms: 1000,
                connection_inactivity_timeout_ms: 2000,
                max_connections_per_ip4: 3000,
                max_connections_per_ip6_prefix: 4000,
                max_connections_per_ip6_prefix_size: 5000,
                max_connection_frequency_per_min: 6000,
                client_allowlist_timeout_ms: 7000,
                reverse_connection_receipt_time_ms: 8000,
                hole_punch_receipt_time_ms: 9000,
                restricted_nat_retries: 10000,
                rpc: VeilidConfigInternalRPC {
                    concurrency: 5,
                    queue_size: 6,
                    max_timestamp_behind_ms: Some(1000),
                    max_timestamp_ahead_ms: Some(2000),
                    timeout_ms: 3000,
                    max_route_hop_count: 7,
                },
                dht: VeilidConfigInternalDHT {
                    max_find_node_count: 1,
                    resolve_node_timeout_ms: 10000,
                    resolve_node_count: 3,
                    resolve_node_fanout: 4,
                    get_value_timeout_ms: 100000,
                    get_value_count: 3,
                    get_value_fanout: 4,
                    set_value_timeout_ms: 10000,
                    set_value_count: 5,
                    set_value_fanout: 4,
                    consensus_width: 10,
                    min_peer_count: 11,
                    min_peer_refresh_time_ms: 12,
                    validate_dial_info_receipt_time_ms: 13,
                    max_watch_expiration_ms: 22,
                    public_watch_limit: 20,
                    member_watch_limit: 21,
                    public_transaction_limit: 23,
                    member_transaction_limit: 24,
                },
                protocol: VeilidConfigInternalProtocol {
                    udp: VeilidConfigInternalUDP {
                        socket_pool_size: 30,
                    },
                },
            },
        }),
    }
}
