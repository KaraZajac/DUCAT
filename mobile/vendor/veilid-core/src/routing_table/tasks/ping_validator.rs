use super::*;

use futures_util::future::{select, Either};
use futures_util::stream::{FuturesUnordered, StreamExt};
use stop_token::future::FutureExt as _;

impl_veilid_log_facility!("rtab");

/////////////////////////////////////
// Priority Groups

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PingValidationGroup {
    /// `Keepalive` pings as Status RPCs sent to keep flows alive:
    /// - Loopbacks with `Destination::PrivateRoute` for keeping
    ///   allocated safety routes alive independent of destination
    /// - Relay keepalives with `Destination::DialInfo` to ensure the
    ///   protected relay flow is used
    /// - Active watch keepalives with `Destination::Direct` + `SafetySelection::Safe`
    ///   for DHT caching nodes we are watching
    Keepalive = 0,
    /// `RouteTest` pings are Status RPCs always sent via safety routes:
    /// - Nodes with `Destination::Direct` + `SafetySelection::Safe` for testing
    ///   safety route deliverability to a specific destination
    /// - Private routes with `Destination::PrivateRoute` + `SafetySelection::Safe` for
    ///   testing imported remote routes
    RouteTest = 1,
    /// `Reliability` pings are Status RPCs sent to routing table nodes:
    /// - Unsafe routing table pings with `Destination::Direct` + `SafetySelection::Unsafe` to
    ///   assess node timing, reachability, and reliability
    Reliability = 2,
}

impl PingValidationGroup {
    /// The maximum number of pings that can be in-flight for this group
    fn max_parallel(&self) -> usize {
        match self {
            Self::RouteTest => 4,
            Self::Keepalive => 4,
            Self::Reliability => 32,
        }
    }
}

/////////////////////////////////////
// Ping Validation Entry Types

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PingValidationKey {
    Destination(Destination),
    RouteTest {
        route_id: RouteId,
        ping_index: usize,
    },
}
impl fmt::Display for PingValidationKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Destination(dest) => write!(f, "Ping({})", f.to_string(dest)),
            Self::RouteTest {
                route_id,
                ping_index,
            } => write!(f, "Route({} #{})", f.to_string(route_id), ping_index),
        }
    }
}

/// Callback invoked when a ping completes.
/// Receives the registry and the StatusResult (which carries the SendDataResult so
/// the callback can see which transport was actually attempted at the first hop).
/// Returns follow-up entries to re-enqueue (e.g., next ping in a route test chain).
pub type PingCompletionCallback = Box<
    dyn FnOnce(VeilidComponentRegistry, Result<StatusResult, RPCError>) -> Vec<PingValidationEntry>
        + Send
        + 'static,
>;

pub struct PingValidationEntry {
    pub group: PingValidationGroup,
    pub priority: usize, // 0 = highest
    pub key: PingValidationKey,
    pub dest: Destination,
    pub on_complete: Option<PingCompletionCallback>,
}

/// A batch of ping validations to enqueue, labeled by purpose for logging
pub(crate) struct PingValidationBatch {
    pub purpose: String,
    pub entries: Vec<PingValidationEntry>,
}

/////////////////////////////////////
// Processor State (internal, not shared)

struct PingCompletionResult {
    group: PingValidationGroup,
    key: PingValidationKey,
    result: Result<StatusResult, RPCError>,
    on_complete: Option<PingCompletionCallback>,
}

#[derive(Default)]
struct PingValidationProcessorState {
    /// Per-group queued items, sorted by priority (BTreeMap key = priority number, 0 = highest)
    groups: BTreeMap<PingValidationGroup, BTreeMap<usize, VecDeque<PingValidationEntry>>>,
    /// Per-group in-flight counts
    group_in_flight: BTreeMap<PingValidationGroup, usize>,
    /// Keys currently queued (for deduplication)
    queued_keys: HashSet<PingValidationKey>,
    /// Keys currently in-flight (for deduplication)
    in_process_keys: HashSet<PingValidationKey>,
}

impl PingValidationProcessorState {
    fn new() -> Self {
        Self::default()
    }

    /// Add an entry to the priority queue, deduplicating against queued and in-process keys
    fn enqueue(&mut self, entry: PingValidationEntry) -> bool {
        if self.in_process_keys.contains(&entry.key) || self.queued_keys.contains(&entry.key) {
            return false;
        }
        self.queued_keys.insert(entry.key.clone());
        self.groups
            .entry(entry.group)
            .or_default()
            .entry(entry.priority)
            .or_default()
            .push_back(entry);
        true
    }

    /// Enqueue a batch, returning how many were newly added (not deduplicated away)
    fn enqueue_all(&mut self, entries: Vec<PingValidationEntry>) -> Vec<PingValidationKey> {
        let mut added = vec![];
        for entry in entries {
            let key = entry.key.clone();
            if self.enqueue(entry) {
                added.push(key);
            }
        }
        added
    }

    /// Start as much pending work as possible, respecting per-group parallelism limits
    fn start_pending_work(
        &mut self,
        registry: &VeilidComponentRegistry,
        in_flight: &mut FuturesUnordered<PinBoxFutureStatic<PingCompletionResult>>,
    ) {
        // Iterate groups in order (RouteTest=0 first, then Keepalive=1, then Reliability=2)
        let groups: Vec<PingValidationGroup> = self.groups.keys().copied().collect();
        for group in groups {
            let max_parallel = group.max_parallel();
            let current_in_flight = *self.group_in_flight.entry(group).or_insert(0);
            let available = max_parallel.saturating_sub(current_in_flight);
            if available == 0 {
                continue;
            }

            let Some(priority_map) = self.groups.get_mut(&group) else {
                continue;
            };

            let mut started = 0usize;
            // Take items from lowest priority number first (BTreeMap iterates in ascending order)
            let priorities: Vec<usize> = priority_map.keys().copied().collect();
            for priority in priorities {
                if started >= available {
                    break;
                }
                let Some(queue) = priority_map.get_mut(&priority) else {
                    continue;
                };
                while started < available {
                    let Some(entry) = queue.pop_front() else {
                        break;
                    };
                    self.queued_keys.remove(&entry.key);
                    self.in_process_keys.insert(entry.key.clone());

                    let fut_group = entry.group;
                    let fut_key = entry.key.clone();
                    let fut_dest = entry.dest.clone();
                    let fut_on_complete = entry.on_complete;
                    let fut_registry = registry.clone();

                    in_flight.push(Box::pin(async move {
                        #[cfg(feature = "verbose-tracing")]
                        veilid_log!(fut_registry debug "--> validator ping ({:?}) to {:?}", fut_group, fut_dest);

                        let rpc_processor = fut_registry.rpc_processor();
                        let result =
                            Box::pin(rpc_processor.rpc_call_status(fut_dest)).await;

                        PingCompletionResult {
                            group: fut_group,
                            key: fut_key,
                            result,
                            on_complete: fut_on_complete,
                        }
                    }));

                    started += 1;
                }
                // Clean up empty queues
                if queue.is_empty() {
                    priority_map.remove(&priority);
                }
            }

            // Clean up empty priority maps
            if priority_map.is_empty() {
                self.groups.remove(&group);
            }

            *self.group_in_flight.entry(group).or_insert(0) += started;
        }
    }

    /// Handle a completed ping: update tracking, invoke callback, re-enqueue follow-ups
    fn handle_completion(
        &mut self,
        result: PingCompletionResult,
        registry: &VeilidComponentRegistry,
    ) {
        // Update tracking
        self.in_process_keys.remove(&result.key);
        if let Some(count) = self.group_in_flight.get_mut(&result.group) {
            *count = count.saturating_sub(1);
        }

        // Invoke callback and enqueue follow-up entries
        if let Some(callback) = result.on_complete {
            let follow_ups = callback(registry.clone(), result.result);
            for entry in follow_ups {
                self.enqueue(entry);
            }
        }
    }

    /// Returns the total number of queued items
    fn queued_count(&self) -> usize {
        self.groups
            .values()
            .flat_map(|pm| pm.values())
            .map(|q| q.len())
            .sum()
    }

    /// Returns the total number of in-flight items
    fn in_flight_count(&self) -> usize {
        self.group_in_flight.values().sum()
    }
}

/////////////////////////////////////
// Background Processor

impl RoutingTable {
    /// Long-lived background processor for the ping validation queue.
    /// Spawned at startup, processes items immediately as they are enqueued.
    pub(crate) async fn ping_validation_processor(
        registry: VeilidComponentRegistry,
        stop_token: StopToken,
        rx: flume::Receiver<PingValidationBatch>,
    ) {
        let mut state = PingValidationProcessorState::new();
        let mut in_flight = FuturesUnordered::<PinBoxFutureStatic<PingCompletionResult>>::new();

        loop {
            // Start as much pending work as possible from the priority queue
            state.start_pending_work(&registry, &mut in_flight);

            if in_flight.is_empty() {
                // Nothing in-flight: block on channel recv
                match rx.recv_async().timeout_at(stop_token.clone()).await {
                    Ok(Ok(batch)) => {
                        let added = state.enqueue_all(batch.entries);
                        if !added.is_empty() && debug_target_enabled!("rtab::state::ping") {
                            veilid_log!(registry debug target: "rtab::state::ping", "Enqueued {} pings for {}:\n{}", added.len(), batch.purpose, indent_all_string(added.to_multiline_string()));
                        }
                    }
                    _ => break, // channel closed or stop token
                }
            } else {
                // Stuff in-flight: race channel recv vs future completion,
                // but also respect the stop token so shutdown isn't blocked
                // waiting for in-flight RPC timeouts.
                let recv_fut = rx.recv_async();
                let next_fut = in_flight.next();

                match select(recv_fut, next_fut)
                    .timeout_at(stop_token.clone())
                    .await
                {
                    Ok(Either::Left((Ok(batch), _))) => {
                        // New entries received from channel
                        let added = state.enqueue_all(batch.entries);
                        if !added.is_empty() && debug_target_enabled!("rtab::state::ping") {
                            veilid_log!(registry debug target: "rtab::state::ping", "Enqueued {} pings for {}:\n{}", added.len(), batch.purpose, indent_all_string(added.to_multiline_string()));
                        }
                    }
                    Ok(Either::Left((Err(_), _))) => {
                        // Channel closed, abandon in-flight items
                        break;
                    }
                    Ok(Either::Right((Some(result), _))) => {
                        // A ping completed
                        state.handle_completion(result, &registry);
                    }
                    Ok(Either::Right((None, _))) => {
                        // FuturesUnordered empty (shouldn't happen since we checked)
                        continue;
                    }
                    Err(_) => {
                        // Stop token fired, shut down immediately
                        break;
                    }
                }
            }
        }

        veilid_log!(registry debug "Ping validation processor stopped. {} queued, {} in-flight remaining.",
            state.queued_count(), state.in_flight_count());
    }

    /////////////////////////////////////
    // Enqueue APIs

    /// Enqueue ping validation entries with full control (callbacks, groups, priorities).
    /// `purpose` labels the batch for the "Enqueued N pings" processor-side log.
    pub fn enqueue_ping_validation_entries(
        &self,
        purpose: String,
        entries: Vec<PingValidationEntry>,
    ) {
        if entries.is_empty() {
            return;
        }
        if let Err(e) = self
            .ping_validation_sender
            .lock()
            .send(PingValidationBatch { purpose, entries })
        {
            veilid_log!(self warn "Failed to enqueue ping validations: channel closed ({} entries dropped)", e.into_inner().entries.len());
        }
    }

    /// Convenience wrapper for simple destination pings (Destination key, no callback)
    pub fn enqueue_ping_validations(
        &self,
        purpose: String,
        group: PingValidationGroup,
        priority: usize,
        ping_validations: Vec<Destination>,
    ) {
        if ping_validations.is_empty() {
            return;
        }
        let entries: Vec<PingValidationEntry> = ping_validations
            .into_iter()
            .map(|dest| PingValidationEntry {
                group,
                priority,
                key: PingValidationKey::Destination(dest.clone()),
                dest,
                on_complete: None,
            })
            .collect();
        self.enqueue_ping_validation_entries(purpose, entries);
    }

    /////////////////////////////////////
    // Reliability Ping Validations

    /// Ping each node in the routing table if they need to be pinged to determine their reliability.
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err))]
    pub fn add_reliability_ping_validations(
        &self,
        cur_ts: Timestamp,
        routing_domain: RoutingDomain,
    ) -> EyreResult<()> {
        let priority_nodes = self.get_relability_ping_nodes(routing_domain, cur_ts);

        let name = format!("Reliability({:#})", routing_domain);
        for (priority, node_refs) in priority_nodes {
            let validations = node_refs
                .into_iter()
                .map(|nr| Destination::direct(nr, None))
                .collect();
            self.enqueue_ping_validations(
                name.clone(),
                PingValidationGroup::Reliability,
                priority,
                validations,
            );
        }

        Ok(())
    }

    /////////////////////////////////////
    // Route Test Integration

    /// Enqueue route test pings for a set of routes.
    /// Allocated routes: one loopback per SequenceOrdering the route supports.
    /// Remote routes: a single ping.
    pub async fn enqueue_route_tests(
        &self,
        cur_ts: Timestamp,
        route_ids: Vec<RouteId>,
        priority: usize,
    ) {
        let rss = self.route_spec_store();

        let mut entries = Vec::new();
        for route_id in route_ids {
            let is_remote = rss.is_route_id_remote(&route_id);

            if is_remote {
                let rrid = RemoteRouteSetId::from_route_id(route_id.clone());
                match Self::build_remote_route_test_entry(rss, &rrid, priority) {
                    Some(entry) => entries.push(entry),
                    None => {
                        veilid_log!(self debug "Could not build remote route test for {:#}", route_id);
                    }
                }
            } else {
                let arsid = AllocatedRouteSetId::from_route_id(route_id);
                let Some(info) = rss.get_allocated_route_test_info(&arsid) else {
                    veilid_log!(self debug "Could not build allocated route test for {:#}", arsid);
                    continue;
                };
                let req = RoutePingValidationRequest {
                    route_id: arsid,
                    orderings: info.orderings,
                    purpose: RoutePingValidationPurpose::Test,
                };
                let route_entries =
                    Self::build_route_loopback_ping_validations(rss, cur_ts, &req, priority).await;
                entries.extend(route_entries);
            }
        }

        if !entries.is_empty() {
            self.enqueue_ping_validation_entries("RouteTest".to_owned(), entries);
        }
    }

    /// Enqueue per-ordering loopback pings for allocated routes. Same shape as a test,
    /// driven by the keepalive rate limiter — the entry it produces is identical.
    pub async fn enqueue_route_loopback_keepalives(
        &self,
        cur_ts: Timestamp,
        requests: Vec<RoutePingValidationRequest>,
        priority: usize,
    ) {
        let rss = self.route_spec_store();

        let mut entries = Vec::new();
        for req in requests {
            let route_entries =
                Self::build_route_loopback_ping_validations(rss, cur_ts, &req, priority).await;
            entries.extend(route_entries);
        }

        if !entries.is_empty() {
            self.enqueue_ping_validation_entries("RouteKeepalive".to_owned(), entries);
        }
    }

    /// Assemble the components needed to build a loopback Destination for an allocated route.
    /// Releases the route on InvalidTarget; returns None on TryAgain or assembly error.
    async fn assemble_loopback_route_components(
        rss: &RouteSpecStore,
        route_id: &AllocatedRouteSetId,
    ) -> Option<(PublicKey, Arc<PrivateRoute>, usize)> {
        let info = rss.get_allocated_route_test_info(route_id)?;
        let private_route = match rss.assemble_single_private_route(&info.key, None).await {
            Ok(v) => v,
            Err(VeilidAPIError::InvalidTarget { message: _ }) => {
                veilid_log!(rss debug "Route {:#} is dead (invalid target), releasing", route_id);
                rss.release_route(route_id.clone().into());
                return None;
            }
            Err(VeilidAPIError::TryAgain { message: _ }) => return None,
            Err(e) => {
                veilid_log!(rss debug "Error assembling route {:#}: {}", route_id, e);
                return None;
            }
        };
        Some((info.key, private_route, info.hops.len()))
    }

    /// Wrap an assembled private route in a loopback Destination::PrivateRoute at the given sequencing.
    fn build_loopback_destination(
        route_id: &AllocatedRouteSetId,
        private_route: Arc<PrivateRoute>,
        hop_count: usize,
        sequencing: Sequencing,
    ) -> Destination {
        let safety_spec = SafetySpec {
            preferred_route: Some(route_id.clone().into()),
            hop_count,
            stability: Stability::Reliable,
            sequencing,
        };
        Destination::PrivateRoute {
            private_route,
            safety_selection: SafetySelection::Safe(safety_spec),
        }
    }

    /// Build one loopback PingValidationEntry per requested SequenceOrdering.
    /// Each entry's callback reports per-(first-hop, transport) failure on definitive failure
    /// and a per-ordering keepalive timestamp is stamped at enqueue so the keepalive rate
    /// limiter sees the work even before the answer comes back.
    async fn build_route_loopback_ping_validations(
        rss: &RouteSpecStore,
        cur_ts: Timestamp,
        req: &RoutePingValidationRequest,
        priority: usize,
    ) -> Vec<PingValidationEntry> {
        let Some(info) = rss.get_allocated_route_test_info(&req.route_id) else {
            return Vec::new();
        };
        let Some((key, private_route, hop_count)) =
            Self::assemble_loopback_route_components(rss, &req.route_id).await
        else {
            return Vec::new();
        };

        let mut entries = Vec::new();
        for (idx, ordering) in req.orderings.iter().enumerate() {
            let dest = Self::build_loopback_destination(
                &req.route_id,
                private_route.clone(),
                hop_count,
                ordering.strict_sequencing(),
            );
            entries.push(PingValidationEntry {
                group: match req.purpose {
                    RoutePingValidationPurpose::Test => PingValidationGroup::RouteTest,
                    RoutePingValidationPurpose::Keepalive => PingValidationGroup::Keepalive,
                },
                priority,
                key: PingValidationKey::RouteTest {
                    route_id: req.route_id.clone().into(),
                    ping_index: idx,
                },
                dest,
                on_complete: Some(Self::make_loopback_probe_callback(
                    req.route_id.clone().into(),
                    key.clone(),
                    info.hops.clone(),
                    ordering,
                )),
            });
        }

        rss.update_allocated_route_stats(cur_ts, &key, |s| {
            for ordering in req.orderings.iter() {
                s.record_loopback_keepalive(cur_ts, ordering);
            }
            Ok(())
        });

        entries
    }

    /// Build a PingValidationEntry for testing a remote route
    fn build_remote_route_test_entry(
        rss: &RouteSpecStore,
        route_id: &RemoteRouteSetId,
        priority: usize,
    ) -> Option<PingValidationEntry> {
        let private_route = rss.best_remote_private_route(route_id)?;

        let safety_spec = SafetySpec {
            preferred_route: None,
            hop_count: rss.get_default_route_hop_count_safe(),
            stability: Stability::Reliable,
            sequencing: Sequencing::PreferOrdered,
        };
        let safety_selection = SafetySelection::Safe(safety_spec);
        let dest = Destination::PrivateRoute {
            private_route,
            safety_selection,
        };

        let route_id_clone: RouteId = route_id.clone().into();

        // Remote routes: single ping, callback just handles success/failure
        Some(PingValidationEntry {
            group: PingValidationGroup::RouteTest,
            priority,
            key: PingValidationKey::RouteTest {
                route_id: route_id.clone().into(),
                ping_index: 0,
            },
            dest,
            on_complete: Some(Box::new(
                move |registry: VeilidComponentRegistry,
                      result: Result<StatusResult, RPCError>|
                      -> Vec<PingValidationEntry> {
                    match result {
                        Ok(StatusResult::Answer { .. }) => {
                            #[cfg(feature = "verbose-tracing")]
                            veilid_log!(registry trace "Remote route test PASSED: {:#}", route_id_clone);
                        }
                        Ok(StatusResult::Failed(_)) => {
                            let routing_table = registry.routing_table();
                            let nm = routing_table.network_manager();
                            if nm
                                .online_detector()
                                .online_state(RoutingDomain::PublicInternet)
                                == OnlineState::Offline
                            {
                                #[cfg(feature = "verbose-tracing")]
                                veilid_log!(registry debug "Deferring remote route release (offline detected): {:#}", route_id_clone);
                            } else {
                                veilid_log!(registry debug "Remote route test failed (no response): {:#}", route_id_clone);
                                routing_table
                                    .route_spec_store()
                                    .release_route(route_id_clone);
                            }
                        }
                        Ok(StatusResult::NotSent(nr)) => {
                            veilid_log!(registry debug "Remote route test not sent: {:#} - {:?}", route_id_clone, nr);
                        }
                        Err(e) => {
                            veilid_log!(registry debug "Remote route test error: {:#} - {}", route_id_clone, e);
                        }
                    }
                    Vec::new()
                },
            )),
        })
    }

    /// Completion callback for a per-ordering route loopback ping. On Answer we record a
    /// per-ordering pass on the route (drives last_known_valid_ts / clears failures). On Failed
    /// we record a per-ordering failure on the route (drives the ordering, and eventually the
    /// route, to dead) and attribute failure to the transport actually used at the first hop.
    /// NotSent is treated as a non-network failure and left for next cycle.
    fn make_loopback_probe_callback(
        route_id: RouteId,
        route_key: PublicKey,
        hops: Vec<NodeRef>,
        ordering: SequenceOrdering,
    ) -> PingCompletionCallback {
        Box::new(
            move |registry: VeilidComponentRegistry,
                  result: Result<StatusResult, RPCError>|
                  -> Vec<PingValidationEntry> {
                let routing_table = registry.routing_table();
                let rss = routing_table.route_spec_store();
                let sdr = match result {
                    Ok(StatusResult::Answer {
                        answer: _answer, ..
                    }) => {
                        // End-to-end route round-trip (includes inter-hop legs), not a per-node RTT.
                        #[cfg(feature = "verbose-tracing")]
                        veilid_log!(registry debug "Route loopback rtt: route={:#} ord={:#} rtt={:#} hops={}", route_key, ordering, _answer.answer_context.latency, hops.len());

                        let cur_ts = Timestamp::now();
                        rss.update_allocated_route_stats(cur_ts, &route_key, |s| {
                            s.record_loopback_result(ordering, true, cur_ts);
                            Ok(())
                        });
                        return Vec::new();
                    }
                    Ok(StatusResult::Failed(sdr)) => sdr,
                    Ok(StatusResult::NotSent(nr)) => {
                        veilid_log!(registry debug "Route loopback not sent: {:#} ({:#}) - {:?}", route_id, ordering, nr);
                        return Vec::new();
                    }
                    Err(e) => {
                        veilid_log!(registry debug "Deferring route loopback failure (RPC error): {:#} ({:#}) - {}", route_id, ordering, e);
                        return Vec::new();
                    }
                };

                if routing_table
                    .network_manager()
                    .online_detector()
                    .online_state(RoutingDomain::PublicInternet)
                    == OnlineState::Offline
                {
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(registry debug "Deferring route loopback failure (offline): {:#} ({:#})", route_id, ordering);
                    return Vec::new();
                }

                // Record a per-ordering failure on the route so repeated loopback failures
                // drive this ordering (and eventually the route) to dead and release.
                let cur_ts = Timestamp::now();
                rss.update_allocated_route_stats(cur_ts, &route_key, |s| {
                    s.record_loopback_result(ordering, false, cur_ts);
                    Ok(())
                });
                // If this failure leaves the route dead for all orderings, mark it for release
                // now (sticky) so it gets released once its refcount drains.
                rss.mark_allocated_route_for_release_if_dead(&route_key);

                let Some(transport) = sdr.opt_transport_type() else {
                    veilid_log!(registry debug "Route loopback FAILED with no transport recorded: {:#} ({:#})", route_id, ordering);
                    return Vec::new();
                };
                veilid_log!(registry debug "Route loopback FAILED: {:#} ({:#} via {:#})", route_id, ordering, transport);
                for hop in &hops {
                    hop.report_failed_route_test(transport);
                }

                Vec::new()
            },
        )
    }
}
