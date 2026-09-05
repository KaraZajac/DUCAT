/// Detect DialInfo for the DialInfo for the PublicInternet RoutingDomain
use super::*;
use futures_util::stream::FuturesUnordered;
use stop_token::future::FutureExt as StopTokenFutureExt;

impl_veilid_log_facility!("net");

type InboundProtocolMap = HashMap<(AddressType, LowLevelProtocolType, u16), Vec<ProtocolType>>;

struct DialInfoConfirmationResult {
    external_address_types: AddressTypeSet,
    dial_info_details: Vec<DialInfoDetail>,
}

impl NativeNetwork {
    /// Get routing domains that need dial info confirmation
    pub(super) fn routing_domains_needing_confirm_dial_info(&self) -> RoutingDomainSet {
        let mut needs_confirm_dial_info = RoutingDomainSet::empty();

        // Check if the routing domain state itself requires dial info confirmation
        for routing_domain in RoutingDomain::all() {
            if self.routing_domain_needs_confirm_dial_info(routing_domain) {
                needs_confirm_dial_info.insert(routing_domain);
            }
        }

        needs_confirm_dial_info
    }

    /// Get a single routing domain's need for dial info confirmation
    pub(super) fn routing_domain_needs_confirm_dial_info(
        &self,
        routing_domain: RoutingDomain,
    ) -> bool {
        let routing_table = self.routing_table();

        // PublicInternet domain has specific requirements for dial info confirmation
        if matches!(routing_domain, RoutingDomain::PublicInternet) {
            return self.public_internet_wants_confirm_dial_info();
        }

        // All other domains need to be auto-confirmed if they are in the NeedsDialInfoConfirmation stage
        let rdc = routing_table.get_routing_domain_controller(routing_domain);
        let state = rdc.state();
        matches!(
            state.inbound_stage,
            RoutingDomainInboundStage::NeedsDialInfoConfirmation
        )
    }

    // Determine if we need to run the dial info confirmation for the PublicInternet domain
    // Considers if the routing table is capable of providing enough nodes to validate dial info
    // Also check if we want to review the dial info because we are outbound-only
    // Outbound-only nodes will frequently be symmetric NAT and not have a -consistent- public address
    // so we have to do checks to see if the address has -become- consistent.
    fn public_internet_wants_confirm_dial_info(&self) -> bool {
        // Suppress dial info confirmation if we are configured to only use relays for inbound connections
        let config = self.config();
        if config.network.privacy.require_inbound_relay {
            return false;
        }

        let routing_table = self.routing_table();

        let (routing_domain_inbound_stage, current_peer_info, entry_summary) = {
            let rdc = routing_table.get_routing_domain_controller(RoutingDomain::PublicInternet);
            let state = rdc.state();
            let rdd = rdc.read_dyn();
            (
                state.inbound_stage,
                rdd.get_peer_info(),
                rdd.get_entry_summary(),
            )
        };

        let (needs_confirm_dial_info, state_is_publishable) = match routing_domain_inbound_stage {
            RoutingDomainInboundStage::Invalid | RoutingDomainInboundStage::Unusable => {
                // Never tick if we haven't set up the network or the network is not usable
                return false;
            }
            RoutingDomainInboundStage::NeedsDialInfoConfirmation => {
                // Still need to confirm dial info
                (true, false)
            }
            RoutingDomainInboundStage::NeedsRelays | RoutingDomainInboundStage::ReadyToPublish => {
                // Already have confirmed dialinfo
                (false, true)
            }
        };

        if needs_confirm_dial_info
            || (state_is_publishable
                && !current_peer_info.node_info().has_dial_info()
                && self.inner.lock().next_outbound_only_dial_info_check <= Timestamp::now())
        {
            // Bootstrap needs to have gotten us enough connectivity nodes
            let mut has_enough_nodes = true;
            for ck in VALID_CRYPTO_KINDS {
                if entry_summary
                    .per_crypto_kind
                    .get(&ck)
                    .map(|cc| cc.live.connectivity_capabilities)
                    .unwrap_or_default()
                    < EXTERNAL_INFO_VALIDATIONS
                {
                    has_enough_nodes = false;
                    break;
                }
            }

            has_enough_nodes
        } else {
            false
        }
    }

    #[cfg_attr(feature = "instrument", instrument(parent = None, level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn confirm_generic_dial_info_task_routine(
        &self,
        _stop_token: StopToken,
        routing_domain: RoutingDomain,
        _l: Timestamp,
        _t: Timestamp,
    ) -> EyreResult<()> {
        // Network lock ensures only one task operating on the low level network state
        // can happen at the same time. This a blocking lock so we can ensure this runs
        // as soon as network_interfaces_task is finished
        let _guard = self.network_task_lock.read().await;

        // Check again here in case of race condition
        let needs_confirm_dial_info = self.routing_domain_needs_confirm_dial_info(routing_domain);
        if !needs_confirm_dial_info {
            return Ok(());
        }

        // Just confirm the dial info automatically
        let routing_table = self.routing_table();
        let rdc = routing_table.get_routing_domain_controller(routing_domain);
        let mut editor = rdc.edit_dyn();
        editor.set_confirmed(true);
        editor.commit();

        Ok(())
    }

    #[cfg_attr(feature = "instrument", instrument(parent = None, level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn confirm_public_internet_dial_info_task_routine(
        &self,
        stop_token: StopToken,
        l: Timestamp,
        t: Timestamp,
    ) -> EyreResult<()> {
        // Network lock ensures only one task operating on the low level network state
        // can happen at the same time. This a blocking lock so we can ensure this runs
        // as soon as network_interfaces_task is finished
        let _guard = self.network_task_lock.read().await;

        if !self.routing_domain_needs_confirm_dial_info(RoutingDomain::PublicInternet) {
            return Ok(());
        }

        // Skip while presumed offline. Validators won't be able to send us receipts,
        // so the discovery would just fail and pollute the dial-info confirm failure counter.
        if self
            .network_manager()
            .online_detector()
            .online_state(RoutingDomain::PublicInternet)
            == OnlineState::Offline
        {
            veilid_log!(self debug "Skipping dial-info confirmation (offline detected)");
            return Ok(());
        }

        let confirmed = self
            .do_dial_info_confirm_public_internet(stop_token, l, t)
            .await?;

        // Done with public dial info check
        if confirmed {
            let mut inner = self.inner.lock();

            // Don't try to re-do OutboundOnly dialinfo for another 10 seconds
            inner.next_outbound_only_dial_info_check = Timestamp::now().later(
                TimestampDuration::new_secs(UPDATE_OUTBOUND_ONLY_DIAL_INFO_PERIOD_SECS),
            )
        }

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn process_detected_dial_info(
        &self,
        dial_info_details: &mut Vec<DialInfoDetail>,
        ddi: DetectedDialInfo,
    ) {
        match ddi {
            DetectedDialInfo::SymmetricNAT => {}
            DetectedDialInfo::Detected(did) => {
                // We got a dialinfo, add it and tag us as inbound capable
                dial_info_details.push(did.clone());
            }
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn update_with_detection_result(
        &self,
        dial_info_details: &mut Vec<DialInfoDetail>,
        inbound_protocol_map: &InboundProtocolMap,
        dr: DetectionResult,
    ) {
        // Found some new dial info for this protocol/address combination
        self.process_detected_dial_info(dial_info_details, dr.ddi.clone());

        // Add additional dialinfo for protocols on the same port
        match &dr.ddi {
            DetectedDialInfo::SymmetricNAT => {}
            DetectedDialInfo::Detected(did) => {
                let ipmkey = (
                    did.dial_info.address_type(),
                    did.dial_info.protocol_type().low_level_protocol_type(),
                    dr.config.port,
                );
                if let Some(ipm) = inbound_protocol_map.get(&ipmkey) {
                    for additional_pt in ipm.iter().skip(1) {
                        // Make dialinfo for additional protocol type
                        let additional_ddi = DetectedDialInfo::Detected(DialInfoDetail {
                            dial_info: self
                                .make_dial_info(did.dial_info.socket_address(), *additional_pt),
                            class: did.class,
                        });
                        // Add additional dialinfo
                        self.process_detected_dial_info(dial_info_details, additional_ddi);
                    }
                }
            }
        }
    }

    async fn do_public_internet_dial_info_detection(
        &self,
        stop_token: StopToken,
        cur_ts: Timestamp,
        inbound_protocol_map: &InboundProtocolMap,
    ) -> EyreResult<Option<DialInfoConfirmationResult>> {
        // Process all protocol and address combinations
        let mut unord = FuturesUnordered::new();
        let mut context_configs = HashSet::new();
        for ((address_type, _llpt, port), protocols) in inbound_protocol_map.clone() {
            let protocol_type = *protocols.first().unwrap_or_log();
            let dcc = DiscoveryContextConfig {
                protocol_type,
                address_type,
                port,
            };
            context_configs.insert(dcc);
            let discovery_context = DiscoveryContext::new(self.clone(), dcc, stop_token.clone());
            unord.push(discovery_context.discover());
        }

        // Wait for all discovery futures to complete and apply discoverycontexts
        let mut external_address_types = AddressTypeSet::new();
        let mut detection_results = HashMap::<DiscoveryContextConfig, DetectionResult>::new();
        loop {
            match unord
                .next()
                .timeout_at(stop_token.clone())
                .in_current_span()
                .await
            {
                Ok(Some(Some(dr))) => {
                    // Got something for this config
                    context_configs.remove(&dr.config);

                    // Add the external address kinds to the set we've seen
                    external_address_types |= dr.external_address_types;

                    // Save best detection result for each discovery context config
                    detection_results.insert(dr.config, dr);
                }
                Ok(Some(None)) => {
                    // Found no dial info for this protocol/address combination
                }
                Ok(None) => {
                    // All done, normally
                    break;
                }
                Err(_) => {
                    // Stop token, exit early without error propagation
                    veilid_log!(self debug "Stopped public internet dial info detection");
                    return Ok(None);
                }
            }
        }

        // Apply best effort coalesced detection results
        let mut dial_info_details = Vec::new();

        for (_, dr) in detection_results {
            // Import the dialinfo
            self.update_with_detection_result(&mut dial_info_details, inbound_protocol_map, dr);
        }

        let done_duration = TimestampDuration::since_non_decreasing(cur_ts);

        // If we got no external address types, try again
        if external_address_types.is_empty() {
            veilid_log!(self debug "DialInfo discovery failed in {:#}, trying again, got no external address types", done_duration);
            return Ok(None);
        }

        // All done
        veilid_log!(self debug "DialInfo discovery finished in {:#} with address_types {:?}", done_duration, external_address_types);

        Ok(Some(DialInfoConfirmationResult {
            external_address_types,
            dial_info_details,
        }))
    }

    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn do_dial_info_confirm_public_internet(
        &self,
        stop_token: StopToken,
        _l: Timestamp,
        cur_ts: Timestamp,
    ) -> EyreResult<bool> {
        // Figure out if we can optimize TCP/WS checking since they are often on the same port
        let (protocol_config, static_dial_info_details, inbound_protocol_map) = {
            let inner = self.inner.lock();
            let Some(network_state) = inner.network_state.as_ref().cloned() else {
                bail!("should not be doing public dial info confirmation before we have an initial network state");
            };

            // Get the protocol config
            let protocol_config = network_state.protocol_config.clone();

            // Get the static dial info we don't want to change
            let static_dial_info_details =
                network_state.static_dial_info_details(RoutingDomain::PublicInternet);
            let static_di_types: BTreeSet<TransportType> = static_dial_info_details
                .iter()
                .map(|did| {
                    TransportType::new(did.dial_info.protocol_type(), did.dial_info.address_type())
                })
                .collect();

            let mut inbound_protocol_map =
                HashMap::<(AddressType, LowLevelProtocolType, u16), Vec<ProtocolType>>::new();

            for at in protocol_config.family_global {
                for pt in protocol_config.inbound {
                    // Skip transport types that already have static public dialinfo
                    // as they don't need to participate in discovery
                    let key = TransportType::new(pt, at);
                    if static_di_types.contains(&key) {
                        continue;
                    }

                    if let Some(pla) = inner.preferred_local_addresses.get(&key) {
                        let llpt = pt.low_level_protocol_type();
                        let itmkey = (at, llpt, pla.port());
                        inbound_protocol_map
                            .entry(itmkey)
                            .and_modify(|x| x.push(pt))
                            .or_insert_with(|| vec![pt]);
                    }
                }
            }

            (
                protocol_config,
                static_dial_info_details,
                inbound_protocol_map,
            )
        };

        // Save off existing public dial info for change detection later
        let routing_table = self.routing_table();

        // Get the last dial info details and last network config
        let last_network_config = {
            let rdc = routing_table.get_routing_domain_controller(RoutingDomain::PublicInternet);
            let rdd = rdc.read_dyn();
            rdd.network_config().clone()
        };

        // Set most permissive network config and start from scratch
        let rdc = routing_table
            .get_specific_routing_domain_controller::<PublicInternetRoutingDomainController>();
        let mut editor = rdc.edit();
        editor.set_network_config(RoutingDomainNetworkConfig::new(
            protocol_config.outbound,
            protocol_config.inbound,
            protocol_config.family_global,
            protocol_config.public_internet_capabilities.clone(),
        ));
        editor.commit();

        let opt_confirmation_result = self
            .do_public_internet_dial_info_detection(stop_token, cur_ts, &inbound_protocol_map)
            .await?;

        let Some(confirmation_result) = opt_confirmation_result else {
            // Revert to last network config
            editor.set_network_config(last_network_config);

            // Track consecutive confirmation failures so a single transient failure
            // does not cause us to unpublish working peer info
            let confirm_failure_count = {
                let mut inner = self.inner.lock();
                *inner
                    .dial_info_confirm_failure_count
                    .entry(RoutingDomain::PublicInternet)
                    .and_modify(|x| *x += 1)
                    .or_insert(1)
            };

            if confirm_failure_count >= MAX_DIAL_INFO_CONFIRM_FAILURE_COUNT {
                veilid_log!(self debug "DialInfo confirmation failed {} times consecutively for PublicInternet, unpublishing peer info", confirm_failure_count);
                editor.set_confirmed(false);
                editor.commit();
                rdc.unpublish_peer_info();
            } else {
                veilid_log!(self debug "DialInfo confirmation failed {} time(s) consecutively for PublicInternet, will retry before unpublishing", confirm_failure_count);
            }
            return Ok(false);
        };

        // Successful confirmation, reset the failure counter
        self.inner
            .lock()
            .dial_info_confirm_failure_count
            .remove(&RoutingDomain::PublicInternet);

        // Set the address types we've seen and confirm the dialinfo we just discovered
        editor.set_network_config(RoutingDomainNetworkConfig::new(
            protocol_config.outbound,
            protocol_config.inbound,
            confirmation_result.external_address_types,
            protocol_config.public_internet_capabilities,
        ));
        editor.clear_dial_info_details(None, None);
        for did in static_dial_info_details {
            editor.add_dial_info_detail(did);
        }
        for did in confirmation_result.dial_info_details {
            editor.add_dial_info_detail(did);
        }

        editor.set_confirmed(true);
        editor.commit();

        // Publish the peer info (idempotent if it hasn't changed)
        rdc.publish_peer_info();

        Ok(true)
    }

    /// Make a dialinfo from an address and protocol type
    pub fn make_dial_info(&self, addr: SocketAddress, protocol_type: ProtocolType) -> DialInfo {
        match protocol_type {
            ProtocolType::UDP => DialInfo::udp(addr),
            ProtocolType::TCP => DialInfo::tcp(addr),
            ProtocolType::WS => DialInfo::try_ws(
                addr,
                format!("ws://{}/{}", addr, self.config().network.protocol.ws.path),
            )
            .unwrap_or_log(),
            #[cfg(feature = "enable-protocol-wss")]
            ProtocolType::WSS => DialInfo::try_wss(
                addr,
                format!("wss://{}/{}", addr, self.config().network.protocol.wss.path),
            )
            .unwrap_or_log(),
        }
    }
}
