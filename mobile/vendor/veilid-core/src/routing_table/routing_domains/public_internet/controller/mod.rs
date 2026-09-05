mod bootstrap;
mod closest_peers_refresh;
mod contact_method;
mod peer_minimum_refresh;
mod ping_validations;
mod private_route_management;

mod relay_management;
mod state;
mod tasks;

use super::*;

pub use state::*;

impl_veilid_log_facility!("rtab");

/// How frequently we tick the relay management routine
pub const RELAY_MANAGEMENT_INTERVAL_SECS: u32 = 1;

/// How frequently we tick the private route management routine
pub const PRIVATE_ROUTE_MANAGEMENT_INTERVAL_SECS: u32 = 1;

/// Table store key for the PublicInternet routing domain persisted state
const PUBLIC_INTERNET_PERSISTED_STATE_KEY: &[u8] = b"public_internet_persisted_state";

/// PublicInternet routing domain state that survives across restarts
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PublicInternetPersistedState {
    low_water_mark: LowWaterMark,
}

////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

pub struct PublicInternetRoutingDomainControllerUnlockedInner {
    registry: VeilidComponentRegistry,
    detail: Box<RwLock<PublicInternetRoutingDomainDetail>>,
    /// Published peer info for this routing domain
    published_peer_info: Mutex<Option<Arc<PeerInfo>>>,
    /// Last time we pinged checked the active watches
    opt_active_watch_keepalive_ts: Mutex<Option<Timestamp>>,
    /// Last observed (outbound, inbound) stage pair; used to log stage transitions
    last_observed_stages: Mutex<Option<(RoutingDomainOutboundStage, RoutingDomainInboundStage)>>,
    /// Flap detector over public_internet_ready (should stay true once true on a stable network)
    readiness_flap_detector: Mutex<FlapDetector<bool>>,
    /// Flap detector over the selected relay set (should be stable on a stable network)
    relay_flap_detector: Mutex<FlapDetector<BTreeSet<NodeId>>>,
    /// Flap detector over safety_routes_ready (route validity should be stable once established)
    route_flap_detector: Mutex<FlapDetector<bool>>,

    /// Background process to get our initial routing table
    bootstrap_task: TickTask<EyreReport>,
    /// Background process to get more nodes for each crypto kind
    peer_minimum_refresh_task: TickTask<EyreReport>,
    /// Background process to ensure we have enough nodes close to our own in our routing table
    closest_peers_refresh_task: TickTask<EyreReport>,
    /// Background process to check PublicInternet nodes to see if they are still alive and for reliability
    ping_validator_public_internet_task: TickTask<EyreReport>,
    /// Background process to keep relays up
    relay_management_task: TickTask<EyreReport>,
    /// Background process to keep private routes up
    private_route_management_task: TickTask<EyreReport>,

    /// Subscription to RoutingDomainCommitEvent for fast-path tick on edit-commits
    commit_subscription: Mutex<Option<EventBusSubscription>>,
    /// Subscription to PeerInfoChangeEvent for fast-path tick of downstream stages
    peer_info_subscription: Mutex<Option<EventBusSubscription>>,

    /// Bootstrap wait detector (to prevent spamming logs)
    bootstrap_wait_detector: AtomicOptionTimestamp,
    /// Per crypto kind, when we first observed zero responsive connectivity nodes
    no_responsive_since: Mutex<BTreeMap<CryptoKind, Timestamp>>,
}

impl fmt::Debug for PublicInternetRoutingDomainControllerUnlockedInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublicInternetRoutingDomainControllerUnlockedInner")
            .field("detail", &self.detail)
            .field("published_peer_info", &self.published_peer_info)
            .field(
                "opt_active_watch_keepalive_ts",
                &self.opt_active_watch_keepalive_ts,
            )
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct PublicInternetRoutingDomainController {
    unlocked_inner: Arc<PublicInternetRoutingDomainControllerUnlockedInner>,
}

impl core::ops::Deref for PublicInternetRoutingDomainController {
    type Target = PublicInternetRoutingDomainControllerUnlockedInner;

    fn deref(&self) -> &Self::Target {
        &self.unlocked_inner
    }
}

impl_veilid_component_accessors!(PublicInternetRoutingDomainController);

impl SpecificRoutingDomainController for PublicInternetRoutingDomainController {
    const ROUTING_DOMAIN: RoutingDomain = RoutingDomain::PublicInternet;
    type Detail = PublicInternetRoutingDomainDetail;
    type Editor<'a> = RoutingDomainEditorPublicInternet<'a>;

    fn read(&self) -> RwLockReadGuard<'_, Self::Detail> {
        self.unlocked_inner.detail.read()
    }
    fn write(&self) -> RwLockWriteGuard<'_, Self::Detail> {
        self.unlocked_inner.detail.write()
    }
    fn edit(&self) -> Self::Editor<'_> {
        RoutingDomainEditorPublicInternet::new(self)
    }
}

impl PublicInternetRoutingDomainController {
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        let config = registry.config();
        let detail = Box::new(RwLock::new(PublicInternetRoutingDomainDetail::new(
            registry.clone(),
        )));
        let unlocked_inner = PublicInternetRoutingDomainControllerUnlockedInner {
            registry,
            detail,
            published_peer_info: Default::default(),
            opt_active_watch_keepalive_ts: Default::default(),
            last_observed_stages: Default::default(),
            // 3 readiness flips within ~30s = flapping (one-time startup/teardown flip won't trip)
            readiness_flap_detector: Mutex::new(FlapDetector::new_secs(3.0, 30)),
            // 4 relay-set changes within ~60s = flapping
            relay_flap_detector: Mutex::new(FlapDetector::new_secs(4.0, 60)),
            // 3 safety_routes_ready flips within ~30s = route validity flapping
            route_flap_detector: Mutex::new(FlapDetector::new_secs(3.0, 30)),
            bootstrap_task: TickTask::new("bootstrap_task", 1),
            peer_minimum_refresh_task: TickTask::new("peer_minimum_refresh_task", 1),
            closest_peers_refresh_task: TickTask::new_ms(
                "closest_peers_refresh_task",
                config.internal().network.dht.min_peer_refresh_time_ms,
            ),
            ping_validator_public_internet_task: TickTask::new(
                "ping_validator_public_internet_task",
                1,
            ),
            relay_management_task: TickTask::new(
                "relay_management_task",
                RELAY_MANAGEMENT_INTERVAL_SECS,
            ),
            private_route_management_task: TickTask::new(
                "private_route_management_task",
                PRIVATE_ROUTE_MANAGEMENT_INTERVAL_SECS,
            ),
            commit_subscription: Default::default(),
            peer_info_subscription: Default::default(),
            bootstrap_wait_detector: AtomicOptionTimestamp::new(None),
            no_responsive_since: Default::default(),
        };
        let this = Self {
            unlocked_inner: Arc::new(unlocked_inner),
        };
        this.setup_tasks();
        this
    }
}

impl RoutingDomainController for PublicInternetRoutingDomainController {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    /// Read access to the routing domain detail
    fn read_dyn(&self) -> MappedRwLockReadGuard<'_, dyn RoutingDomainDetail> {
        RwLockReadGuard::map(self.unlocked_inner.detail.read(), |x| {
            x as &dyn RoutingDomainDetail
        })
    }

    /// Write access to the routing domain detail
    fn write_dyn(&self) -> MappedRwLockWriteGuard<'_, dyn RoutingDomainDetail> {
        RwLockWriteGuard::map(self.unlocked_inner.detail.write(), |x| {
            x as &mut dyn RoutingDomainDetail
        })
    }

    /// Editor access to common fields in the routing domain detail
    fn edit_dyn(&self) -> Box<dyn RoutingDomainEditor + '_> {
        Box::new(RoutingDomainEditorPublicInternet::new(self))
    }

    fn routing_domain(&self) -> RoutingDomain {
        RoutingDomain::PublicInternet
    }

    /// Start up the routing domain controller
    fn startup(&self) -> PinBoxFuture<'_, EyreResult<()>> {
        Box::pin(async move {
            let commit_sub = impl_subscribe_event_bus_async_clone!(self, commit_event_handler);
            *self.commit_subscription.lock() = Some(commit_sub);

            let peer_info_sub =
                impl_subscribe_event_bus_async_clone!(self, peer_info_change_event_handler);
            *self.peer_info_subscription.lock() = Some(peer_info_sub);

            // Don't let detached time count toward the fallback-bootstrap delay
            self.no_responsive_since.lock().clear();

            // Publish peer info
            self.publish_peer_info();
            Ok(())
        })
    }

    /// Shut down the routing domain controller
    fn shutdown(&self) -> PinBoxFuture<'_, ()> {
        Box::pin(async move {
            if let Some(sub) = self.commit_subscription.lock().take() {
                self.event_bus().unsubscribe(sub);
            }
            if let Some(sub) = self.peer_info_subscription.lock().take() {
                self.event_bus().unsubscribe(sub);
            }

            // Unpublish peer info
            self.unpublish_peer_info();
        })
    }

    fn tick(&self) -> PinBoxFuture<'_, EyreResult<()>> {
        Box::pin(PublicInternetRoutingDomainController::tick(self))
    }

    fn cancel_tasks(&self) -> PinBoxFuture<'_, ()> {
        Box::pin(PublicInternetRoutingDomainController::cancel_tasks(self))
    }

    fn load<'a>(&'a self, db: &'a TableDB) -> PinBoxFuture<'a, EyreResult<()>> {
        Box::pin(async move {
            let Some(persisted): Option<PublicInternetPersistedState> =
                db.load_json(0, PUBLIC_INTERNET_PERSISTED_STATE_KEY).await?
            else {
                return Ok(());
            };
            let lwm = Arc::new(persisted.low_water_mark);
            let detail = self.read_dyn();
            detail.reset_low_water_mark();
            detail.update_low_water_mark(lwm);
            Ok(())
        })
    }

    fn save<'a>(&'a self, dbx: &'a TableDBTransaction) -> PinBoxFuture<'a, EyreResult<()>> {
        Box::pin(async move {
            let low_water_mark = {
                let detail = self.read_dyn();
                detail.get_low_water_mark().as_ref().clone()
            };
            let persisted = PublicInternetPersistedState { low_water_mark };
            dbx.store_json(0, PUBLIC_INTERNET_PERSISTED_STATE_KEY, &persisted)
                .await?;
            Ok(())
        })
    }

    fn state(&self) -> RoutingDomainState {
        PublicInternetRoutingDomainController::state(self)
    }

    fn get_health(&self) -> RoutingDomainHealth {
        let state = self.state();

        let entry_summary = state.entry_summary;
        let low_water_mark = state.low_water_mark;
        let is_ready_inbound = state.is_ready_inbound;
        let is_ready_outbound = state.is_ready_outbound;

        RoutingDomainHealth {
            entry_summary,
            low_water_mark,
            is_ready_inbound,
            is_ready_outbound,
        }
    }

    fn publish_peer_info(&self) -> bool {
        let (opt_old_peer_info, opt_new_peer_info) = {
            let state = self.state();

            let new_peer_info = if matches!(
                state.inbound_stage,
                RoutingDomainInboundStage::ReadyToPublish
            ) {
                state.current_peer_info
            } else {
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(self debug "[PublicInternet] Not publishing peer info because it is not ready to publish");
                return false;
            };

            // Don't publish if the peer info hasnt changed from our previous publication
            let mut ppi_lock = self.published_peer_info.lock();
            let opt_old_peer_info = (*ppi_lock).clone();

            if let Some(old_peer_info) = &opt_old_peer_info {
                if new_peer_info.equivalent(old_peer_info) {
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(self debug "[PublicInternet] Not publishing peer info because it is equivalent");
                    return false;
                }
            }

            veilid_log!(self debug "[PublicInternet] Published new peer info: {}", new_peer_info);

            // Purge any routing-table entries whose direct dial info overlaps with ours.
            // Done before commit so any reliability/route work that fires off the change event
            // doesn't see those entries.
            let removed = self
                .routing_table()
                .purge_entries_with_overlapping_dial_info(
                    RoutingDomain::PublicInternet,
                    new_peer_info.node_info(),
                );
            if removed > 0 {
                veilid_log!(self debug "[PublicInternet] Purged {} entries with dial info overlapping our own", removed);
            }

            *ppi_lock = Some(new_peer_info.clone());

            // Report peer info change to route spec store cache
            self.routing_table()
                .route_spec_store()
                .report_peer_info_changed();

            (opt_old_peer_info, Some(new_peer_info))
        };

        if let Err(e) = self.event_bus().post(PeerInfoChangeEvent {
            routing_domain: RoutingDomain::PublicInternet,
            opt_old_peer_info,
            opt_new_peer_info,
        }) {
            veilid_log!(self debug "Failed to post event: {}", e);
        }

        true
    }

    fn unpublish_peer_info(&self) {
        let mut ppi_lock = self.published_peer_info.lock();
        let opt_old_peer_info = ppi_lock.clone();
        if opt_old_peer_info.is_none() {
            return;
        }
        veilid_log!(self debug "[PublicInternet] Unpublished peer info");
        *ppi_lock = None;

        // Report peer info change to route spec store cache
        self.routing_table()
            .route_spec_store()
            .report_peer_info_changed();

        if let Err(e) = self.event_bus().post(PeerInfoChangeEvent {
            routing_domain: RoutingDomain::PublicInternet,
            opt_old_peer_info,
            opt_new_peer_info: None,
        }) {
            veilid_log!(self debug "Failed to post event: {}", e);
        }
    }

    fn get_published_peer_info(&self) -> Option<Arc<PeerInfo>> {
        self.published_peer_info.lock().clone()
    }

    fn get_contact_methods(&self, request: ContactMethodRequest) -> Vec<ContactMethod> {
        PublicInternetRoutingDomainController::get_contact_methods(self, request)
    }
}
