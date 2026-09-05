//! WASM equivalent of native confirm_dial_info: learn supported address families by
//! probing nodes proven live over another family. A failed cross-family probe is
//! tolerated, like native dial-info discovery.

use super::*;

/// Interval between polling for address family confirmation
const ADDR_FAMILY_POLL_INTERVAL_MS: u32 = 1000;
/// Minimum number of nodes to confirm an address family
const ADDR_FAMILY_MIN_NODES_TO_CONFIRM: usize = 4;
/// Maximum number of probes per family
const ADDR_FAMILY_MAX_PROBES: usize = 8;
/// Maximum number of failures to tolerate before declaring an address family unsupported
const ADDR_FAMILY_FAILURE_THRESHOLD: usize = 3;

/// Address family probe result
enum FamilyProbeDisposition {
    Supported,
    Unsupported,
    Inconclusive,
    Cancelled,
}

/// Address family confirmation state
enum AddressFamilyConfirmState {
    /// No address types are configured for this routing domain
    NoConfiguredAddressTypes,
    /// All configured address types have been confirmed
    AllConfiguredConfirmed,
    /// Not enough candidates to verify unconfirmed address type
    NotEnoughCandidates,
    /// Confirm candidates to determine final set of address types
    ConfirmCandidates,
}

/// Address family confirmation state
struct AddressFamilyConfirmCandidates {
    /// All address types configured for this routing domain
    configured_address_types: AddressTypeSet,
    /// All address types that have been positively seen since we started the confirmation process
    confirmed_address_types: AddressTypeSet,
    /// Nodes that can confirm an unconfirmed address type
    candidates: BTreeMap<AddressType, Vec<NodeRef>>,
}

impl AddressFamilyConfirmCandidates {
    /// Determine the current state of the address family confirmation process
    pub fn state(&self) -> AddressFamilyConfirmState {
        if self.configured_address_types.is_empty() {
            AddressFamilyConfirmState::NoConfiguredAddressTypes
        } else if self.confirmed_address_types == self.configured_address_types {
            AddressFamilyConfirmState::AllConfiguredConfirmed
        } else {
            let missing_address_types =
                self.configured_address_types - self.confirmed_address_types;
            for mat in missing_address_types {
                let candidates = self
                    .candidates
                    .get(&mat)
                    .as_ref()
                    .map(|c| c.len())
                    .unwrap_or(0);
                if candidates < ADDR_FAMILY_MIN_NODES_TO_CONFIRM {
                    return AddressFamilyConfirmState::NotEnoughCandidates;
                }
            }
            AddressFamilyConfirmState::ConfirmCandidates
        }
    }
}

impl WasmNetwork {
    // Reset address family confirmation state
    pub(super) fn reset_address_family_confirm_state(&self, routing_domain: RoutingDomain) -> bool {
        let configured_address_types = match routing_domain {
            RoutingDomain::PublicInternet => self.inner.lock().protocol_config.family_global,
            _ => return false,
        };

        let routing_table = self.routing_table();
        let rdc = routing_table.get_routing_domain_controller(routing_domain);
        let mut network_config = rdc.read_dyn().network_config().clone();
        network_config.address_types = configured_address_types;
        {
            let mut editor = rdc.edit_dyn();
            editor.set_network_config(network_config);
            editor.set_confirmed(false);

            self.inner
                .lock()
                .last_confirm_request_ts
                .insert(routing_domain, Timestamp::now_non_decreasing());

            editor.commit();
        }

        veilid_log!(self info "{:#} address families reset: {:?}", routing_domain, configured_address_types);
        true
    }

    // Apply address family verdict for routing domain
    fn confirm_address_families(
        &self,
        routing_domain: RoutingDomain,
        confirmed_address_families: AddressTypeSet,
    ) {
        let routing_table = self.routing_table();
        let rdc = routing_table.get_routing_domain_controller(routing_domain);
        let mut network_config = rdc.read_dyn().network_config().clone();
        network_config.address_types = confirmed_address_families;
        {
            let mut editor = rdc.edit_dyn();
            editor.set_network_config(network_config);
            editor.set_confirmed(true);
            editor.commit();
        }
        rdc.publish_peer_info();

        veilid_log!(self info "{:#} address families confirmed: {:?}", routing_domain, confirmed_address_families);
    }

    /// Start the address family confirmation worker and online/offline event listeners
    pub(super) fn start_address_family_confirm_worker(&self) {
        let stop_source = StopSource::new();
        let stop_token = stop_source.token();
        let this = self.clone();
        let jh = spawn("address family confirm", async move {
            this.address_family_confirm_worker(stop_token).await;
        });

        let opt_online_event_listener_jh = if let Some(online_event_listener) =
            GLOBAL_ONLINE_EVENT.as_ref()
        {
            let this = self.clone();
            let stop_token = stop_source.token();
            Some(spawn("global scope online event listener", async move {
                let online_event_receiver = online_event_listener.receiver();
                while let Ok(Ok(_event)) = online_event_receiver
                    .recv_async()
                    .timeout_at(stop_token.clone())
                    .await
                {
                    veilid_log!(this debug "Global scope online detected");

                    // Request address family reconfirmation
                    this.routing_domain_request_confirm_dial_info(RoutingDomain::PublicInternet);
                }
            }))
        } else {
            veilid_log!(self error "Failed to create global scope online event listener");
            None
        };
        let opt_offline_event_listener_jh = if let Some(offline_event_listener) =
            GLOBAL_OFFLINE_EVENT.as_ref()
        {
            let this = self.clone();
            let stop_token = stop_source.token();
            Some(spawn("global scope offline event listener", async move {
                let offline_event_receiver = offline_event_listener.receiver();
                while let Ok(Ok(_event)) = offline_event_receiver
                    .recv_async()
                    .timeout_at(stop_token.clone())
                    .await
                {
                    veilid_log!(this debug "Global scope offline detected");

                    // Kill off all connections
                    //
                    // Needed for Chrome + Firefox because they send offline/online when
                    // switching networks but don't kill connections immediately when offline
                    //
                    // Safari kills connections when switching networks but doesn't send
                    // online/offline unless no network is available.
                    this.network_manager()
                        .connection_manager()
                        .close_all_connections();

                    // Request address family reconfirmation
                    this.routing_domain_request_confirm_dial_info(RoutingDomain::PublicInternet);
                }
            }))
        } else {
            veilid_log!(self error "Failed to create global scope offline event listener");
            None
        };

        let mut inner = self.inner.lock();
        inner.address_family_stop_source = Some(stop_source);
        inner.address_family_worker_jh = Some(jh);
        inner.online_event_listener_jh = opt_online_event_listener_jh;
        inner.offline_event_listener_jh = opt_offline_event_listener_jh;
    }

    async fn address_family_confirm_worker(&self, stop_token: StopToken) {
        loop {
            if sleep(ADDR_FAMILY_POLL_INTERVAL_MS)
                .timeout_at(stop_token.clone())
                .await
                .is_err()
            {
                return;
            }

            // Operate on routing domains that are detecting address changes
            let routing_table = self.routing_table();
            for routing_domain in self.routing_domains_detecting_address_changes() {
                // Skip routing domains that are already confirmed
                let confirmed = {
                    let rdc = routing_table.get_routing_domain_controller(routing_domain);
                    let confirmed = rdc.read_dyn().confirmed();
                    confirmed
                };

                if confirmed {
                    continue;
                }

                if let Err(e) = self
                    .perform_address_family_confirmation_pass(routing_domain, &stop_token)
                    .await
                {
                    veilid_log!(self error "Failed to confirm address families for routing domain {:?}: {}", routing_domain, e);
                }
            }
        }
    }

    /// One confirmation pass: probe each allowed family, drop conclusively-unreachable ones,
    /// then set confirmed=true (mirrors native confirm completion)
    async fn perform_address_family_confirmation_pass(
        &self,
        routing_domain: RoutingDomain,
        stop_token: &StopToken,
    ) -> EyreResult<()> {
        // Collect candidate node refs before awaiting
        let address_family_confirm_candidates =
            self.collect_family_probe_candidates(routing_domain)?;

        match address_family_confirm_candidates.state() {
            AddressFamilyConfirmState::NoConfiguredAddressTypes => {
                veilid_log!(self debug "no address types configured for routing domain {:#}; confirming no address types", routing_domain);
                self.confirm_address_families(
                    routing_domain,
                    address_family_confirm_candidates.confirmed_address_types,
                );
                return Ok(());
            }
            AddressFamilyConfirmState::AllConfiguredConfirmed => {
                veilid_log!(self debug "all address types confirmed for routing domain {:#}; enabling all configuredaddress types", routing_domain);
                self.confirm_address_families(
                    routing_domain,
                    address_family_confirm_candidates.confirmed_address_types,
                );
                return Ok(());
            }
            AddressFamilyConfirmState::NotEnoughCandidates => {
                veilid_log!(self debug "not enough candidates to confirm address types for routing domain {:#}; skipping confirmation", routing_domain);
                return Ok(());
            }
            AddressFamilyConfirmState::ConfirmCandidates => {
                veilid_log!(self debug "Need to confirm address types {:?} for routing domain {:#}", address_family_confirm_candidates.candidates.keys(), routing_domain);
            }
        }

        // Keep optimistic; remove only families that fail conclusively (no candidates = keep)
        let mut confirmed_address_types = address_family_confirm_candidates.confirmed_address_types;
        for (at, cands) in address_family_confirm_candidates.candidates {
            if cands.len() < ADDR_FAMILY_MIN_NODES_TO_CONFIRM {
                veilid_log!(self error "not enough candidates to confirm address type {:#} for routing domain {:#}; skipping confirmation", at, routing_domain);
                continue;
            }
            match self
                .probe_address_family(routing_domain, at, cands.clone(), stop_token)
                .await
            {
                FamilyProbeDisposition::Unsupported => {
                    // Definitely not supported, don't add to confirmed set
                }
                FamilyProbeDisposition::Cancelled => {
                    // Bail out immediately because we got the stop token
                    return Ok(());
                }
                FamilyProbeDisposition::Inconclusive => {
                    // Shouldn't happen because we only get here if there weren't enough candidates
                    veilid_log!(self error "Inconculsive results confirming address type {:#} for routing domain {:#}; skipping confirmation", at, routing_domain);
                    continue;
                }
                FamilyProbeDisposition::Supported => {
                    // Record as confirmed
                    confirmed_address_types.insert(at);
                }
            }
        }

        // Confirm whatever we got from the probes
        self.confirm_address_families(routing_domain, confirmed_address_types);

        Ok(())
    }

    /// Per family, nodes with a WS/WSS dial info in that family that are live over another
    /// family (so a probe failure is our egress, not a dead node). Returns map + node count.
    fn collect_family_probe_candidates(
        &self,
        routing_domain: RoutingDomain,
    ) -> EyreResult<AddressFamilyConfirmCandidates> {
        // All possible configures address types we can confirm
        let configured_address_types = configured_address_type_set(&self.config());

        // List of candidates that can be pinged for positive confirmation of missing address types
        let mut candidates: BTreeMap<AddressType, Vec<NodeRef>> = BTreeMap::new();

        // All address types we have positively seen since the last confirm request
        let mut confirmed_address_types = AddressTypeSet::empty();

        // Only families reachable over a protocol we can actually dial outbound
        let (outbound_protocols, last_confirm_request_ts) = {
            let inner = self.inner.lock();

            let outbound_protocols = inner.protocol_config.outbound;
            let Some(last_confirm_request_ts) =
                inner.last_confirm_request_ts.get(&routing_domain).copied()
            else {
                // Should have had a request timestamp for this routing domain
                bail!(
                    "No last confirm request timestamp for routing domain {:?}",
                    routing_domain
                );
            };

            (outbound_protocols, last_confirm_request_ts)
        };

        // Make snapshot of all nodes in the routing table that have been seen consistently
        let cur_ts = Timestamp::now();
        let snapshot = self
            .routing_table()
            .snapshot_entries(cur_ts, BucketEntryState::Unreliable);

        for snap in snapshot.entries() {
            let Some(pi) = snap.get_peer_info(routing_domain) else {
                continue;
            };

            // Families we've received answers over since the last confirm request
            let mut snap_live_families = AddressTypeSet::empty();
            for (tt, st) in snap.per_transport.iter() {
                // Skip any address types that are not configured for this routing domain
                if !configured_address_types.contains(tt.address_type()) {
                    continue;
                }
                if let Some(last_seen_ts) = st.last_seen_ts.as_ref().copied() {
                    if last_seen_ts > last_confirm_request_ts {
                        // Record some node as having positive confirmation of this address type
                        confirmed_address_types.insert(tt.address_type());

                        // Also record for this node specifically
                        snap_live_families.insert(tt.address_type());
                    }
                }
            }

            let ni = pi.node_info();

            for at in configured_address_types {
                // Skip address type if no directly dialable address for it
                let has_dialable_in_at = ni.dial_info_detail_list().iter().any(|did| {
                    !did.class.requires_signal()
                        && did.dial_info.address_type() == at
                        && outbound_protocols.contains(did.dial_info.protocol_type())
                });
                if !has_dialable_in_at {
                    continue;
                }

                // If this node supports this address type but has already been seen live for it
                // then we can skip this address type
                if snap_live_families.contains(at) {
                    continue;
                }

                // Record ping candidate for this address type
                candidates
                    .entry(at)
                    .or_default()
                    .push(snap.node_ref.clone());
            }
        }

        // Remove any candidate address types that have already been confirmed
        for at in confirmed_address_types.iter() {
            candidates.remove(&at);
        }

        // Return the candidates
        Ok(AddressFamilyConfirmCandidates {
            configured_address_types,
            confirmed_address_types,
            candidates,
        })
    }

    /// >=1 answer => Supported; N controlled failures => Unsupported; else Inconclusive
    async fn probe_address_family(
        &self,
        routing_domain: RoutingDomain,
        at: AddressType,
        candidates: Vec<NodeRef>,
        stop_token: &StopToken,
    ) -> FamilyProbeDisposition {
        let mut failures = 0usize;
        for nr in candidates.into_iter().take(ADDR_FAMILY_MAX_PROBES) {
            let fnr = nr.custom_filtered(
                NodeRefFilter::new()
                    .with_routing_domain(routing_domain)
                    .with_address_type(at),
            );
            let res = match self
                .rpc_processor()
                .rpc_call_status(Destination::direct(fnr, None))
                .timeout_at(stop_token.clone())
                .await
            {
                Ok(res) => res,
                Err(_) => return FamilyProbeDisposition::Cancelled,
            };
            match res {
                Ok(StatusResult::Answer { .. }) => return FamilyProbeDisposition::Supported,
                _ => {
                    failures += 1;
                    if failures >= ADDR_FAMILY_FAILURE_THRESHOLD {
                        return FamilyProbeDisposition::Unsupported;
                    }
                }
            }
        }
        FamilyProbeDisposition::Inconclusive
    }
}
