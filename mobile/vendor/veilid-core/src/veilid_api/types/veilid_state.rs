use super::*;

/// Attachment abstraction for network 'signal strength'.
#[apply(api_data_enum!)]
#[api(eq, copy, default, ts(namespace, from_wasm_abi, into_wasm_abi))]
pub enum AttachmentState {
    /// Not attached to the network.
    #[default]
    Detached = 0,
    /// Attaching, but not yet able to perform network operations.
    Attaching = 1,
    /// Attached with the weakest signal strength.
    AttachedWeak = 2,
    /// Attached with fair signal strength.
    AttachedFair = 3,
    /// Attached with good signal strength.
    AttachedGood = 4,
    /// Attached with strong signal strength.
    AttachedStrong = 5,
    /// Attached with the strongest signal strength.
    AttachedFull = 6,
    /// Detaching from the network and shutting down connections.
    Detaching = 7,
}
impl AttachmentState {
    /// Returns true when the node is fully detached from the network.
    #[must_use]
    pub fn is_detached(&self) -> bool {
        matches!(self, Self::Detached)
    }
    /// Returns true when the node is attached at any signal strength.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        matches!(
            self,
            Self::AttachedWeak
                | Self::AttachedFair
                | Self::AttachedGood
                | Self::AttachedStrong
                | Self::AttachedFull
        )
    }

    /// Signal-strength bars (0..=5).
    /// Detached/Detaching/Attaching → 0
    /// AttachedWeak → 1, AttachedFair → 2, AttachedGood → 3,
    /// AttachedStrong → 4, AttachedFull → 5.
    #[must_use]
    pub fn bar_count(&self) -> u8 {
        match self {
            Self::Detached | Self::Detaching | Self::Attaching => 0,
            Self::AttachedWeak => 1,
            Self::AttachedFair => 2,
            Self::AttachedGood => 3,
            Self::AttachedStrong => 4,
            Self::AttachedFull => 5,
        }
    }
}

impl fmt::Display for AttachmentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let out = if f.alternate() {
            match self {
                AttachmentState::Attaching => "Attaching",
                AttachmentState::AttachedWeak => "AttachedWeak",
                AttachmentState::AttachedFair => "AttachedFair",
                AttachmentState::AttachedGood => "AttachedGood",
                AttachmentState::AttachedStrong => "AttachedStrong",
                AttachmentState::AttachedFull => "AttachedFull",
                AttachmentState::Detaching => "Detaching",
                AttachmentState::Detached => "Detached",
            }
        } else {
            match self {
                AttachmentState::Attaching => "attaching",
                AttachmentState::AttachedWeak => "attached_weak",
                AttachmentState::AttachedFair => "attached_fair",
                AttachmentState::AttachedGood => "attached_good",
                AttachmentState::AttachedStrong => "attached_strong",
                AttachmentState::AttachedFull => "attached_full",
                AttachmentState::Detaching => "detaching",
                AttachmentState::Detached => "detached",
            }
        };

        write!(f, "{}", out)
    }
}

impl TryFrom<String> for AttachmentState {
    type Error = ();

    fn try_from(s: String) -> Result<Self, Self::Error> {
        AttachmentState::try_from(s.as_ref())
    }
}

impl TryFrom<&str> for AttachmentState {
    type Error = ();

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Ok(match s {
            "attaching" => AttachmentState::Attaching,
            "attached_weak" => AttachmentState::AttachedWeak,
            "attached_fair" => AttachmentState::AttachedFair,
            "attached_good" => AttachmentState::AttachedGood,
            "attached_strong" => AttachmentState::AttachedStrong,
            "attached_full" => AttachmentState::AttachedFull,
            "detaching" => AttachmentState::Detaching,
            "detached" => AttachmentState::Detached,
            _ => return Err(()),
        })
    }
}

/// Describe the attachment state of the Veilid node
#[apply(api_data_struct!)]
#[api(eq, default, ts)]
pub struct VeilidStateAttachment {
    /// Overall network quality (signal-strength bars).
    pub state: AttachmentState,
    /// Whether the PublicInternet routing domain is ready for all operations.
    pub public_internet_ready: bool,
    /// Whether the LocalNetwork routing domain is ready for all operations.
    pub local_network_ready: bool,
    /// Node uptime.
    pub uptime: TimestampDuration,
    /// Uptime since last attach, None when detached.
    pub attached_uptime: Option<TimestampDuration>,
    /// Reliable peers in the routing table.
    pub reliable_peer_count: NodeCount,
    /// Live peers (reliable, unreliable, and newly added) in the routing table.
    pub live_peer_count: NodeCount,
    /// Smoothed estimate of total reachable network size.
    pub estimated_network_size: NodeCount,
    /// Median p75 latency across reliable peers, None when no samples yet.
    pub median_latency: Option<TimestampDuration>,
    /// Entries in bucket overflow awaiting lazy kick.
    pub over_attached_nodes: NodeCount,
}

impl fmt::Display for VeilidStateAttachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "state: {}\npublic_internet_ready: {}\nlocal_network_ready: {}\nuptime: {}\nattached_uptime: {}\nreliable_peer_count: {}\nlive_peer_count: {}\nestimated_network_size: {}\nmedian_latency: {}\nover_attached_nodes: {}",
            f.to_string(self.state),
            if self.public_internet_ready { "true" } else { "false" },
            if self.local_network_ready { "true" } else { "false" },
            f.to_string(self.uptime),
            f.to_string_opt(self.attached_uptime.as_ref()),
            self.reliable_peer_count,
            self.live_peer_count,
            self.estimated_network_size,
            f.to_string_opt(self.median_latency.as_ref()),
            self.over_attached_nodes,
        )
    }
}

/// Describe a recently accessed peer
#[apply(api_data_struct!)]
#[api(eq, ts)]
pub struct PeerTableData {
    /// The node ids used by this peer
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<String>"))]
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        tsify(type = "string[]")
    )]
    pub node_ids: Vec<NodeId>,
    /// The peer's human readable address.
    pub peer_address: String,
    /// Statistics we have collected on this peer.
    pub peer_stats: PeerStats,
}

impl fmt::Display for PeerTableData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "node_ids: [{}]\npeer_address: {}\npeer_stats: {}",
            self.node_ids
                .iter()
                .map(|id| f.to_string(id))
                .collect::<Vec<_>>()
                .join(", "),
            self.peer_address,
            indent_all_string(f.to_string(&self.peer_stats))
        )
    }
}

/// Describe the current network state of the Veilid node
#[apply(api_data_struct!)]
#[api(eq, default, ts)]
pub struct VeilidStateNetwork {
    /// If the network has been started or not.
    pub started: bool,
    /// The total number of bytes per second used by Veilid currently in the download direction.
    pub bps_down: ByteCount,
    /// The total number of bytes per second used by Veilid currently in the upload direction.
    pub bps_up: ByteCount,
    /// The list of most recently accessed peers.
    /// This is not an active connection table, nor is representative of the entire routing table.
    pub peers: Vec<PeerTableData>,
    /// The list of node ids for this node
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<String>"))]
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        tsify(type = "string[]")
    )]
    pub node_ids: Vec<NodeId>,
}

impl fmt::Display for VeilidStateNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "started: {}\nbps_down: {}\nbps_up: {}\npeers: [{}]\nnode_ids: [{}]",
            if self.started { "true" } else { "false" },
            f.to_string(self.bps_down),
            f.to_string(self.bps_up),
            f.to_table_string(&self.peers),
            self.node_ids
                .iter()
                .map(|id| f.to_string(id))
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

/// Describe a private or safety route change that has happened
#[apply(api_data_struct!)]
#[api(eq, default, ts)]
pub struct VeilidRouteChange {
    /// If an allocated route dies or was released, it is listed here.
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<String>"))]
    pub dead_routes: Vec<RouteId>,
    /// If an imported remote private route has died, it is listed here.
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<String>"))]
    pub dead_remote_routes: Vec<RouteId>,
}

impl fmt::Display for VeilidRouteChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "dead_routes: [{}]\ndead_remote_routes: [{}]",
            self.dead_routes
                .iter()
                .map(|id| f.to_string(id))
                .collect::<Vec<_>>()
                .join(", "),
            self.dead_remote_routes
                .iter()
                .map(|id| f.to_string(id))
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

/// Describe changes to the Veilid node configuration
/// Currently this is only ever emitted once, however we reserve the right to
/// add the ability to change the configuration or have it changed by the Veilid node
/// itself during runtime.
#[apply(api_data_struct!)]
#[api(eq, default, ts)]
pub struct VeilidStateConfig {
    /// If the Veilid node configuration has changed the full new config will be here.
    pub config: VeilidConfig,
}

impl fmt::Display for VeilidStateConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "config: {}", serialize_json_pretty(&self.config))
    }
}

/// Describe when DHT records have subkey values changed
#[apply(api_data_struct!)]
#[api(eq, ts)]
pub struct VeilidValueChange {
    /// The DHT Record key that changed
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    pub key: RecordKey,
    /// The portion of the DHT Record's subkeys that have changed
    /// If the subkey range is empty, any watch present on the value has died.
    pub subkeys: ValueSubkeyRangeSet,
    /// The count remaining on the watch that triggered this value change
    /// If there is no watch and this is received, it will be set to u32::MAX
    /// If this value is zero, any watch present on the value has died.
    pub count: u32,
    /// The (optional) value data for the first subkey in the subkeys range
    /// If 'subkeys' is not a single value, other values than the first value
    /// must be retrieved with RoutingContext::get_dht_value().
    pub value: Option<ValueData>,
}

impl fmt::Display for VeilidValueChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "key: {}\nsubkeys: {}\ncount: {}\nvalue: {}",
            f.to_string(&self.key),
            f.to_string(&self.subkeys),
            self.count,
            f.to_string_opt(self.value.as_ref())
        )
    }
}

/// An update from the veilid-core to the host application describing a change
/// to the internal state of the Veilid node.
#[apply(api_data_enum!)]
#[api(eq, ts(into_wasm_abi))]
#[serde(tag = "kind")]
pub enum VeilidUpdate {
    /// A log message emitted by veilid-core.
    Log(Box<VeilidLog>),
    /// A one-way application message delivered over the network.
    AppMessage(Box<VeilidAppMessage>),
    /// An application call expecting a reply from the host application.
    AppCall(Box<VeilidAppCall>),
    /// The node's attachment state has changed.
    Attachment(Box<VeilidStateAttachment>),
    /// The node's network state has changed.
    Network(Box<VeilidStateNetwork>),
    /// The node's configuration has changed.
    Config(Box<VeilidStateConfig>),
    /// An allocated or imported route has died or been released.
    RouteChange(Box<VeilidRouteChange>),
    /// A watched DHT record has changed subkey values.
    ValueChange(Box<VeilidValueChange>),
    /// The node is shutting down; no further updates will follow.
    Shutdown,
}

impl fmt::Display for VeilidUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            VeilidUpdate::Log(log) => write!(f, "Log: {}", indent_all_string(f.to_string(log))),
            VeilidUpdate::AppMessage(app_message) => write!(
                f,
                "AppMessage: {}",
                indent_all_string(f.to_string(app_message))
            ),
            VeilidUpdate::AppCall(app_call) => {
                write!(f, "AppCall: {}", indent_all_string(f.to_string(app_call)))
            }
            VeilidUpdate::Attachment(attachment) => write!(
                f,
                "Attachment: {}",
                indent_all_string(f.to_string(attachment))
            ),
            VeilidUpdate::Network(network) => {
                write!(f, "Network: {}", indent_all_string(f.to_string(network)))
            }
            VeilidUpdate::Config(config) => {
                write!(f, "Config: {}", indent_all_string(f.to_string(config)))
            }
            VeilidUpdate::RouteChange(route_change) => write!(
                f,
                "RouteChange: {}",
                indent_all_string(f.to_string(route_change))
            ),
            VeilidUpdate::ValueChange(value_change) => write!(
                f,
                "ValueChange: {}",
                indent_all_string(f.to_string(value_change))
            ),
            VeilidUpdate::Shutdown => write!(f, "Shutdown"),
        }
    }
}
/// A queriable state of the internals of veilid-core.
#[apply(api_data_struct!)]
#[api(eq, default, ts(into_wasm_abi))]
pub struct VeilidState {
    /// The current attachment state.
    pub attachment: Box<VeilidStateAttachment>,
    /// The current network state.
    pub network: Box<VeilidStateNetwork>,
    /// The current node configuration.
    pub config: Box<VeilidStateConfig>,
}
