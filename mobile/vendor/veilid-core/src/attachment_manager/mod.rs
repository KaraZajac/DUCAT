mod attachment_level_calculator;

#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod tests_attachment_manager;

use super::*;
use attachment_level_calculator::*;
use network_manager::StartupDisposition;
use routing_table::{BucketEntryState, RoutingDomain, RoutingDomainReadyEvent, RoutingTable};

impl_veilid_log_facility!("attach");

const TICK_INTERVAL_MSEC: u32 = 1000;
const ATTACHMENT_MAINTAINER_INTERVAL_MSEC: u32 = 1000;
const BIND_WAIT_DELAY: TimestampDuration = TimestampDuration::new_ms(10000);

type UpdateStateCallback = Box<dyn FnOnce(MutexGuard<AttachmentManagerInner>)>;

#[derive(Debug, Clone)]
pub struct AttachmentManagerStartupContext {
    pub initialize_lock: Arc<StartupLock>,
    pub attachment_lock: Arc<StartupLock>,
}
impl AttachmentManagerStartupContext {
    pub fn new() -> Self {
        Self {
            initialize_lock: Arc::new(StartupLock::new()),
            attachment_lock: Arc::new(StartupLock::new()),
        }
    }
}
impl Default for AttachmentManagerStartupContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Event sent every second while veilid-core is initialized
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct TickEvent {
    pub last_tick_ts: Option<Timestamp>,
    pub cur_tick_ts: Timestamp,
}

struct AttachmentManagerInner {
    /// The current attachment state
    attachment_state: AttachmentState,
    /// The last VeilidStateAttachment that was calculated
    last_veilid_state_attachment: Box<VeilidStateAttachment>,
    /// The last time PublicInternet changed readiness
    last_public_internet_ready_changed_ts: Option<Timestamp>,
    /// The last time LocalNetwork changed readiness
    last_local_network_ready_changed_ts: Option<Timestamp>,
    /// Whether to maintain peers
    maintain_peers: bool,
    /// The timestamp when the node started
    started_ts: Timestamp,
    /// The timestamp when the node attached
    attach_ts: Option<Timestamp>,
    /// The timestamp when the node should retry binding
    bind_retry_ts: Option<Timestamp>,
    /// The timestamp of the last tick
    last_tick_ts: Option<Timestamp>,
    /// The future for the attachment maintainer task
    tick_future: Option<PinBoxFutureStatic<()>>,
    /// Subscription to RoutingDomainReadyEvent
    routing_domain_ready_subscription: Option<EventBusSubscription>,
}

impl fmt::Debug for AttachmentManagerInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentManagerInner")
            .field("attachment_state", &self.attachment_state)
            .field(
                "last_veilid_state_attachment",
                &self.last_veilid_state_attachment,
            )
            .field(
                "last_public_internet_ready_changed_ts",
                &self.last_public_internet_ready_changed_ts,
            )
            .field(
                "last_local_network_ready_changed_ts",
                &self.last_local_network_ready_changed_ts,
            )
            .field("maintain_peers", &self.maintain_peers)
            .field("started_ts", &self.started_ts)
            .field("attach_ts", &self.attach_ts)
            .field("bind_retry_ts", &self.bind_retry_ts)
            .field("last_tick_ts", &self.last_tick_ts)
            .finish()
    }
}

pub struct AttachmentManager {
    registry: VeilidComponentRegistry,
    inner: Mutex<AttachmentManagerInner>,
    startup_context: AttachmentManagerStartupContext,
    attachment_maintainer_task: TickTask<EyreReport>,
    level_calculator: AttachmentLevelCalculator,
}

impl fmt::Debug for AttachmentManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentManager")
            // .field("registry", &self.registry)
            .field("inner", &self.inner)
            .field("startup_context", &self.startup_context)
            .field("level_calculator", &self.level_calculator)
            // .field("attachment_maintainer_task", &self.attachment_maintainer_task)
            .finish()
    }
}

impl_veilid_component!(AttachmentManager);

impl AttachmentManager {
    fn new_inner() -> AttachmentManagerInner {
        AttachmentManagerInner {
            attachment_state: AttachmentState::Detached,
            last_veilid_state_attachment: Box::new(VeilidStateAttachment::default()),
            last_public_internet_ready_changed_ts: None,
            last_local_network_ready_changed_ts: None,
            maintain_peers: false,
            started_ts: Timestamp::now(),
            attach_ts: None,
            last_tick_ts: None,
            bind_retry_ts: None,
            tick_future: None,
            routing_domain_ready_subscription: None,
        }
    }
    pub fn new(
        registry: VeilidComponentRegistry,
        startup_context: AttachmentManagerStartupContext,
    ) -> Self {
        Self {
            level_calculator: AttachmentLevelCalculator::new(registry.clone()),
            registry,
            inner: Mutex::new(Self::new_inner()),
            startup_context,
            attachment_maintainer_task: TickTask::new_ms(
                "attachment_maintainer_task",
                ATTACHMENT_MAINTAINER_INTERVAL_MSEC,
            ),
        }
    }

    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub fn is_attached(&self) -> bool {
        self.inner.lock().attachment_state.is_attached()
    }

    #[allow(dead_code)]
    pub fn is_detached(&self) -> bool {
        self.inner.lock().attachment_state.is_detached()
    }

    #[allow(dead_code)]
    pub fn get_attach_timestamp(&self) -> Option<Timestamp> {
        self.inner.lock().attach_ts
    }

    fn log_facilities_impl(&self) -> VeilidComponentLogFacilities {
        VeilidComponentLogFacilities::new()
            .with_facility(VeilidComponentLogFacility::try_new("attach").unwrap())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key())))]
    #[allow(clippy::unused_async)]
    pub async fn init_async(&self) -> EyreResult<()> {
        let guard = self.startup_context.initialize_lock.startup()?;
        guard.success();
        Ok(())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key())))]
    #[allow(clippy::unused_async)]
    pub async fn post_init_async(&self) -> EyreResult<()> {
        let registry = self.registry();

        veilid_log!(self debug "starting attachment maintainer task");
        impl_setup_task_async!(
            self,
            Self,
            attachment_maintainer_task,
            attachment_maintainer_task_routine
        );

        let routing_domain_ready_subscription =
            impl_subscribe_event_bus!(self, Self, routing_domain_ready_event_handler);

        let mut inner = self.inner.lock();
        inner.routing_domain_ready_subscription = Some(routing_domain_ready_subscription);

        // Let the attachment maintainer run
        let guard = self.startup_context.attachment_lock.startup()?;
        guard.success();

        // Create top level tick interval
        let tick_future = interval(
            "attachment maintainer tick",
            TICK_INTERVAL_MSEC,
            true,
            move || {
                let registry = registry.clone();
                async move {
                    let this = registry.attachment_manager();
                    if let Err(e) = this.tick().await {
                        veilid_log!(this warn "attachment maintainer tick failed: {}", e);
                    }
                }
            },
        );

        // Keep ticker, drop this to stop the ticker
        inner.tick_future = Some(tick_future);

        Ok(())
    }

    fn routing_domain_ready_event_handler(&self, _evt: Arc<RoutingDomainReadyEvent>) {
        let inner = self.inner.lock();
        if !matches!(
            inner.attachment_state,
            AttachmentState::AttachedWeak
                | AttachmentState::AttachedFair
                | AttachmentState::AttachedGood
                | AttachmentState::AttachedStrong
                | AttachmentState::AttachedFull
                | AttachmentState::Attaching
        ) {
            return;
        }
        self.update_attached_state(inner, None);
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn pre_terminate_async(&self) {
        // Start attachment shutdown
        let guard = self
            .startup_context
            .attachment_lock
            .shutdown()
            .await
            .expect_or_log("should be initialized");

        // Wait for detached state
        while !matches!(
            self.inner.lock().attachment_state,
            AttachmentState::Detached
        ) {
            sleep(100).await;
        }

        let sub = self.inner.lock().routing_domain_ready_subscription.take();
        if let Some(sub) = sub {
            self.event_bus().unsubscribe(sub);
        }

        // Stop ticker
        let tick_future = self.inner.lock().tick_future.take();
        if let Some(tick_future) = tick_future {
            tick_future.await;
        }

        // Stop background operations
        veilid_log!(self debug "stopping attachment maintainer task");
        if let Err(e) = self.attachment_maintainer_task.stop().await {
            veilid_log!(self warn "attachment_maintainer not stopped: {}", e);
        }

        // Attachent shutdown successful
        guard.success();
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn terminate_async(&self) {
        let guard = self
            .startup_context
            .initialize_lock
            .shutdown()
            .await
            .expect_or_log("should be initialized");

        // Shutdown successful
        guard.success();
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    #[allow(clippy::unused_async)]
    pub async fn attach(&self) -> bool {
        let Ok(_guard) = self.startup_context.initialize_lock.enter() else {
            return false;
        };

        let mut inner = self.inner.lock();
        // If attaching is disabled (because we are terminating)
        // then just return now

        if !self.startup_context.attachment_lock.is_started() {
            return false;
        }
        let previous = inner.maintain_peers;
        inner.maintain_peers = true;

        previous != inner.maintain_peers
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key())))]
    #[allow(clippy::unused_async)]
    pub async fn detach(&self) -> bool {
        let Ok(_guard) = self.startup_context.initialize_lock.enter() else {
            return false;
        };

        {
            let mut inner = self.inner.lock();
            let previous = inner.maintain_peers;
            if !previous {
                // Already detached or detaching
                return false;
            }
            // Wants to be detached
            inner.maintain_peers = false;
        }

        true
    }

    /////////////////////////////////////////////////////////////////////////////

    async fn tick(&self) -> EyreResult<()> {
        let cur_tick_ts = Timestamp::now_non_decreasing();
        let last_tick_ts = {
            let mut inner = self.inner.lock();
            let last_tick_ts = inner.last_tick_ts;
            inner.last_tick_ts = Some(cur_tick_ts);
            last_tick_ts
        };

        // Log if we're seeing missed ticks
        if let Some(lag) = last_tick_ts.map(|x| cur_tick_ts.duration_since(x)) {
            if lag > TimestampDuration::new_ms(2 * (TICK_INTERVAL_MSEC as u64)) {
                veilid_log!(self debug "tick lag: {}", lag);
            }
        }

        // Tick our own ticktask for the attachment maintainer state machine
        self.attachment_maintainer_task.tick().await?;

        // Send a 'tick' event for the rest of the system to get ticks
        let event_bus = self.event_bus();
        event_bus.post(TickEvent {
            last_tick_ts,
            cur_tick_ts,
        })?;

        Ok(())
    }

    // Manage attachment state
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn attachment_maintainer_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        cur_ts: Timestamp,
    ) -> EyreResult<()> {
        // Enter the attachment lock and return if we are shutting down
        let Ok(_guard) = self.startup_context.attachment_lock.enter() else {
            let do_shutdown = {
                let inner = self.inner.lock();
                let current_attachment_state = inner.attachment_state;

                // Only shutdown is possible if we get here
                match current_attachment_state {
                    AttachmentState::Detached | AttachmentState::Detaching => {
                        // no need to shut down because if we got here we're already shutting down
                        false
                    }
                    AttachmentState::Attaching
                    | AttachmentState::AttachedWeak
                    | AttachmentState::AttachedFair
                    | AttachmentState::AttachedGood
                    | AttachmentState::AttachedStrong
                    | AttachmentState::AttachedFull => {
                        veilid_log!(self debug "terminating attachment maintainer task");
                        self.transition_to_non_attached_state(
                            inner,
                            AttachmentState::Detaching,
                            None,
                        );

                        // perform shutdown
                        true
                    }
                }
            };
            if do_shutdown {
                self.shutdown().await;
                {
                    let inner = self.inner.lock();
                    self.transition_to_non_attached_state(inner, AttachmentState::Detached, None);
                }
            }

            return Ok(());
        };

        // Process the attachment state machine
        loop {
            let mut do_startup = false;
            let mut do_shutdown = false;
            let mut continue_loop = false;

            // Process the current state
            {
                let mut inner = self.inner.lock();

                let state = inner.attachment_state;
                let maintain_peers = inner.maintain_peers;
                let bind_retry_waiting =
                    if inner.bind_retry_ts.is_some() && inner.bind_retry_ts.unwrap() > cur_ts {
                        true
                    } else {
                        inner.bind_retry_ts = None;
                        false
                    };

                match state {
                    AttachmentState::Detached => {
                        if maintain_peers && !bind_retry_waiting {
                            veilid_log!(self debug "attachment starting");
                            do_startup = true;
                        }
                    }
                    AttachmentState::Attaching
                    | AttachmentState::AttachedWeak
                    | AttachmentState::AttachedFair
                    | AttachmentState::AttachedGood
                    | AttachmentState::AttachedStrong
                    | AttachmentState::AttachedFull => {
                        if maintain_peers {
                            let network_manager = self.network_manager();
                            if network_manager.network_needs_restart() {
                                veilid_log!(self info "Restarting network");
                                self.transition_to_non_attached_state(
                                    inner,
                                    AttachmentState::Detaching,
                                    None,
                                );
                                continue_loop = true;
                            } else {
                                self.update_attached_state(inner, None);
                            }
                        } else {
                            veilid_log!(self debug "stopped maintaining peers");
                            self.transition_to_non_attached_state(
                                inner,
                                AttachmentState::Detaching,
                                None,
                            );
                            continue_loop = true;
                        }
                    }
                    AttachmentState::Detaching => {
                        veilid_log!(self debug "shutting down attachment");
                        do_shutdown = true;
                    }
                }
            };

            if do_startup {
                match self.startup().await {
                    Err(err) => {
                        error!("attachment startup failed: {}", err);
                    }
                    Ok(StartupDisposition::BindRetry) => {
                        veilid_log!(self info "waiting for network to bind...");
                        self.inner.lock().bind_retry_ts =
                            Some(Timestamp::now_non_decreasing().later(BIND_WAIT_DELAY));
                    }
                    Ok(StartupDisposition::Success) => {
                        veilid_log!(self debug "started maintaining peers");

                        let inner = self.inner.lock();
                        self.transition_to_non_attached_state(
                            inner,
                            AttachmentState::Attaching,
                            None,
                        );
                    }
                }
            } else if do_shutdown {
                self.shutdown().await;

                let inner = self.inner.lock();
                self.transition_to_non_attached_state(inner, AttachmentState::Detached, None);
            }

            // Loop again if we should process the next state immediately
            if !continue_loop {
                break;
            }
        }

        Ok(())
    }

    async fn startup(&self) -> EyreResult<StartupDisposition> {
        veilid_log!(self info "Attaching...");

        let rpc_processor = self.rpc_processor();
        let network_manager = self.network_manager();

        // Startup network manager
        let res = network_manager.startup().await?;
        match res {
            StartupDisposition::Success => {
                veilid_log!(self debug "NetworkManager startup success");
            }
            StartupDisposition::BindRetry => {
                veilid_log!(self debug "NetworkManager bind retry");
                return Ok(StartupDisposition::BindRetry);
            }
        }

        // Startup rpc processor
        if let Err(e) = rpc_processor.startup() {
            network_manager.shutdown().await;
            return Err(e);
        }

        // Startup routing table
        let routing_table = self.routing_table();
        if let Err(e) = routing_table.startup().await {
            rpc_processor.shutdown().await;
            network_manager.shutdown().await;
            return Err(e);
        }

        // Inform api clients that things have changed
        network_manager.send_network_update();

        veilid_log!(self info "Attach successful");

        Ok(StartupDisposition::Success)
    }

    async fn shutdown(&self) {
        veilid_log!(self info "Detaching...");

        let routing_table = self.routing_table();
        let rpc_processor = self.rpc_processor();
        let network_manager = self.network_manager();

        // Shutdown RoutingTable
        routing_table.shutdown().await;

        // Shutdown NetworkManager
        network_manager.shutdown().await;

        // Shutdown RPCProcessor
        rpc_processor.shutdown().await;

        // Send update
        network_manager.send_network_update();

        veilid_log!(self info "Detach successful");
    }

    /// Update attachment and network readiness state
    /// and possibly send a VeilidUpdate::Attachment.
    fn update_attached_state(
        &self,
        mut inner: MutexGuard<AttachmentManagerInner>,
        opt_callback: Option<UpdateStateCallback>,
    ) {
        let routing_table = self.network_manager().routing_table();
        let health = routing_table.get_routing_table_health();

        let now = Timestamp::now_non_decreasing();
        let uptime = now.duration_since(inner.started_ts);
        let attached_uptime = inner.attach_ts.map(|ts| now.duration_since(ts));

        let new_state = self.level_calculator.recompute();
        let inputs = self.level_calculator.last_inputs();
        inner.attachment_state = new_state;

        let veilid_state_attachment = Box::new(VeilidStateAttachment {
            state: new_state,
            public_internet_ready: health.routing_domain_ready(RoutingDomain::PublicInternet),
            local_network_ready: health.routing_domain_ready(RoutingDomain::LocalNetwork),
            uptime,
            attached_uptime,
            reliable_peer_count: NodeCount::from(inputs.reliable_count as u64),
            live_peer_count: NodeCount::from(inputs.live_count as u64),
            estimated_network_size: NodeCount::from(inputs.estimated_network_size),
            median_latency: inputs.median_latency,
            over_attached_nodes: NodeCount::from(inputs.excess_kickable as u64),
        });

        self.update_veilid_state_attachment(inner, veilid_state_attachment, opt_callback);
    }

    fn transition_to_non_attached_state(
        &self,
        mut inner: MutexGuard<AttachmentManagerInner>,
        state: AttachmentState,
        opt_callback: Option<UpdateStateCallback>,
    ) {
        // Update timestamps
        let now = Timestamp::now_non_decreasing();
        match state {
            AttachmentState::Attaching => {
                inner.attach_ts = Some(now);
                inner.last_public_internet_ready_changed_ts = Some(now);
                inner.last_local_network_ready_changed_ts = Some(now);
            }
            AttachmentState::Detached => {
                inner.attach_ts = None;
                inner.last_public_internet_ready_changed_ts = None;
                inner.last_local_network_ready_changed_ts = None;
            }
            AttachmentState::Detaching => {
                // ok
            }
            AttachmentState::AttachedWeak
            | AttachmentState::AttachedFair
            | AttachmentState::AttachedGood
            | AttachmentState::AttachedStrong
            | AttachmentState::AttachedFull => {
                veilid_log!(self error "don't use this for attached states, use update_attached_state()");
                return;
            }
        }
        let uptime = now.duration_since(inner.started_ts);
        let attached_uptime = inner.attach_ts.map(|ts| now.duration_since(ts));

        // Update attachment state
        inner.attachment_state = state;
        let veilid_state_attachment = Box::new(VeilidStateAttachment {
            state,
            public_internet_ready: false,
            local_network_ready: false,
            uptime,
            attached_uptime,
            reliable_peer_count: NodeCount::from(0),
            live_peer_count: NodeCount::from(0),
            estimated_network_size: NodeCount::from(0),
            median_latency: None,
            over_attached_nodes: NodeCount::from(0),
        });
        self.update_veilid_state_attachment(inner, veilid_state_attachment, opt_callback);
    }

    fn update_veilid_state_attachment(
        &self,
        mut inner: MutexGuard<AttachmentManagerInner>,
        veilid_state_attachment: Box<VeilidStateAttachment>,
        opt_callback: Option<UpdateStateCallback>,
    ) {
        // Print PublicInternet readiness change
        let mut public_internet_ready_changed = false;
        if inner.last_veilid_state_attachment.public_internet_ready
            != veilid_state_attachment.public_internet_ready
        {
            let cur_ts = Timestamp::now_non_decreasing();
            if let Some(last_changed_ts) = inner.last_public_internet_ready_changed_ts {
                let dur = cur_ts.duration_since(last_changed_ts);
                veilid_log!(self info "PublicInternet {} in {:#}", if veilid_state_attachment.public_internet_ready { "ready" } else { "unavailable" }, dur);
            }
            inner.last_public_internet_ready_changed_ts = Some(cur_ts);
            public_internet_ready_changed = true;
        }

        // Print LocalNetwork readiness change
        let mut local_network_ready_changed = false;
        if inner.last_veilid_state_attachment.local_network_ready
            != veilid_state_attachment.local_network_ready
        {
            let cur_ts = Timestamp::now_non_decreasing();
            if let Some(last_changed_ts) = inner.last_local_network_ready_changed_ts {
                let dur = cur_ts.duration_since(last_changed_ts);
                veilid_log!(self info "LocalNetwork {} in {:#}", if veilid_state_attachment.local_network_ready { "ready" } else { "unavailable" }, dur);
            }
            inner.last_local_network_ready_changed_ts = Some(cur_ts);
            local_network_ready_changed = true;
        }

        // Send update if one of:
        // * the attachment state has changed
        // * routing domain readiness has changed
        let send_update = inner.last_veilid_state_attachment.state != veilid_state_attachment.state
            || public_internet_ready_changed
            || local_network_ready_changed;

        let opt_update_to_send = if send_update {
            Some(VeilidUpdate::Attachment(veilid_state_attachment.clone()))
        } else {
            None
        };

        inner.last_veilid_state_attachment = veilid_state_attachment;

        if let Some(callback) = opt_callback {
            callback(inner);
        } else {
            drop(inner);
        }

        // Send update
        if let Some(update_to_send) = opt_update_to_send {
            (self.update_callback())(update_to_send);
        }
    }

    pub fn get_veilid_state(&self) -> Box<VeilidStateAttachment> {
        // Update the attachment state immediately
        let out_state = Arc::new(Mutex::new(None));
        let out_state_clone = out_state.clone();
        let update_out_state = Box::new(move |inner: MutexGuard<AttachmentManagerInner>| {
            *out_state_clone.lock() = Some(inner.last_veilid_state_attachment.clone())
        });
        {
            let inner = self.inner.lock();
            let current_attachment_state = inner.attachment_state;
            match current_attachment_state {
                AttachmentState::Attaching
                | AttachmentState::AttachedWeak
                | AttachmentState::AttachedFair
                | AttachmentState::AttachedGood
                | AttachmentState::AttachedStrong
                | AttachmentState::AttachedFull => {
                    // Update the attachment state immediately, possibly sending an update
                    self.update_attached_state(inner, Some(update_out_state));
                }
                AttachmentState::Detached | AttachmentState::Detaching => {
                    // Transition to the current state just returns the current state,
                    // possibly sending an update
                    self.transition_to_non_attached_state(
                        inner,
                        current_attachment_state,
                        Some(update_out_state),
                    )
                }
            }
        }

        // Return the last attachment state
        let out = { out_state.lock().take() };
        out.unwrap_or_default()
    }

    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub fn debug_info_nodeinfo(&self) -> String {
        let mut out = String::new();
        out += &format!(
            "VeilidStateAttachment:\n{}\n",
            indent_all_string(format!("{:#}", self.get_veilid_state()))
        );
        out
    }
}
