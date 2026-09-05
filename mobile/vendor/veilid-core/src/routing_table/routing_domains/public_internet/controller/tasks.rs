use super::*;

impl_veilid_log_facility!("rtab");

impl PublicInternetRoutingDomainController {
    pub fn setup_tasks(&self) {
        // Set up bootstrap task
        impl_setup_task_async_clone!(self, bootstrap_task, bootstrap_task_routine);

        // Set up peer minimum refresh task
        impl_setup_task_async_clone!(
            self,
            peer_minimum_refresh_task,
            peer_minimum_refresh_task_routine
        );

        // Set up 'closest peers refresh' task
        impl_setup_task_async_clone!(
            self,
            closest_peers_refresh_task,
            closest_peers_refresh_task_routine
        );

        // Set ping validator tick task
        impl_setup_task_clone!(
            self,
            ping_validator_public_internet_task,
            ping_validator_public_internet_task_routine
        );

        // Set relay management tick task
        impl_setup_task_clone!(self, relay_management_task, relay_management_task_routine);

        // Set private route management tick task
        impl_setup_task_async_clone!(
            self,
            private_route_management_task,
            private_route_management_task_routine
        );
    }

    fn check_bootstrap(state: &RoutingDomainState) -> bool {
        match state.outbound_stage {
            RoutingDomainOutboundStage::Invalid => false,
            RoutingDomainOutboundStage::NeedsBootstrap => true,
            RoutingDomainOutboundStage::NeedsPublishedPeerInfo
            | RoutingDomainOutboundStage::NeedsMoreTestedNodes
            | RoutingDomainOutboundStage::NeedsSafetyRoutes
            | RoutingDomainOutboundStage::ReadyToOperate => false,
        }
    }

    // Peer minimum refresh may need to happen before we have published peer info
    // and it may need to happen for some crypto kinds and not others
    fn check_peer_minimum_refresh(state: &RoutingDomainState, nodes_needed: &NodesNeeded) -> bool {
        match state.outbound_stage {
            RoutingDomainOutboundStage::Invalid | RoutingDomainOutboundStage::NeedsBootstrap => {
                false
            }
            RoutingDomainOutboundStage::NeedsPublishedPeerInfo
            | RoutingDomainOutboundStage::NeedsMoreTestedNodes
            | RoutingDomainOutboundStage::NeedsSafetyRoutes
            | RoutingDomainOutboundStage::ReadyToOperate => {
                !nodes_needed.needs_peer_minimum_refresh.is_empty()
            }
        }
    }

    // Finding closer peers is best done after we have published peer info
    // so that when we find them, they will also put us in their routing table
    fn check_closest_peers_refresh(state: &RoutingDomainState) -> bool {
        match state.outbound_stage {
            RoutingDomainOutboundStage::Invalid
            | RoutingDomainOutboundStage::NeedsBootstrap
            | RoutingDomainOutboundStage::NeedsPublishedPeerInfo => false,
            RoutingDomainOutboundStage::NeedsMoreTestedNodes
            | RoutingDomainOutboundStage::NeedsSafetyRoutes
            | RoutingDomainOutboundStage::ReadyToOperate => true,
        }
    }

    fn check_ping_validator_public_internet(state: &RoutingDomainState) -> bool {
        match state.outbound_stage {
            RoutingDomainOutboundStage::Invalid => false,
            RoutingDomainOutboundStage::NeedsBootstrap => false,
            RoutingDomainOutboundStage::NeedsPublishedPeerInfo
            | RoutingDomainOutboundStage::NeedsMoreTestedNodes
            | RoutingDomainOutboundStage::NeedsSafetyRoutes
            | RoutingDomainOutboundStage::ReadyToOperate => true,
        }
    }

    fn check_relay_management(state: &RoutingDomainState) -> bool {
        match state.inbound_stage {
            RoutingDomainInboundStage::Invalid
            | RoutingDomainInboundStage::NeedsDialInfoConfirmation
            | RoutingDomainInboundStage::Unusable => false,
            RoutingDomainInboundStage::NeedsRelays | RoutingDomainInboundStage::ReadyToPublish => {
                // Tick the relay management task if we need relays at all, or if we have some already but they need to be validated/optimized
                state.relay_requirements.needs_relays()
                    || state
                        .opt_relay_compilation
                        .as_ref()
                        .map(|c| !c.relays.is_empty())
                        .unwrap_or(false)
            }
        }
    }

    fn check_private_route_management(state: &RoutingDomainState) -> bool {
        match state.outbound_stage {
            RoutingDomainOutboundStage::Invalid
            | RoutingDomainOutboundStage::NeedsBootstrap
            | RoutingDomainOutboundStage::NeedsPublishedPeerInfo
            | RoutingDomainOutboundStage::NeedsMoreTestedNodes => false,
            RoutingDomainOutboundStage::NeedsSafetyRoutes
            | RoutingDomainOutboundStage::ReadyToOperate => true,
        }
    }

    pub async fn tick(&self) -> EyreResult<()> {
        let state = self.state();
        let nodes_needed = self.nodes_needed();

        // Log stage transitions for attach-time diagnostics
        {
            let mut last = self.last_observed_stages.lock();
            let cur = (state.outbound_stage, state.inbound_stage);
            if last.as_ref() != Some(&cur) {
                veilid_log!(self debug "[PublicInternet] stage: outbound={:?} inbound={:?}", cur.0, cur.1);
                *last = Some(cur);
            }
        }

        // public_internet_ready should stay true once true on a stable network; warn if it flaps
        let ready = state.is_ready_inbound && state.is_ready_outbound;
        let opt_penalty = self
            .readiness_flap_detector
            .lock()
            .record(Timestamp::now().as_u64(), ready);
        if let Some(penalty) = opt_penalty {
            veilid_log!(self debugwarn "[PublicInternet] public_internet_ready FLAPPING (penalty={:.1}): now ready={} (outbound={:?} inbound={:?})",
                penalty, ready, state.outbound_stage, state.inbound_stage);
        }

        // The selected relay set should be stable on a stable network; warn if it churns
        let relays = self.read_dyn().relays();
        let relay_set: BTreeSet<NodeId> =
            relays.iter().map(|r| r.relay_node.best_node_id()).collect();
        let opt_relay_penalty = self
            .relay_flap_detector
            .lock()
            .record(Timestamp::now().as_u64(), relay_set);
        if let Some(penalty) = opt_relay_penalty {
            veilid_log!(self debugwarn "[PublicInternet] relay set FLAPPING (penalty={:.1})", penalty);
        }

        // Route validity (safety_routes_ready) should be stable once established; warn if it churns
        let safety_ready = self.safety_routes_ready();
        let opt_route_penalty = self
            .route_flap_detector
            .lock()
            .record(Timestamp::now().as_u64(), safety_ready);
        if let Some(penalty) = opt_route_penalty {
            veilid_log!(self debugwarn "[PublicInternet] safety_routes_ready FLAPPING (penalty={:.1}): now ready={}", penalty, safety_ready);
        }

        if Self::check_bootstrap(&state) {
            self.bootstrap_task.tick().await?;
        }
        if Self::check_peer_minimum_refresh(&state, &nodes_needed) {
            self.peer_minimum_refresh_task.tick().await?;
        }
        if Self::check_closest_peers_refresh(&state) {
            self.closest_peers_refresh_task.tick().await?;
        }
        if Self::check_ping_validator_public_internet(&state) {
            self.ping_validator_public_internet_task.tick().await?;
        }
        if Self::check_relay_management(&state) {
            self.relay_management_task.tick().await?;
        }
        if Self::check_private_route_management(&state) {
            self.private_route_management_task.tick().await?;
        }

        Ok(())
    }

    pub async fn commit_event_handler(&self, evt: Arc<RoutingDomainCommitEvent>) {
        if evt.routing_domain != RoutingDomain::PublicInternet {
            return;
        }
        if !(evt.confirmation_changed || evt.dial_info_changed || evt.relays_changed) {
            return;
        }
        let state = self.state();
        if Self::check_relay_management(&state) {
            if let Err(e) = self.relay_management_task.tick().await {
                veilid_log!(self warn "[PublicInternet] relay_management tick failed: {}", e);
            }
        }
    }

    pub async fn peer_info_change_event_handler(&self, evt: Arc<PeerInfoChangeEvent>) {
        if evt.routing_domain != RoutingDomain::PublicInternet || evt.opt_new_peer_info.is_none() {
            return;
        }
        let state = self.state();
        let nodes_needed = self.nodes_needed();
        if Self::check_peer_minimum_refresh(&state, &nodes_needed) {
            if let Err(e) = self.peer_minimum_refresh_task.tick().await {
                veilid_log!(self warn "[PublicInternet] peer_minimum_refresh tick failed: {}", e);
            }
        }
        if Self::check_closest_peers_refresh(&state) {
            if let Err(e) = self.closest_peers_refresh_task.tick().await {
                veilid_log!(self warn "[PublicInternet] closest_peers_refresh tick failed: {}", e);
            }
        }
        if Self::check_ping_validator_public_internet(&state) {
            if let Err(e) = self.ping_validator_public_internet_task.tick().await {
                veilid_log!(self warn "[PublicInternet] ping_validator tick failed: {}", e);
            }
        }
        if Self::check_private_route_management(&state) {
            if let Err(e) = self.private_route_management_task.tick().await {
                veilid_log!(self warn "[PublicInternet] private_route_management tick failed: {}", e);
            }
        }
    }

    pub async fn cancel_tasks(&self) {
        veilid_log!(self debug "stopping PublicInternet bootstrap task");
        if let Err(e) = self.bootstrap_task.stop().await {
            veilid_log!(self warn "bootstrap_task not stopped: {}", e);
        }

        veilid_log!(self debug "stopping PublicInternet peer minimum refresh task");
        if let Err(e) = self.peer_minimum_refresh_task.stop().await {
            veilid_log!(self warn "peer_minimum_refresh_task not stopped: {}", e);
        }

        veilid_log!(self debug "stopping PublicInternet closest peers refresh task");
        if let Err(e) = self.closest_peers_refresh_task.stop().await {
            veilid_log!(self warn "closest_peers_refresh_task not stopped: {}", e);
        }

        veilid_log!(self debug "stopping PublicInternet ping_validator tasks");
        if let Err(e) = self.ping_validator_public_internet_task.stop().await {
            veilid_log!(self warn "ping_validator_public_internet_task not stopped: {}", e);
        }

        veilid_log!(self debug "stopping PublicInternet relay management task");
        if let Err(e) = self.relay_management_task.stop().await {
            veilid_log!(self warn "relay_management_task not stopped: {}", e);
        }
        veilid_log!(self debug "stopping PublicInternet private route management task");
        if let Err(e) = self.private_route_management_task.stop().await {
            veilid_log!(self warn "private_route_management_task not stopped: {}", e);
        }
    }
}
