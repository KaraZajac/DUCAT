use super::*;

impl_veilid_log_facility!("net");

/// Distinct previously-live targets that must fail on the stream framing type, with no
/// intervening success, for the local node to be considered offline.
pub const OFFLINE_DETECTION_FAILURE_THRESHOLD_CONNECTION: usize = 4;
/// Same, but for the datagram socket class.
pub const OFFLINE_DETECTION_FAILURE_THRESHOLD_MESSAGE: usize = 4;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum OnlineState {
    #[default]
    Online,
    Offline,
}

/// Failure count threshold for a socket class.
fn failure_threshold(framing_type: FramingType) -> usize {
    match framing_type {
        FramingType::Connection => OFFLINE_DETECTION_FAILURE_THRESHOLD_CONNECTION,
        FramingType::Message => OFFLINE_DETECTION_FAILURE_THRESHOLD_MESSAGE,
    }
}

/// Events that drive the online/offline state machine.
enum OnlineEvent {
    /// Received something from outside.
    Success,
    /// Failures for previously-live nodes on one framing type.
    Failure {
        live_node_ids: Vec<NodeId>,
        sent_ts: Timestamp,
        framing_type: FramingType,
    },
    /// The local network address changed.
    NetworkChange,
    /// The node detached from the network (a deliberate offline period).
    Detached,
}

/// Per-routing-domain online/offline tracking.
#[derive(Debug)]
struct OnlineDetectionPerDomain {
    /// Previously-live nodes that failed on each framing type since the last success.
    pending_failures: BTreeMap<FramingType, BTreeSet<NodeId>>,
    /// When we last switched online/offline.
    last_transition_ts: Timestamp,
    /// Current state.
    online_state: OnlineState,
    /// Flap detector over online_state.
    flap_detector: FlapDetector<OnlineState>,
    /// Start of the current offline period, if currently offline.
    offline_started_ts: Option<Timestamp>,
    /// Recently-completed offline intervals (start, end), pruned to
    /// `RELIABLE_PING_INTERVAL_MAX` old. Credits node reliability for time we
    /// couldn't test.
    recent_offline_intervals: VecDeque<(Timestamp, Timestamp)>,
}

impl Default for OnlineDetectionPerDomain {
    fn default() -> Self {
        Self {
            pending_failures: BTreeMap::new(),
            last_transition_ts: Timestamp::new(0),
            online_state: OnlineState::default(),
            // 4 online/offline flips within ~60s = flapping
            flap_detector: FlapDetector::new_secs(4.0, 60),
            offline_started_ts: None,
            recent_offline_intervals: VecDeque::new(),
        }
    }
}

/// Detects if our node is online or offline.
///
/// In order to determine if network errors are 'our fault 'or 'the other side's fault', we need to
/// generally know if we are online or offline, before assigning blame to another node and changing
/// its state. This
pub struct OnlineDetector {
    registry: VeilidComponentRegistry,
    inner: Mutex<BTreeMap<RoutingDomain, OnlineDetectionPerDomain>>,
    /// Framing types the network uses; only these gate offline detection.
    enabled_framing_types: Mutex<FramingTypeSet>,
}

impl_veilid_component_accessors!(OnlineDetector);

impl fmt::Debug for OnlineDetector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OnlineDetector")
            .field("inner", &*self.inner.lock())
            .field("enabled_framing_types", &*self.enabled_framing_types.lock())
            .finish()
    }
}

impl OnlineDetector {
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            registry,
            inner: Mutex::new(BTreeMap::new()),
            // Require both classes until the network reports its protocol config.
            enabled_framing_types: Mutex::new(FramingTypeSet::all()),
        }
    }

    /// Whether this routing domain uses ping-based online detection.
    fn uses_ping_detection(routing_domain: RoutingDomain) -> bool {
        // Other domains will plug in their own signals (LocalNetwork: interface link state).
        matches!(routing_domain, RoutingDomain::PublicInternet)
    }

    /// Set which framing types offline detection requires, from the network protocol config.
    pub fn set_protocol_config(&self, outbound: ProtocolTypeSet, inbound: ProtocolTypeSet) {
        let mut framing_types = FramingTypeSet::new();
        for pt in outbound | inbound {
            framing_types.insert(pt.framing_type());
        }
        *self.enabled_framing_types.lock() = framing_types;
    }

    /// Record a successful round-trip.
    pub fn record_success(&self, routing_domain: RoutingDomain) {
        self.process_event(routing_domain, OnlineEvent::Success);
    }

    /// Record RPC failures against the attempted nodes (direct target, or routed hops).
    pub fn record_failure(
        &self,
        routing_domain: RoutingDomain,
        node_ids: impl IntoIterator<Item = NodeId>,
        sent_ts: Timestamp,
        transport: TransportType,
    ) {
        if !Self::uses_ping_detection(routing_domain) {
            return;
        }
        // Only previously-live nodes count: a dead node we probe timing out isn't evidence
        // that *we* are offline. Read liveness now, before the caller's lost-answer /
        // failed-to-send handler clears first_steady_answer_ts.
        let routing_table = self.routing_table();
        let live_node_ids: Vec<NodeId> = node_ids
            .into_iter()
            .filter(|node_id| {
                routing_table
                    .lookup_node_id(node_id.clone())
                    .ok()
                    .flatten()
                    .is_some_and(|nr| nr.rpc_stats().first_steady_answer_ts.is_some())
            })
            .collect();
        if live_node_ids.is_empty() {
            return;
        }
        self.process_event(
            routing_domain,
            OnlineEvent::Failure {
                live_node_ids,
                sent_ts,
                framing_type: transport.framing_type(),
            },
        );
    }

    /// Local network addresses changed: presume offline until a new RPC succeeds.
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
    pub fn network_address_change(&self, routing_domain: RoutingDomain) {
        self.process_event(routing_domain, OnlineEvent::NetworkChange);
    }

    /// Re-arm for a new attach; online/offline history persists across detach.
    pub fn startup(&self) {
        *self.enabled_framing_types.lock() = FramingTypeSet::all();
    }

    /// Detached: mark every ping-detected domain offline so the detached period
    /// is credited to node reliability until we reconnect.
    pub fn shutdown(&self) {
        for routing_domain in RoutingDomain::all() {
            self.process_event(routing_domain, OnlineEvent::Detached);
        }
    }

    /// Current online state for a routing domain.
    pub fn online_state(&self, routing_domain: RoutingDomain) -> OnlineState {
        // Domains without ping detection are always considered online.
        if !Self::uses_ping_detection(routing_domain) {
            return OnlineState::Online;
        }
        self.inner
            .lock()
            .get(&routing_domain)
            .map(|pd| pd.online_state)
            .unwrap_or_default()
    }

    /// When this domain last came back online, if within the reliable ping span.
    /// None if currently offline or no recent reattach.
    pub fn online_since(&self, routing_domain: RoutingDomain) -> Option<Timestamp> {
        if !Self::uses_ping_detection(routing_domain) {
            return None;
        }
        let inner = self.inner.lock();
        let pd = inner.get(&routing_domain)?;
        if pd.offline_started_ts.is_some() {
            return None;
        }
        let (_, end) = pd.recent_offline_intervals.back()?;
        // Protection from a reattach only lasts the reliable ping span.
        if Timestamp::now().duration_since(*end) > crate::routing_table::RELIABLE_PING_INTERVAL_MAX
        {
            return None;
        }
        Some(*end)
    }

    /// Total time within `[from, to]` this domain was offline (couldn't ping).
    pub fn offline_overlap(
        &self,
        routing_domain: RoutingDomain,
        from: Timestamp,
        to: Timestamp,
    ) -> TimestampDuration {
        if !Self::uses_ping_detection(routing_domain) || to <= from {
            return TimestampDuration::new(0);
        }
        let inner = self.inner.lock();
        let Some(pd) = inner.get(&routing_domain) else {
            return TimestampDuration::new(0);
        };
        let mut overlap = 0u64;
        // Completed intervals plus any ongoing offline period extended to `to`.
        for (s, e) in pd
            .recent_offline_intervals
            .iter()
            .copied()
            .chain(pd.offline_started_ts.map(|s| (s, to)))
        {
            let lo = s.max(from);
            let hi = e.min(to);
            if hi > lo {
                overlap += hi.duration_since(lo).as_u64();
            }
        }
        TimestampDuration::new(overlap)
    }

    /// Drive the state machine with an event, dispatching to the current state's handler.
    fn process_event(&self, routing_domain: RoutingDomain, event: OnlineEvent) {
        if !Self::uses_ping_detection(routing_domain) {
            return;
        }
        let enabled_framing_types = *self.enabled_framing_types.lock();
        let now = Timestamp::now();

        let (opt_new_state, opt_flap_penalty) = {
            let mut inner = self.inner.lock();
            let pd = inner.entry(routing_domain).or_default();
            let opt_new_state = match pd.online_state {
                OnlineState::Offline => Self::process_offline_event(&event),
                OnlineState::Online => {
                    Self::process_online_event(pd, &event, enabled_framing_types)
                }
            };
            let mut opt_flap_penalty = None;
            if let Some(new_state) = opt_new_state {
                match new_state {
                    OnlineState::Offline => pd.offline_started_ts = Some(now),
                    OnlineState::Online => {
                        if let Some(start) = pd.offline_started_ts.take() {
                            pd.recent_offline_intervals.push_back((start, now));
                            // Prune intervals older than the reliable ping span.
                            while pd.recent_offline_intervals.front().is_some_and(|(_, e)| {
                                now.duration_since(*e)
                                    > crate::routing_table::RELIABLE_PING_INTERVAL_MAX
                            }) {
                                pd.recent_offline_intervals.pop_front();
                            }
                        }
                    }
                }
                pd.online_state = new_state;
                pd.last_transition_ts = now;
                // A deliberate detach is a clean break, not network flapping: reset the
                // flap baseline so pre-detach history doesn't combine with reattach turbulence.
                if matches!(event, OnlineEvent::Detached) {
                    pd.flap_detector.reset();
                } else {
                    opt_flap_penalty = pd.flap_detector.record(now.as_u64(), new_state);
                }
            }
            (opt_new_state, opt_flap_penalty)
        };

        // Log the transition (and any flapping) outside the lock.
        if let Some(new_state) = opt_new_state {
            veilid_log!(self info "{:?} {} detected", routing_domain,
                if new_state == OnlineState::Online { "online" } else { "offline" });
        }
        if let Some(penalty) = opt_flap_penalty {
            veilid_log!(self debugwarn "{:?} online state FLAPPING (penalty={:.1})", routing_domain, penalty);
        }
    }

    /// Handle an event while offline.
    fn process_offline_event(event: &OnlineEvent) -> Option<OnlineState> {
        match event {
            // Any success means we're back online.
            OnlineEvent::Success => Some(OnlineState::Online),
            OnlineEvent::Failure { .. } | OnlineEvent::NetworkChange | OnlineEvent::Detached => {
                None
            }
        }
    }

    /// Handle an event while online.
    fn process_online_event(
        pd: &mut OnlineDetectionPerDomain,
        event: &OnlineEvent,
        enabled_framing_types: FramingTypeSet,
    ) -> Option<OnlineState> {
        match event {
            OnlineEvent::Success => {
                pd.pending_failures.clear();
                None
            }
            OnlineEvent::Failure {
                live_node_ids,
                sent_ts,
                framing_type,
            } => {
                // Ignore failures from RPCs sent before this online period began.
                if *sent_ts < pd.last_transition_ts {
                    return None;
                }
                let bucket = pd.pending_failures.entry(*framing_type).or_default();
                for node_id in live_node_ids {
                    bucket.insert(node_id.clone());
                }
                // Offline only when every enabled socket class has crossed its threshold, so
                // localized trouble on one class doesn't drive offline while another works.
                let all_failed = !enabled_framing_types.is_empty()
                    && enabled_framing_types.iter().all(|ft| {
                        pd.pending_failures.get(&ft).map_or(0, BTreeSet::len)
                            >= failure_threshold(ft)
                    });
                if all_failed {
                    pd.pending_failures.clear();
                    return Some(OnlineState::Offline);
                }
                None
            }
            OnlineEvent::NetworkChange | OnlineEvent::Detached => {
                pd.pending_failures.clear();
                Some(OnlineState::Offline)
            }
        }
    }
}
