use super::*;

impl_veilid_log_facility!("rtab");

/// Minimum number of nodes we need, per crypto kind or we trigger a bootstrap
const MIN_BOOTSTRAP_CONNECTIVITY_PEERS: usize = 4;
/// How long we tolerate zero responsive nodes before falling back to bootstrap
const NO_RESPONSIVE_NODES_BOOTSTRAP_DELAY: TimestampDuration = TimestampDuration::new_secs(10);
/// Fraction of the connectivity nodes we need to have tested to allocate routes
const ALLOCATION_CONNECTIVITY_NODES_TESTED_FACTOR: f32 = 0.25;
/// Fraction of the low water mark nodes we need to have tested to allocate routes
const ALLOCATION_LOW_WATER_MARK_NODES_TESTED_FACTOR: f32 = 0.5;
/// Cap on required tested nodes so a large stale table can't inflate the readiness bar
const MAX_REQUIRED_TESTED_CONNECTIVITY_NODES: usize = 12;

/// Description of which nodes this routing domain needs per crypto kind to operate properly
pub struct NodesNeeded {
    pub needs_bootstrap: Vec<CryptoKind>,
    pub needs_peer_minimum_refresh: Vec<CryptoKind>,
    pub needs_more_tested_nodes: Vec<CryptoKind>,
}

/// Connectivity-capable entry counts for one crypto kind
#[derive(Debug, Clone, Copy, Default)]
pub struct KindNodeCounts {
    pub maybe_live: usize,
    pub responsive: usize,
    pub live_external: usize,
    pub low_water_mark: usize,
}

/// Node requirements for one crypto kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindNodesNeeded {
    pub needs_bootstrap: bool,
    pub needs_peer_minimum_refresh: bool,
    pub needs_more_tested_nodes: bool,
}

/// Decide what one crypto kind needs from its entry counts
pub fn nodes_needed_for_counts(
    counts: KindNodeCounts,
    min_peer_count: usize,
    no_responsive_elapsed: Option<TimestampDuration>,
) -> KindNodesNeeded {
    // Bootstrap when almost nothing is known, or when nothing has responded for too long
    let needs_bootstrap = counts.maybe_live < MIN_BOOTSTRAP_CONNECTIVITY_PEERS
        || no_responsive_elapsed.is_some_and(|d| d >= NO_RESPONSIVE_NODES_BOOTSTRAP_DELAY);
    // Require a fraction of known nodes tested, capped so stale entries can't raise the bar
    let required_tested_connectivity_nodes = usize::max(
        (counts.live_external as f32 * ALLOCATION_CONNECTIVITY_NODES_TESTED_FACTOR) as usize,
        (counts.low_water_mark as f32 * ALLOCATION_LOW_WATER_MARK_NODES_TESTED_FACTOR) as usize,
    )
    .min(MAX_REQUIRED_TESTED_CONNECTIVITY_NODES);
    let needs_more_tested_nodes = counts.responsive < required_tested_connectivity_nodes;
    // Gather nodes network-wide while below the known-node minimum or still proving the
    // tested bar; quiet once ready so refresh fanouts don't compete with operations
    let needs_peer_minimum_refresh =
        !needs_bootstrap && (counts.maybe_live < min_peer_count || needs_more_tested_nodes);

    KindNodesNeeded {
        needs_bootstrap,
        needs_peer_minimum_refresh,
        needs_more_tested_nodes,
    }
}

impl PublicInternetRoutingDomainController {
    pub fn state(&self) -> RoutingDomainState {
        let (
            relay_requirements,
            opt_relay_compilation,
            current_peer_info,
            confirmed,
            address_types,
            outbound_protocols,
            entry_summary,
            low_water_mark,
            relays,
        ) = {
            let detail = self.read_dyn();
            let relay_requirements = detail.relay_requirements();
            let opt_relay_compilation = detail.relay_compilation();
            let current_peer_info = detail.get_peer_info();
            let confirmed = detail.confirmed();
            let address_types = detail.address_types();
            let outbound_protocols = detail.outbound_protocols();
            let entry_summary = detail.get_entry_summary();
            let low_water_mark = detail.get_low_water_mark();
            let relays = detail.relays();

            (
                relay_requirements,
                opt_relay_compilation,
                current_peer_info,
                confirmed,
                address_types,
                outbound_protocols,
                entry_summary,
                low_water_mark,
                relays,
            )
        };

        // Determine inbound stage
        let inbound_stage = {
            if !confirmed {
                if address_types.is_empty() || outbound_protocols.is_empty() {
                    RoutingDomainInboundStage::Invalid
                } else {
                    RoutingDomainInboundStage::NeedsDialInfoConfirmation
                }
            } else if address_types.is_empty() || outbound_protocols.is_empty() {
                RoutingDomainInboundStage::Unusable
            } else if relay_requirements.needs_relays() == relays.is_empty() {
                // If relays are needed, they are all allocated at the same time, so we either have all the relays
                // we need, or we don't have any at all
                // If relays are not needed, but we have some, we are also in the 'needsrelays' state so we can get
                // rid of the unnecessary relays before we publish
                RoutingDomainInboundStage::NeedsRelays
            } else {
                RoutingDomainInboundStage::ReadyToPublish
            }
        };

        // Determine outbound stage
        let outbound_stage = {
            if address_types.is_empty()
                || outbound_protocols.is_empty()
                || !self.network_manager().network_is_started()
            {
                RoutingDomainOutboundStage::Invalid
            } else {
                // Figure out if we need more nodes
                let nodes_needed = self.nodes_needed();
                if !nodes_needed.needs_bootstrap.is_empty() {
                    RoutingDomainOutboundStage::NeedsBootstrap
                } else if self.get_published_peer_info().is_none() {
                    RoutingDomainOutboundStage::NeedsPublishedPeerInfo
                } else if !nodes_needed.needs_more_tested_nodes.is_empty() {
                    RoutingDomainOutboundStage::NeedsMoreTestedNodes
                } else if !self.safety_routes_ready() {
                    RoutingDomainOutboundStage::NeedsSafetyRoutes
                } else {
                    RoutingDomainOutboundStage::ReadyToOperate
                }
            }
        };

        // Inbound is ready if our current stage is ReadyToPublish and we have actually published peer info
        let is_ready_inbound = matches!(inbound_stage, RoutingDomainInboundStage::ReadyToPublish)
            && self.get_published_peer_info().is_some();

        // Outbound is ready if are in the 'ready to operate' stage
        let is_ready_outbound =
            matches!(outbound_stage, RoutingDomainOutboundStage::ReadyToOperate);

        RoutingDomainState {
            inbound_stage,
            outbound_stage,
            relay_requirements,
            opt_relay_compilation,
            current_peer_info,
            entry_summary,
            low_water_mark,
            is_ready_inbound,
            is_ready_outbound,
        }
    }

    /// Figure out if we need more nodes for this routing domain
    pub(in crate::routing_table) fn nodes_needed(&self) -> NodesNeeded {
        // Nodes needed summary is based off the entry summary and the low water mark
        let (entry_summary, low_water_mark) = {
            let rdd = self.read_dyn();
            (rdd.get_entry_summary(), rdd.get_low_water_mark())
        };

        // Calculate which crypto kinds we need more nodes for
        let mut needs_bootstrap = Vec::new();
        let mut needs_peer_minimum_refresh = Vec::new();
        let mut needs_more_tested_nodes = Vec::new();

        let min_peer_count = self.config().internal().network.dht.min_peer_count as usize;
        let cur_ts = Timestamp::now();
        let mut no_responsive_since = self.no_responsive_since.lock();
        for ck in VALID_CRYPTO_KINDS {
            // Use live_external (excludes our NAT siblings) for the tested-nodes basis
            // because tested nodes are only useful if they can serve as safety-route hops.
            let kind_summary = entry_summary.per_crypto_kind.get(&ck);
            let counts = KindNodeCounts {
                maybe_live: kind_summary
                    .map(|x| x.maybe_live.connectivity_capabilities)
                    .unwrap_or_default(),
                responsive: kind_summary
                    .map(|x| x.responsive.connectivity_capabilities)
                    .unwrap_or_default(),
                live_external: kind_summary
                    .map(|x| x.live_external.connectivity_capabilities)
                    .unwrap_or_default(),
                low_water_mark: low_water_mark
                    .per_crypto_kind
                    .get(&ck)
                    .map(|x| x.connectivity_capabilities)
                    .unwrap_or_default(),
            };

            // Track how long this kind has had zero responsive nodes
            let no_responsive_elapsed = if counts.responsive == 0 {
                let since = *no_responsive_since.entry(ck).or_insert(cur_ts);
                Some(cur_ts.duration_since(since))
            } else {
                no_responsive_since.remove(&ck);
                None
            };

            let needed = nodes_needed_for_counts(counts, min_peer_count, no_responsive_elapsed);
            if needed.needs_bootstrap {
                needs_bootstrap.push(ck);
            }
            if needed.needs_peer_minimum_refresh {
                needs_peer_minimum_refresh.push(ck);
            }
            if needed.needs_more_tested_nodes {
                needs_more_tested_nodes.push(ck);
            }
        }
        drop(no_responsive_since);

        NodesNeeded {
            needs_bootstrap,
            needs_peer_minimum_refresh,
            needs_more_tested_nodes,
        }
    }
}
