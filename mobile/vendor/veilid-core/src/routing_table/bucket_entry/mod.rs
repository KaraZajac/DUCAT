mod state;
mod state_reason;

use super::*;
#[cfg(feature = "tracking")]
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::{AtomicU32, Ordering};
use hashlink::LruCache;

pub(crate) use state::*;
pub(crate) use state_reason::*;

impl_veilid_log_facility!("rtab");

// Connectionless protocols like UDP are dependent on a NAT translation timeout
// We ping relays to maintain our UDP NAT state with a RELAY_KEEPALIVE_PING_INTERVAL_SECS=10 frequency
// since 30 seconds is a typical UDP NAT state timeout  .
// Non-relay flows are assumed to be alive for half the typical timeout and we regenerate the hole punch
// if it the flow hasn't had any activity in this amount of time.
pub(crate) const CONNECTIONLESS_TIMEOUT: TimestampDuration = TimestampDuration::new_secs(15);

#[derive(Debug, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct LastFlowKey {
    pub transport: TransportType,
}

impl fmt::Display for LastFlowKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", f.to_string(self.transport))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct LastSenderInfoKey {
    pub routing_domain: RoutingDomain,
    pub transport: TransportType,
}

impl fmt::Display for LastSenderInfoKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}",
            f.to_string(self.routing_domain),
            f.to_string(self.transport)
        )
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub(crate) struct LastFlowEntry {
    pub flow: Flow,
    pub timestamp: Timestamp,
}

impl fmt::Display for LastFlowEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} @ {}",
            f.to_string(self.flow),
            f.to_string(self.timestamp)
        )
    }
}

/// Bucket entry information specific to the LocalNetwork RoutingDomain
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BucketEntryPublicInternet {
    /// The PublicInternet node info
    peer_info: Option<Arc<PeerInfo>>,
    /// The last node info timestamp of ours that this entry has seen
    last_seen_our_node_info_ts: Timestamp,
    /// Last known node status
    node_status: Option<NodeStatus>,
}

impl fmt::Display for BucketEntryPublicInternet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(pi) = &self.peer_info {
            writeln!(f, "peer_info:")?;
            writeln!(f, "    {}", indent_string(f.to_string(&**pi)))?;
        } else {
            writeln!(f, "peer_info: None")?;
        }
        writeln!(
            f,
            "last_seen_our_node_info_ts: {}",
            f.to_string(self.last_seen_our_node_info_ts)
        )?;
        writeln!(f, "node_status: {:?}", self.node_status)?;
        Ok(())
    }
}

/// Bucket entry information specific to the LocalNetwork RoutingDomain
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BucketEntryLocalNetwork {
    /// The LocalNetwork peerinfo
    peer_info: Option<Arc<PeerInfo>>,
    /// The last node info timestamp of ours that this entry has seen
    last_seen_our_node_info_ts: Timestamp,
    /// Last known node status
    node_status: Option<NodeStatus>,
}

impl fmt::Display for BucketEntryLocalNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(pi) = &self.peer_info {
            writeln!(f, "peer_info:")?;
            writeln!(f, "    {}", indent_string(f.to_string(&**pi)))?;
        } else {
            writeln!(f, "peer_info: None")?;
        }
        writeln!(
            f,
            "last_seen_our_node_info_ts: {}",
            f.to_string(self.last_seen_our_node_info_ts)
        )?;
        writeln!(f, "node_status: {:?}", self.node_status)?;
        Ok(())
    }
}

/// The data associated with each bucket entry
///
/// Deserialized via a manual `DeserializeSeed` (see below) rather than `derive(Deserialize)`,
/// so the registry can be attached and `best_node_id` reconstructed (and validated) on load.
#[derive(Debug, Serialize)]
pub(crate) struct BucketEntryInner {
    /// Registry for logging
    #[serde(skip)]
    registry: VeilidComponentRegistry,
    /// The best (most-preferred valid) node id. Always present and kept in sync with
    /// `node_ids`, so `best_node_id()` is infallible. Reconstructed on load, not persisted.
    #[serde(skip)]
    best_node_id: NodeId,
    /// The node ids matching this bucket entry
    node_ids: NodeIdGroup,
    /// when the peer was added to the routing table
    time_added: Timestamp,
    /// The set of envelope versions supported by the node
    /// inclusive of the requirements of any relay the node may be using
    envelope_support: Vec<EnvelopeVersion>,
    /// If this node has updated its SignedNodeInfo since our network and dial info has last changed, for example when our IP address changes.
    /// Used to determine if we should make this entry 'live' again when we receive a signednodeinfo update that
    /// has the same timestamp, because if we change our own IP address or
    /// network class it may be possible for nodes that were unreachable may now
    /// be reachable with the same SignedNodeInfo/DialInfo
    updated_since_last_network_change: bool,
    /// The last flows used to contact this node, per protocol type
    #[serde(skip)]
    last_flows: BTreeMap<LastFlowKey, LastFlowEntry>,
    /// Last seen senderinfo per protocol/address type
    #[serde(skip)]
    last_sender_info: HashMap<LastSenderInfoKey, SenderInfo>,
    /// The node info for this entry on the publicinternet routing domain
    public_internet: BucketEntryPublicInternet,
    /// The node info for this entry on the localnetwork routing domain
    local_network: BucketEntryLocalNetwork,
    /// API-visible statistics gathered for the peer
    #[serde(default)]
    peer_stats: PeerStats,

    ////////////////////////////////////////////////////////////////////////
    // State Calculation Statistics
    ////////////////////////////////////////////////////////////////////////
    /// Node-level information about RPCs used to calculate state
    #[serde(default)]
    rpc_stats: RPCStats,
    /// Statistics gathered per SequenceOrdering used to calculate state
    #[serde(skip)]
    per_sequence_ordering_stats: BTreeMap<SequenceOrdering, RPCStats>,
    /// Per-transport reliability stats used to calculate state
    #[serde(skip)]
    per_transport_stats: BTreeMap<TransportType, RPCStats>,
    /// Per-local-route stats keyed by local-route pubkey (our outbound SR for outbound,
    /// our inbound reply-PR for inbound answers). Only populated for routed exchanges.
    #[serde(skip, default = "default_per_route_stats")]
    per_route_stats: LruCache<PublicKey, PerRouteStats>,
    /// Stats for connection-oriented protocols
    #[serde(skip)]
    connection_stats: ConnectionStats,
    /// If the entry is being punished and should be considered dead
    #[serde(skip)]
    punishment: Option<PunishmentReason>,
    /// Contact method failures cache for assisting with contact method selection
    #[serde(skip)]
    contact_method_failures: HashMap<ContactMethod, Timestamp>,

    /// Verbose stats: Recorded states for this peer over time for debugging purposes
    #[serde(skip)]
    state_stats: StateStats,
    /// Verbose stats: Answer stats for this node over time for debugging purposes
    #[serde(skip)]
    answer_stats: AnswerStats,

    ////////////////////////////////////////////////////////////////////////
    // Stats Accounting
    ////////////////////////////////////////////////////////////////////////
    /// The accounting for the latency statistics
    #[serde(skip)]
    latency_stats_accounting: LatencyStatsAccounting,
    /// The accounting for protected-connection drop durations
    #[serde(skip)]
    protected_drop_span_accounting: LatencyStatsAccounting,
    /// The accounting for the transfer statistics
    #[serde(skip)]
    transfer_stats_accounting: TransferStatsAccounting,
    /// The account for the state and reason statistics
    #[serde(skip)]
    state_stats_accounting: Mutex<StateStatsAccounting>,
    /// The accounting for the answer statistics
    #[serde(skip)]
    answer_stats_accounting: AnswerStatsAccounting,

    ////////////////////////////////////////////////////////////////////////
    // Geolocation Feature
    ////////////////////////////////////////////////////////////////////////
    /// Node location
    #[cfg(feature = "geolocation")]
    #[serde(skip)]
    geolocation_info: GeolocationInfo,
}

/// Deserialization context for `BucketEntryInner`: carries the registry to attach to the
/// deserialized entry, replacing the old `prepare` step.
pub(crate) struct BucketEntryInnerDeserializeSeed {
    pub registry: VeilidComponentRegistry,
}

impl BucketEntryInner {
    /// Deserialize a persisted entry, attaching `registry` and reconstructing the best node
    /// id. Errors if the entry has no valid node id.
    pub fn deserialize_from_persisted(
        registry: VeilidComponentRegistry,
        bytes: &[u8],
    ) -> EyreResult<BucketEntryInner> {
        use serde::de::DeserializeSeed;
        let seed = BucketEntryInnerDeserializeSeed { registry };
        let mut de = serde_json::Deserializer::from_slice(bytes);
        seed.deserialize(&mut de).map_err(|e| eyre!("{}", e))
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for BucketEntryInnerDeserializeSeed {
    type Value = BucketEntryInner;

    fn deserialize<D>(self, deserializer: D) -> Result<BucketEntryInner, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(BucketEntryInnerVisitor {
            registry: self.registry,
        })
    }
}

struct BucketEntryInnerVisitor {
    registry: VeilidComponentRegistry,
}

impl<'de> serde::de::Visitor<'de> for BucketEntryInnerVisitor {
    type Value = BucketEntryInner;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a BucketEntryInner map")
    }

    fn visit_map<A>(self, mut map: A) -> Result<BucketEntryInner, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        use serde::de::Error as _;

        let mut node_ids: Option<NodeIdGroup> = None;
        let mut time_added: Option<Timestamp> = None;
        let mut envelope_support: Option<Vec<EnvelopeVersion>> = None;
        let mut updated_since_last_network_change: Option<bool> = None;
        let mut public_internet: Option<BucketEntryPublicInternet> = None;
        let mut local_network: Option<BucketEntryLocalNetwork> = None;
        let mut peer_stats: Option<PeerStats> = None;
        let mut rpc_stats: Option<RPCStats> = None;

        // Only the persisted (non-skip) fields are recognized; unknown keys are ignored,
        // matching the previous derive.
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "node_ids" => {
                    if node_ids.is_some() {
                        return Err(A::Error::duplicate_field("node_ids"));
                    }
                    node_ids = Some(map.next_value()?);
                }
                "time_added" => {
                    if time_added.is_some() {
                        return Err(A::Error::duplicate_field("time_added"));
                    }
                    time_added = Some(map.next_value()?);
                }
                "envelope_support" => {
                    if envelope_support.is_some() {
                        return Err(A::Error::duplicate_field("envelope_support"));
                    }
                    envelope_support = Some(map.next_value()?);
                }
                "updated_since_last_network_change" => {
                    if updated_since_last_network_change.is_some() {
                        return Err(A::Error::duplicate_field(
                            "updated_since_last_network_change",
                        ));
                    }
                    updated_since_last_network_change = Some(map.next_value()?);
                }
                "public_internet" => {
                    if public_internet.is_some() {
                        return Err(A::Error::duplicate_field("public_internet"));
                    }
                    public_internet = Some(map.next_value()?);
                }
                "local_network" => {
                    if local_network.is_some() {
                        return Err(A::Error::duplicate_field("local_network"));
                    }
                    local_network = Some(map.next_value()?);
                }
                "peer_stats" => {
                    if peer_stats.is_some() {
                        return Err(A::Error::duplicate_field("peer_stats"));
                    }
                    peer_stats = Some(map.next_value()?);
                }
                "rpc_stats" => {
                    if rpc_stats.is_some() {
                        return Err(A::Error::duplicate_field("rpc_stats"));
                    }
                    rpc_stats = Some(map.next_value()?);
                }
                _ => {
                    let _ = map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }

        let node_ids = node_ids.ok_or_else(|| A::Error::missing_field("node_ids"))?;
        let time_added = time_added.ok_or_else(|| A::Error::missing_field("time_added"))?;
        let envelope_support =
            envelope_support.ok_or_else(|| A::Error::missing_field("envelope_support"))?;
        let updated_since_last_network_change = updated_since_last_network_change
            .ok_or_else(|| A::Error::missing_field("updated_since_last_network_change"))?;
        let public_internet =
            public_internet.ok_or_else(|| A::Error::missing_field("public_internet"))?;
        let local_network =
            local_network.ok_or_else(|| A::Error::missing_field("local_network"))?;
        // peer_stats and rpc_stats default when absent (matching their #[serde(default)]).
        let peer_stats = peer_stats.unwrap_or_default();
        let rpc_stats = rpc_stats.unwrap_or_default();

        // Reconstruct the best node id (most-preferred valid kind); fail if there is none.
        let best_node_id = node_ids
            .iter()
            .find(|nid| VALID_CRYPTO_KINDS.contains(&nid.kind()))
            .cloned()
            .ok_or_else(|| A::Error::custom("bucket entry has no valid node id"))?;

        Ok(BucketEntryInner {
            registry: self.registry,
            best_node_id,
            node_ids,
            time_added,
            envelope_support,
            updated_since_last_network_change,
            last_flows: BTreeMap::new(),
            last_sender_info: HashMap::new(),
            public_internet,
            local_network,
            peer_stats,
            rpc_stats,
            per_sequence_ordering_stats: BTreeMap::new(),
            per_transport_stats: BTreeMap::new(),
            per_route_stats: default_per_route_stats(),
            connection_stats: ConnectionStats::default(),
            punishment: None,
            contact_method_failures: HashMap::new(),
            state_stats: StateStats::default(),
            answer_stats: AnswerStats::default(),
            latency_stats_accounting: LatencyStatsAccounting::new(),
            protected_drop_span_accounting: LatencyStatsAccounting::new(),
            transfer_stats_accounting: TransferStatsAccounting::new(),
            state_stats_accounting: Mutex::new(StateStatsAccounting::new()),
            answer_stats_accounting: AnswerStatsAccounting::default(),
            #[cfg(feature = "geolocation")]
            geolocation_info: Default::default(),
        })
    }
}

impl fmt::Display for BucketEntryInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cur_ts = Timestamp::now();

        writeln!(f, "node_ids: {}", self.node_ids)?;
        writeln!(f, "envelope_support: {:?}", self.envelope_support)?;
        writeln!(
            f,
            "updated_since_last_network_change: {:?}",
            self.updated_since_last_network_change
        )?;
        writeln!(f, "last_flows:")?;
        for (key, entry) in &self.last_flows {
            writeln!(f, "    {}: {}", f.to_string(key), f.to_string(entry))?;
        }
        writeln!(f, "last_sender_info:")?;
        for (key, sender_info) in &self.last_sender_info {
            writeln!(
                f,
                "    {}: {}",
                f.to_string(key),
                sender_info.socket_address
            )?;
        }
        writeln!(f, "public_internet:")?;
        write!(
            f,
            "{}",
            indent_all_string(f.to_string(&self.public_internet))
        )?;
        writeln!(f, "local_network:")?;
        write!(f, "{}", indent_all_string(f.to_string(&self.local_network)))?;
        writeln!(f, "peer_stats:")?;
        write!(f, "{}", indent_all_string(f.to_string(&self.peer_stats)))?;
        writeln!(f, "rpc_stats:")?;
        write!(f, "{}", indent_all_string(f.to_string(&self.rpc_stats)))?;
        writeln!(f, "state_stats:")?;
        write!(f, "{}", indent_all_string(f.to_string(&self.state_stats)))?;
        writeln!(f, "answer_stats:")?;
        write!(f, "{}", indent_all_string(f.to_string(&self.answer_stats)))?;
        writeln!(f, "connection_stats:")?;
        write!(
            f,
            "{}",
            indent_all_string(f.to_string(&self.connection_stats))
        )?;
        writeln!(f, "per_sequence_ordering_stats:")?;
        for (key, entry) in &self.per_sequence_ordering_stats {
            writeln!(f, "  {}:", key)?;
            write!(f, "{}", indent_all_string(f.to_string(entry)))?;
        }
        writeln!(f, "per_transport_stats:")?;
        for (key, entry) in &self.per_transport_stats {
            writeln!(f, "  {}:", key)?;
            write!(f, "{}", indent_all_string(f.to_string(entry)))?;
        }
        // Per route stats
        if !self.per_route_stats.is_empty() {
            writeln!(f, "per_route_stats:")?;
            for (key, entry) in self.per_route_stats.iter() {
                writeln!(f, "  {}:", key)?;
                write!(
                    f,
                    "{}",
                    indent_all_string(indent_all_string(f.to_string(entry)))
                )?;
                writeln!(f)?;
            }
        }
        writeln!(f, "punishment: {}", f.to_string_opt(self.punishment))?;

        let mut contact_method_map = BTreeMap::<Timestamp, Vec<ContactMethod>>::new();
        for (cm, ts) in &self.contact_method_failures {
            contact_method_map.entry(*ts).or_default().push(cm.clone());
        }
        writeln!(
            f,
            "contact_method_failures:\n{}\n",
            indent_all_string(if contact_method_map.is_empty() {
                "None".to_string()
            } else {
                contact_method_map
                    .iter()
                    .map(|(ts, cms)| {
                        format!(
                            "{}: [{}]",
                            f.to_string(ts),
                            cms.iter()
                                .map(|cm| f.to_string(cm))
                                .collect::<Vec<String>>()
                                .join(", ")
                        )
                    })
                    .collect::<Vec<String>>()
                    .to_multiline_string()
            })
        )?;
        let state_reason = self.compute_state_reason(cur_ts);
        writeln!(f, "state_reason: {}", f.to_string(state_reason))?;

        Ok(())
    }
}

impl VeilidComponentRegistryAccessor for BucketEntryInner {
    fn registry(&self) -> VeilidComponentRegistry {
        self.registry.clone()
    }
}

fn default_per_route_stats() -> LruCache<PublicKey, PerRouteStats> {
    LruCache::new(PER_ROUTE_STATS_LRU_SIZE)
}

/// The valid node ids added to and removed from a bucket entry by `replace_node_ids`.
pub(crate) struct NodeIdsDelta {
    pub added: BTreeSet<NodeId>,
    pub removed: BTreeSet<NodeId>,
}

impl NodeIdsDelta {
    /// True if no valid node ids changed.
    pub fn is_unchanged(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

impl BucketEntryInner {
    /// Get time added
    pub fn time_added(&self) -> Timestamp {
        self.time_added
    }

    /// Get all node ids
    pub fn node_ids(&self) -> NodeIdGroup {
        self.node_ids.clone()
    }

    /// Get public keys
    pub fn public_keys(&self, routing_domain: RoutingDomain) -> PublicKeyGroup {
        match routing_domain {
            RoutingDomain::LocalNetwork => self
                .local_network
                .peer_info
                .as_ref()
                .map(|x| x.node_info().public_keys())
                .unwrap_or_default(),
            RoutingDomain::PublicInternet => self
                .public_internet
                .peer_info
                .as_ref()
                .map(|x| x.node_info().public_keys())
                .unwrap_or_default(),
        }
    }

    /// Get best node id
    ///
    /// Infallible: `best_node_id` is kept in sync with `node_ids` (seeded by `new`, updated
    /// by `replace_node_ids`, reconstructed on load) and is always a valid node id.
    pub fn best_node_id(&self) -> NodeId {
        self.best_node_id.clone()
    }

    /// Get best public key
    pub fn best_public_key(&self, routing_domain: RoutingDomain) -> Option<PublicKey> {
        match routing_domain {
            RoutingDomain::LocalNetwork => self
                .local_network
                .peer_info
                .as_ref()
                .and_then(|x| x.node_info().public_keys().first().cloned()),
            RoutingDomain::PublicInternet => self
                .public_internet
                .peer_info
                .as_ref()
                .and_then(|x| x.node_info().public_keys().first().cloned()),
        }
    }

    /// Atomically replace the entry's node ids with `new_ids` (one per crypto kind).
    ///
    /// An entry must always retain at least one valid (supported) node id, so this errors
    /// and leaves the entry unchanged if `new_ids` contains no valid node id. Returns the
    /// valid node ids added and removed so the caller can update bucket membership to match.
    pub fn replace_node_ids(&mut self, new_ids: &[NodeId]) -> EyreResult<NodeIdsDelta> {
        let new_group = NodeIdGroup::from(new_ids.to_vec());

        // Enforce the invariant: an entry must always have at least one valid node id, which
        // is also the new best node id (most-preferred valid kind, sorted first).
        let Some(best_node_id) = new_group
            .iter()
            .find(|nid| VALID_CRYPTO_KINDS.contains(&nid.kind()))
            .cloned()
        else {
            bail!("refusing to replace node ids with a set that has no valid node id");
        };

        // Compute the valid-id delta vs the current set (compared by exact value).
        let added: BTreeSet<NodeId> = new_group
            .iter()
            .filter(|nid| VALID_CRYPTO_KINDS.contains(&nid.kind()) && !self.node_ids.contains(nid))
            .cloned()
            .collect();
        let removed: BTreeSet<NodeId> = self
            .node_ids
            .iter()
            .filter(|nid| VALID_CRYPTO_KINDS.contains(&nid.kind()) && !new_group.contains(nid))
            .cloned()
            .collect();

        self.node_ids = new_group;
        self.best_node_id = best_node_id;
        Ok(NodeIdsDelta { added, removed })
    }

    /// All-of capability check
    pub fn has_all_capabilities(
        &self,
        routing_domain: RoutingDomain,
        capabilities: &[VeilidCapability],
    ) -> bool {
        let Some(ni) = self.node_info(routing_domain) else {
            return false;
        };
        ni.has_all_capabilities(capabilities)
    }

    /// Any-of capability check
    pub fn has_any_capabilities(
        &self,
        routing_domain: RoutingDomain,
        capabilities: &[VeilidCapability],
    ) -> bool {
        let Some(ni) = self.node_info(routing_domain) else {
            return false;
        };
        ni.has_any_capabilities(capabilities)
    }

    pub fn update_peer_info(
        &mut self,
        routing_domain: RoutingDomain,
        peer_info: Arc<PeerInfo>,
    ) -> bool {
        // Get the correct PeerInfo for the chosen routing domain
        let opt_current_pi = match routing_domain {
            RoutingDomain::LocalNetwork => &mut self.local_network.peer_info,
            RoutingDomain::PublicInternet => &mut self.public_internet.peer_info,
        };

        // See if we have an existing PeerInfo to update or not
        let mut node_info_changed = false;
        // let mut had_previous_node_info = false;
        if let Some(current_pi) = opt_current_pi {
            // had_previous_node_info = true;

            // Always allow overwriting unsigned node (bootstrap)
            if !current_pi.signatures().is_empty() {
                // Current peer info is signed so accept only newer peer info (may be unsigned if bootstrapping)

                // If the timestamp hasn't changed or is less, ignore this update
                if peer_info.node_info().timestamp() <= current_pi.node_info().timestamp() {
                    // If we received a node update with the same timestamp
                    // we can make this node live again, but only if our network has recently changed
                    // which may make nodes that were unreachable now reachable with the same dialinfo
                    if !self.updated_since_last_network_change
                        && peer_info.node_info().timestamp() == current_pi.node_info().timestamp()
                    {
                        // No need to update the signednodeinfo though since the timestamp is the same
                        // Let the node try to live again but don't mark it as seen yet
                        self.updated_since_last_network_change = true;
                        self.revive(Timestamp::now());
                    }
                    return false;
                }

                // See if anything has changed in this update beside the timestamp
                if !peer_info.equivalent(current_pi) {
                    node_info_changed = true;
                }
            }
        }

        // Update the envelope version support we have to use
        let envelope_support = peer_info.node_info().envelope_support().to_vec();

        // Update the signed node info
        // Let the node try to live again but don't mark it as seen yet
        *opt_current_pi = Some(peer_info.clone());
        self.set_envelope_support(envelope_support);
        self.updated_since_last_network_change = true;
        self.revive(Timestamp::now());

        // Update geolocation info
        #[cfg(feature = "geolocation")]
        {
            self.geolocation_info = peer_info.node_info().get_geolocation_info(routing_domain);
        }

        // If we're updating an entry's node info, purge all
        // but the last connection in our last connections list
        // because the dial info could have changed and it's safer to just reconnect.
        // The latest connection would have been the one we got the new node info
        // over so that connection is still valid.
        if node_info_changed {
            self.clear_last_flows_except_latest();
            self.contact_method_failures.clear();
        }

        node_info_changed
    }

    #[cfg(feature = "geolocation")]
    pub(super) fn update_geolocation_info(&mut self) {
        if let Some(ref peerinfo) = self.public_internet.peer_info {
            self.geolocation_info = peerinfo
                .node_info()
                .get_geolocation_info(RoutingDomain::PublicInternet);
        }
    }

    pub fn node_info(&self, routing_domain: RoutingDomain) -> Option<&NodeInfo> {
        let opt_peer_info = match routing_domain {
            RoutingDomain::LocalNetwork => &self.local_network.peer_info,
            RoutingDomain::PublicInternet => &self.public_internet.peer_info,
        };
        opt_peer_info.as_ref().map(|s| s.node_info())
    }

    pub fn get_peer_info(&self, routing_domain: RoutingDomain) -> Option<Arc<PeerInfo>> {
        let opt_current_pi = match routing_domain {
            RoutingDomain::LocalNetwork => &self.local_network.peer_info,
            RoutingDomain::PublicInternet => &self.public_internet.peer_info,
        };

        // Return the peerinfo
        opt_current_pi.clone()
    }

    pub fn best_routing_domain(
        &self,
        routing_table: &RoutingTable,
        routing_domain_set: RoutingDomainSet,
    ) -> Option<RoutingDomain> {
        // Check node info
        for routing_domain in routing_domain_set {
            let opt_current_pi = match routing_domain {
                RoutingDomain::LocalNetwork => &self.local_network.peer_info,
                RoutingDomain::PublicInternet => &self.public_internet.peer_info,
            };
            if opt_current_pi.is_some() {
                return Some(routing_domain);
            }
        }
        // Check connections
        let mut best_routing_domain: Option<RoutingDomain> = None;
        let last_connections =
            self.last_flows(routing_table, true, NodeRefFilter::from(routing_domain_set));
        for lc in last_connections {
            if let Some(rd) = routing_table.routing_domain_for_flow(lc.0) {
                if let Some(brd) = best_routing_domain {
                    if rd < brd {
                        best_routing_domain = Some(rd);
                    }
                } else {
                    best_routing_domain = Some(rd);
                }
            }
        }
        best_routing_domain
    }

    fn flow_to_key(&self, last_flow: Flow) -> LastFlowKey {
        LastFlowKey {
            transport: last_flow.transport_type(),
        }
    }

    // Stores a flow in this entry's table of last flows
    pub(super) fn set_last_flow(&mut self, last_flow: Flow, timestamp: Timestamp) {
        if self.punishment.is_some() {
            // Don't record connection if this entry is currently punished
            return;
        }
        let key = self.flow_to_key(last_flow);
        self.last_flows.insert(
            key,
            LastFlowEntry {
                flow: last_flow,
                timestamp,
            },
        );
    }

    // Removes a flow in this entry's table of last flows
    pub(super) fn remove_last_flow(&mut self, last_flow: Flow) {
        let key = self.flow_to_key(last_flow);
        self.last_flows.remove(&key);
    }

    // Clears the table of last flows to ensure we create new ones and drop any existing ones
    // With a DialInfo::all filter specified, only clear the flows that match the filter
    pub(super) fn clear_last_flows(&mut self, dial_info_filter: DialInfoFilter) {
        if dial_info_filter != DialInfoFilter::all() {
            self.last_flows
                .retain(|k, _v| !dial_info_filter.contains_transport(k.transport));
        } else {
            self.last_flows.clear();
        }
    }

    // Clears the table of last flows except the most recent one
    pub(super) fn clear_last_flows_except_latest(&mut self) {
        if self.last_flows.is_empty() {
            // No last_connections
            return;
        }
        let mut dead_keys = Vec::with_capacity(self.last_flows.len() - 1);
        let mut most_recent_flow = None;
        let mut most_recent_flow_time = 0u64;
        for (k, entry) in &self.last_flows {
            let lct = entry.timestamp.as_u64();
            if lct > most_recent_flow_time {
                most_recent_flow = Some(k);
                most_recent_flow_time = lct;
            }
        }
        let Some(most_recent_flow) = most_recent_flow else {
            return;
        };
        for k in self.last_flows.keys() {
            if k != most_recent_flow {
                dead_keys.push(k.clone());
            }
        }
        for dk in dead_keys {
            self.last_flows.remove(&dk);
        }
    }

    // Gets all the 'last flows' that match a particular filter, and their accompanying timestamps of last use
    pub(super) fn last_flows(
        &self,
        routing_table: &RoutingTable,
        only_live: bool,
        filter: NodeRefFilter,
    ) -> Vec<(Flow, Timestamp)> {
        let opt_connection_manager = routing_table.network_manager().opt_connection_manager();

        let mut out: Vec<(Flow, Timestamp)> = self
            .last_flows
            .iter()
            .filter_map(|(k, entry)| {
                let include = routing_table
                    .routing_domain_for_flow(entry.flow)
                    .map(|rd| {
                        filter.routing_domain_set().contains(rd)
                            && filter.contains_transport(k.transport)
                    })
                    .unwrap_or(false);

                if !include {
                    return None;
                }

                if !only_live {
                    return Some((entry.flow, entry.timestamp));
                }

                let alive = if matches!(
                    entry.flow.protocol_type().framing_type(),
                    FramingType::Connection
                ) {
                    // Connection-oriented: must still be in the connection table
                    opt_connection_manager
                        .as_ref()
                        .map(|cm| cm.get_connection(entry.flow).is_some())
                        .unwrap_or(false)
                } else {
                    // Connectionless: last-seen must be within the mapping timeout
                    let cur_ts = Timestamp::now();
                    entry.timestamp.later(CONNECTIONLESS_TIMEOUT) >= cur_ts
                };

                if alive {
                    Some((entry.flow, entry.timestamp))
                } else {
                    None
                }
            })
            .collect();
        // Sort with newest timestamps
        out.sort_unstable_by_key(|b| std::cmp::Reverse(b.1));
        out
    }

    pub(super) fn add_envelope_version(&mut self, envelope_version: EnvelopeVersion) {
        if !VALID_ENVELOPE_VERSIONS.contains(&envelope_version) {
            veilid_log!(self error "attempt to add invalid envelope version: {}", envelope_version);
            return;
        }
        if self.envelope_support.contains(&envelope_version) {
            return;
        }
        self.envelope_support.push(envelope_version);
        self.envelope_support.sort_unstable_by(|a, b| {
            let a_sort = VALID_ENVELOPE_VERSIONS
                .iter()
                .position(|x| x == a)
                .unwrap_or_log();
            let b_sort = VALID_ENVELOPE_VERSIONS
                .iter()
                .position(|x| x == b)
                .unwrap_or_log();
            a_sort.cmp(&b_sort)
        });
    }

    pub(super) fn set_envelope_support(&mut self, mut envelope_support: Vec<EnvelopeVersion>) {
        envelope_support.sort_unstable();
        envelope_support.dedup();
        self.envelope_support = envelope_support;
    }

    pub fn best_envelope_version(&self) -> Option<EnvelopeVersion> {
        self.envelope_support
            .iter()
            .find(|x| VALID_ENVELOPE_VERSIONS.contains(x))
            .copied()
    }

    /// Create a point-in-time snapshot of mutable fields for sort stability
    pub(super) fn make_snapshot(
        &self,
        registry: VeilidComponentRegistry,
        entry: Arc<BucketEntry>,
        cur_ts: Timestamp,
    ) -> BucketEntrySnapshot {
        let mut per_routing_domain = BTreeMap::new();
        if let Some(peer_info) = self.public_internet.peer_info.clone() {
            per_routing_domain.insert(
                RoutingDomain::PublicInternet,
                PerRoutingDomainSnapshot {
                    peer_info,
                    node_status: self.public_internet.node_status.clone(),
                    last_seen_our_node_info_ts: self.public_internet.last_seen_our_node_info_ts,
                },
            );
        }
        if let Some(peer_info) = self.local_network.peer_info.clone() {
            per_routing_domain.insert(
                RoutingDomain::LocalNetwork,
                PerRoutingDomainSnapshot {
                    peer_info,
                    node_status: self.local_network.node_status.clone(),
                    last_seen_our_node_info_ts: self.local_network.last_seen_our_node_info_ts,
                },
            );
        }
        let per_sequence_ordering = self.per_sequence_ordering_stats.clone();
        let per_transport = self.per_transport_stats.clone();

        let inner = BucketEntrySnapshotInner {
            cur_ts,
            node_ref: NodeRef::new(registry, entry),
            time_added: self.time_added,
            peer_stats: self.peer_stats.clone(),
            rpc_stats: self.rpc_stats.clone(),
            connection_stats: self.connection_stats.clone(),
            state: self.state(cur_ts),
            node_ids: self.node_ids.clone(),
            per_routing_domain,
            per_sequence_ordering,
            per_transport,
        };

        BucketEntrySnapshot::new(inner)
    }

    pub fn set_punished(&mut self, punished: Option<PunishmentReason>) {
        self.punishment = punished;
        if punished.is_some() {
            self.clear_last_flows(DialInfoFilter::all());
        }
    }

    pub fn peer_stats(&self) -> &PeerStats {
        &self.peer_stats
    }

    pub fn rpc_stats(&self) -> &RPCStats {
        &self.rpc_stats
    }

    pub fn connection_stats(&self) -> &ConnectionStats {
        &self.connection_stats
    }

    pub fn update_node_status(&mut self, routing_domain: RoutingDomain, status: NodeStatus) {
        match routing_domain {
            RoutingDomain::LocalNetwork => {
                self.local_network.node_status = Some(status);
            }
            RoutingDomain::PublicInternet => {
                self.public_internet.node_status = Some(status);
            }
        }
    }

    pub fn set_seen_our_node_info_ts(
        &mut self,
        routing_domain: RoutingDomain,
        seen_ts: Timestamp,
    ) -> Option<Timestamp> {
        match routing_domain {
            RoutingDomain::LocalNetwork => {
                let old_ts = self.local_network.last_seen_our_node_info_ts;
                if old_ts != seen_ts {
                    self.local_network.last_seen_our_node_info_ts = seen_ts;
                    Some(old_ts)
                } else {
                    None
                }
            }
            RoutingDomain::PublicInternet => {
                let old_ts = self.public_internet.last_seen_our_node_info_ts;
                if old_ts != seen_ts {
                    self.public_internet.last_seen_our_node_info_ts = seen_ts;
                    Some(old_ts)
                } else {
                    None
                }
            }
        }
    }

    pub fn has_seen_our_node_info_ts(
        &self,
        routing_domain: RoutingDomain,
        our_node_info_ts: Timestamp,
    ) -> bool {
        match routing_domain {
            RoutingDomain::LocalNetwork => {
                our_node_info_ts == self.local_network.last_seen_our_node_info_ts
            }
            RoutingDomain::PublicInternet => {
                our_node_info_ts == self.public_internet.last_seen_our_node_info_ts
            }
        }
    }

    pub fn reset_updated_since_last_network_change(&mut self) {
        self.updated_since_last_network_change = false;
    }

    ///// stats methods
    // called every ROLLING_TRANSFERS_INTERVAL_SECS seconds
    pub(super) fn roll_transfers(&mut self, last_ts: Timestamp, cur_ts: Timestamp) {
        self.transfer_stats_accounting.roll_transfers(
            last_ts,
            cur_ts,
            &mut self.peer_stats.transfer,
        );
        for (_key, prs) in self.per_route_stats.iter_mut() {
            prs.roll_transfers(last_ts, cur_ts);
        }
    }

    pub(super) fn record_routed_up(&mut self, key: PublicKey, bytes: ByteCount) {
        self.per_route_stats
            .entry(key)
            .or_insert_with(PerRouteStats::default)
            .add_up(bytes);
    }
    pub(super) fn record_routed_round_trip(
        &mut self,
        key: PublicKey,
        send_ts: Timestamp,
        recv_ts: Timestamp,
        bytes: ByteCount,
    ) {
        let entry = self
            .per_route_stats
            .entry(key)
            .or_insert_with(PerRouteStats::default);
        entry.record_round_trip(recv_ts.duration_since(send_ts));
        entry.add_down(bytes);
    }

    #[expect(
        dead_code,
        reason = "expose via routing_table debug command when needed"
    )]
    pub fn routed_stats(&self, key: &PublicKey) -> Option<PerRouteStats> {
        self.per_route_stats.peek(key).cloned()
    }

    // Called when a protected connection to this peer is closed by the remote
    // side. Longer durations = more reliable peer.
    pub(super) fn record_protected_connection_drop(&mut self, duration: TimestampDuration) {
        self.connection_stats.protected_drop_span =
            Some(self.protected_drop_span_accounting.record_latency(duration));
    }

    // Called every UPDATE_STATE_STATS_SECS seconds
    pub(super) fn update_state_stats(&mut self) {
        if let Some(state_stats) = self.state_stats_accounting.lock().take_stats() {
            self.state_stats = state_stats;
        }
    }

    // called every ROLLING_ANSWERS_INTERVAL_SECS seconds
    pub(super) fn roll_answer_stats(&mut self, cur_ts: Timestamp) {
        self.answer_stats = self.answer_stats_accounting.roll_answers(cur_ts);
    }

    fn transport_stats_mut(&mut self, transport: TransportType) -> &mut RPCStats {
        self.per_transport_stats.entry(transport).or_default()
    }

    fn sequence_ordering_stats_mut(
        &mut self,
        sequence_ordering: SequenceOrdering,
    ) -> &mut RPCStats {
        self.per_sequence_ordering_stats
            .entry(sequence_ordering)
            .or_default()
    }

    ////////////////////////////////////////////////////////////////
    // Called when rpc processor things happen

    pub(super) fn question_sent(
        &mut self,
        ts: Timestamp,
        bytes: ByteCount,
        expects_answer: bool,
        transport: TransportType,
    ) {
        // Update transport stats
        let ts_stats = self.transport_stats_mut(transport);
        ts_stats.question_sent(ts, expects_answer);

        // Update sequence ordering stats
        let sequence_ordering = transport.sequence_ordering();
        let so_stats = self.sequence_ordering_stats_mut(sequence_ordering);
        so_stats.question_sent(ts, expects_answer);

        // Update node-level rpc stats
        self.rpc_stats.question_sent(ts, expects_answer);

        // Update transfer accounting
        self.transfer_stats_accounting.add_up(bytes);
    }

    pub(super) fn question_rcvd(
        &mut self,
        ts: Timestamp,
        bytes: ByteCount,
        transport: TransportType,
    ) {
        // Update transport stats
        let ts_stats = self.transport_stats_mut(transport);
        ts_stats.question_rcvd(ts);

        // Update sequence ordering stats
        let sequence_ordering = transport.sequence_ordering();
        let so_stats = self.sequence_ordering_stats_mut(sequence_ordering);
        so_stats.question_rcvd(ts);

        // Update node-level rpc stats
        self.rpc_stats.question_rcvd(ts);

        // Update transfer accounting
        self.transfer_stats_accounting.add_down(bytes);
    }

    pub(super) fn answer_sent(&mut self, bytes: ByteCount, transport: TransportType) {
        // Update transport stats
        let ts_stats = self.transport_stats_mut(transport);
        ts_stats.answer_sent();

        // Update sequence ordering stats
        let sequence_ordering = transport.sequence_ordering();
        let so_stats = self.sequence_ordering_stats_mut(sequence_ordering);
        so_stats.answer_sent();

        // Update node-level rpc stats
        self.rpc_stats.answer_sent();

        // Update transfer accounting
        self.transfer_stats_accounting.add_up(bytes);
    }

    pub(super) fn answer_rcvd(
        &mut self,
        send_ts: Timestamp,
        recv_ts: Timestamp,
        bytes: ByteCount,
        transport: TransportType,
    ) {
        // Update transfer accounting
        self.transfer_stats_accounting.add_down(bytes);

        // Update latency accounting
        self.peer_stats.latency = Some(
            self.latency_stats_accounting
                .record_latency(recv_ts.duration_since(send_ts)),
        );

        // Update transport stats
        let ts_stats = self.transport_stats_mut(transport);
        ts_stats.answer_rcvd(recv_ts);

        // Update sequence ordering stats
        let sequence_ordering = transport.sequence_ordering();
        let so_stats = self.sequence_ordering_stats_mut(sequence_ordering);
        so_stats.answer_rcvd(recv_ts);

        // Update node-level rpc stats
        self.rpc_stats.answer_rcvd(recv_ts);
    }

    pub(super) fn lost_question(&mut self, transport: TransportType) {
        let lost_ts = Timestamp::now();

        // Update transport stats
        let ts_stats = self.transport_stats_mut(transport);
        ts_stats.lost_question(lost_ts);

        // Update sequence ordering stats
        let sequence_ordering = transport.sequence_ordering();
        let so_stats = self.sequence_ordering_stats_mut(sequence_ordering);
        so_stats.lost_question(lost_ts);

        // Update node-level rpc stats
        self.rpc_stats.lost_question(lost_ts);
    }

    pub(super) fn failed_to_send(
        &mut self,
        fail_ts: Timestamp,
        expects_answer: bool,
        transport: TransportType,
    ) {
        // Update transport stats
        let ts_stats = self.transport_stats_mut(transport);
        ts_stats.failed_to_send(fail_ts, expects_answer);

        // Update sequence ordering stats
        let sequence_ordering = transport.sequence_ordering();
        let so_stats = self.sequence_ordering_stats_mut(sequence_ordering);
        so_stats.failed_to_send(fail_ts, expects_answer);

        // Update node-level rpc stats
        self.rpc_stats.failed_to_send(fail_ts, expects_answer);
    }

    /// Send attempt aborted because no transport could be chosen (no routing domain,
    /// no peer info, no contact method). No transport was attempted so this doesn't
    /// feed per-transport or per-sequence-ordering stats.
    /// Not called for Punished, where we already chose not to talk.
    pub(super) fn unreachable(&mut self) {
        self.rpc_stats.unreachable();
    }

    pub(super) fn report_sender_info(
        &mut self,
        key: LastSenderInfoKey,
        sender_info: SenderInfo,
    ) -> Option<SenderInfo> {
        let last_sender_info = self.last_sender_info.insert(key, sender_info);
        if last_sender_info != Some(sender_info) {
            // Return last senderinfo if this new one is different
            last_sender_info
        } else {
            None
        }
    }

    pub fn report_contact_method_result(&mut self, cm: &ContactMethod, success: bool) {
        if success {
            self.contact_method_failures.remove(cm);
        } else {
            let now = Timestamp::now();
            self.contact_method_failures.insert(cm.clone(), now);
        }
    }

    pub fn get_contact_method_failure_ts(&self, cm: &ContactMethod) -> Option<Timestamp> {
        self.contact_method_failures.get(cm).copied()
    }

    ////////////////////////////////////////////////////////////////////////
    // Geolocation
    ////////////////////////////////////////////////////////////////////////

    #[cfg(feature = "geolocation")]
    pub fn geolocation_info(&self) -> &GeolocationInfo {
        &self.geolocation_info
    }
}

pub(crate) struct BucketEntry {
    pub(super) ref_count: AtomicU32,
    inner: RwLock<BucketEntryInner>,
    // NodeRef tracking lives outside `inner` so track()/untrack() never re-lock it
    // (NodeRefs are constructed/dropped while `inner` is held, e.g. during snapshot).
    // The registry copy lets the leak report log without locking `inner`.
    #[cfg(all(feature = "tracking", feature = "backtrace"))]
    registry: VeilidComponentRegistry,
    #[cfg(feature = "tracking")]
    next_track_id: AtomicUsize,
    #[cfg(all(feature = "tracking", feature = "backtrace"))]
    pub(super) node_ref_tracks: Mutex<HashMap<usize, backtrace::Backtrace>>,
}

impl fmt::Debug for BucketEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BucketEntry")
            .field("ref_count", &self.ref_count)
            .field("inner", &self.inner)
            .finish()
    }
}

impl BucketEntry {
    pub(super) fn new(registry: VeilidComponentRegistry, first_node_id: NodeId) -> Self {
        // First node id should always be one we support since TypedKeySets are sorted and we must have at least one supported key
        debug_assert!(VALID_CRYPTO_KINDS.contains(&first_node_id.kind()));

        let now = Timestamp::now();
        let inner = BucketEntryInner {
            registry,
            best_node_id: first_node_id.clone(),
            node_ids: NodeIdGroup::from(first_node_id),
            time_added: now,
            envelope_support: Vec::new(),
            updated_since_last_network_change: false,
            last_flows: BTreeMap::new(),
            last_sender_info: HashMap::new(),
            local_network: BucketEntryLocalNetwork {
                last_seen_our_node_info_ts: Timestamp::new(0u64),
                peer_info: None,
                node_status: None,
            },
            public_internet: BucketEntryPublicInternet {
                last_seen_our_node_info_ts: Timestamp::new(0u64),
                peer_info: None,
                node_status: None,
            },
            #[cfg(feature = "geolocation")]
            geolocation_info: Default::default(),
            peer_stats: PeerStats {
                latency: None,
                transfer: TransferStatsDownUp::default(),
            },
            rpc_stats: RPCStats::default(),
            state_stats: StateStats::default(),
            answer_stats: AnswerStats::default(),
            connection_stats: ConnectionStats::default(),
            per_sequence_ordering_stats: BTreeMap::new(),
            per_transport_stats: BTreeMap::new(),
            per_route_stats: LruCache::new(PER_ROUTE_STATS_LRU_SIZE),
            punishment: None,
            contact_method_failures: HashMap::new(),
            latency_stats_accounting: LatencyStatsAccounting::new(),
            protected_drop_span_accounting: LatencyStatsAccounting::new(),
            transfer_stats_accounting: TransferStatsAccounting::new(),
            state_stats_accounting: Mutex::new(StateStatsAccounting::new()),
            answer_stats_accounting: AnswerStatsAccounting::default(),
        };

        Self::new_with_inner(inner)
    }

    pub(super) fn new_with_inner(inner: BucketEntryInner) -> Self {
        Self {
            ref_count: AtomicU32::new(0),
            #[cfg(all(feature = "tracking", feature = "backtrace"))]
            registry: inner.registry(),
            inner: RwLock::new(inner),
            #[cfg(feature = "tracking")]
            next_track_id: AtomicUsize::new(0),
            #[cfg(all(feature = "tracking", feature = "backtrace"))]
            node_ref_tracks: Mutex::new(HashMap::new()),
        }
    }

    // NodeRef tracking uses interior mutability and must NOT lock `inner`, since it is
    // called while `inner` is already held (NodeRef construction during snapshot, etc.)
    #[cfg(feature = "tracking")]
    pub fn track(&self) -> usize {
        let track_id = self.next_track_id.fetch_add(1, Ordering::AcqRel);
        #[cfg(feature = "backtrace")]
        self.node_ref_tracks
            .lock()
            .insert(track_id, backtrace::Backtrace::new_unresolved());
        track_id
    }

    #[cfg(feature = "tracking")]
    pub fn untrack(&self, _track_id: usize) {
        #[cfg(feature = "backtrace")]
        self.node_ref_tracks.lock().remove(&_track_id);
    }

    /// Create a point-in-time snapshot of mutable fields for sort stability
    pub fn snapshot(
        self: &Arc<Self>,
        registry: VeilidComponentRegistry,
        cur_ts: Timestamp,
    ) -> BucketEntrySnapshot {
        let inner = self.inner.read();
        inner.make_snapshot(registry, self.clone(), cur_ts)
    }

    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&BucketEntryInner) -> R,
    {
        let inner = self.inner.read();
        f(&inner)
    }

    pub fn with_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut BucketEntryInner) -> R,
    {
        let mut inner = self.inner.write();
        f(&mut inner)
    }
}

impl Drop for BucketEntry {
    fn drop(&mut self) {
        if self.ref_count.load(Ordering::Acquire) != 0 {
            #[cfg(all(feature = "tracking", feature = "backtrace"))]
            {
                let registry = &self.registry;
                veilid_log!(registry info "NodeRef Tracking");
                for (id, bt) in self.node_ref_tracks.lock().iter() {
                    let mut bt = bt.clone();
                    bt.resolve();
                    veilid_log!(registry info "Id: {}\n----------------\n{:#?}", id, bt);
                }
            }

            #[cfg(debug_assertions)]
            panic!(
                "bucket entry dropped with non-zero refcount: {:#?}",
                &*self.inner.read()
            );
            #[cfg(not(debug_assertions))]
            {
                let inner = self.inner.read();
                let registry = inner.registry();
                veilid_log!(registry error "bucket entry dropped with non-zero refcount: {:#?}", &*inner);
            }
        }
    }
}
