use super::*;

impl_veilid_log_facility!("rtab");

#[derive(Debug)]
enum RoutingDomainChangePublicInternet {
    SetInterfaceAddresses { interface_addresses: Vec<IfAddr> },
    Common(RoutingDomainChange),
}

pub struct RoutingDomainEditorPublicInternet<'a> {
    controller: &'a PublicInternetRoutingDomainController,
    changes: Vec<RoutingDomainChangePublicInternet>,
}

impl<'a> VeilidComponentRegistryAccessor for RoutingDomainEditorPublicInternet<'a> {
    fn registry(&self) -> VeilidComponentRegistry {
        self.controller.registry()
    }
}

impl<'a> RoutingDomainEditorPublicInternet<'a> {
    pub(in crate::routing_table) fn new(
        controller: &'a PublicInternetRoutingDomainController,
    ) -> Self {
        Self {
            controller,
            changes: Vec::new(),
        }
    }

    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
    pub fn set_interface_addresses(&mut self, interface_addresses: Vec<IfAddr>) -> &mut Self {
        self.changes
            .push(RoutingDomainChangePublicInternet::SetInterfaceAddresses {
                interface_addresses,
            });
        self
    }
}

impl<'a> RoutingDomainEditor for RoutingDomainEditorPublicInternet<'a> {
    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    fn clear_dial_info_details(
        &mut self,
        address_types: Option<AddressTypeSet>,
        protocol_types: Option<ProtocolTypeSet>,
    ) {
        self.changes.push(RoutingDomainChangePublicInternet::Common(
            RoutingDomainChange::ClearDialInfoDetails {
                address_types,
                protocol_types,
            },
        ));
    }
    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    fn set_relay_compilation(&mut self, relay_compilation: Option<RelayCompilation>) {
        self.changes.push(RoutingDomainChangePublicInternet::Common(
            RoutingDomainChange::SetRelayCompilation { relay_compilation },
        ));
    }
    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    fn set_relay_state(&mut self, relay_id: NodeId, state: RoutingDomainRelayState) {
        self.changes.push(RoutingDomainChangePublicInternet::Common(
            RoutingDomainChange::SetRelayState { relay_id, state },
        ));
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    fn add_dial_info_detail(&mut self, dial_info_detail: DialInfoDetail) {
        self.changes.push(RoutingDomainChangePublicInternet::Common(
            RoutingDomainChange::AddDialInfoDetail { dial_info_detail },
        ));
    }

    // #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    // fn remove_dial_info_detail(&mut self, dial_info_detail: DialInfoDetail) {
    //     self.changes.push(RoutingDomainChangePublicInternet::Common(
    //         RoutingDomainChange::RemoveDialInfoDetail { dial_info_detail },
    //     ));
    // }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    fn set_network_config(&mut self, network_config: RoutingDomainNetworkConfig) {
        self.changes.push(RoutingDomainChangePublicInternet::Common(
            RoutingDomainChange::SetNetworkConfig { network_config },
        ));
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    fn set_confirmed(&mut self, confirmed: bool) {
        self.changes.push(RoutingDomainChangePublicInternet::Common(
            RoutingDomainChange::SetConfirmed { confirmed },
        ));
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    fn commit(&mut self) {
        // No locking if we have nothing to do
        if self.changes.is_empty() {
            return;
        }
        let routing_table = self.routing_table();

        // Snapshot under the write lock; log/diff after release to avoid
        // BucketEntry lock acquisition while holding the controller lock.
        let Some((old_snapshot, new_snapshot, changed)) = ({
            let mut rdd = self.controller.write_dyn();
            let Some(detail) = rdd
                .as_any_mut()
                .downcast_mut::<PublicInternetRoutingDomainDetail>()
            else {
                veilid_log!(self error "Failed to downcast routing domain detail to PublicInternetRoutingDomainDetail");
                return;
            };

            let old_snapshot = CommitSnapshot {
                dial_info_details: detail.dial_info_details().clone(),
                relays: detail.relays(),
                outbound_protocols: detail.outbound_protocols(),
                inbound_protocols: detail.inbound_protocols(),
                address_types: detail.address_types(),
                capabilities: detail.capabilities(),
                confirmed: detail.confirmed(),
            };

            for change in self.changes.drain(..) {
                match change {
                    RoutingDomainChangePublicInternet::Common(common_change) => {
                        detail.apply_change(common_change);
                    }
                    RoutingDomainChangePublicInternet::SetInterfaceAddresses {
                        interface_addresses,
                    } => {
                        detail.set_interface_addresses(interface_addresses);
                    }
                }
            }

            let new_snapshot = CommitSnapshot {
                dial_info_details: detail.dial_info_details().clone(),
                relays: detail.relays(),
                outbound_protocols: detail.outbound_protocols(),
                inbound_protocols: detail.inbound_protocols(),
                address_types: detail.address_types(),
                capabilities: detail.capabilities(),
                confirmed: detail.confirmed(),
            };

            let changed = old_snapshot.diff(&new_snapshot);

            if changed.peer_info_changed {
                detail.invalidate();
            }

            Some((old_snapshot, new_snapshot, changed))
        }) else {
            return;
        };

        // Lock released. Format and emit log lines.
        let removed_dial_info = old_snapshot
            .dial_info_details
            .iter()
            .filter(|di| !new_snapshot.dial_info_details.contains(di))
            .collect::<Vec<_>>();
        if !removed_dial_info.is_empty() {
            veilid_log!(self info
                "[PublicInternet] removed dial info:\n{}",
                indent_all_string(removed_dial_info.to_multiline_string())
                    .strip_trailing_newline()
            );
        }
        let added_dial_info = new_snapshot
            .dial_info_details
            .iter()
            .filter(|di| !old_snapshot.dial_info_details.contains(di))
            .collect::<Vec<_>>();
        if !added_dial_info.is_empty() {
            veilid_log!(self info
                "[PublicInternet] added dial info:\n{}",
                indent_all_string(added_dial_info.to_multiline_string())
                    .strip_trailing_newline()
            );
        }
        if changed.relays_changed {
            veilid_log!(self info "[PublicInternet] relays changed: [{}] -> [{}]",
            old_snapshot.relays.iter().map(|x| format!("{:#}",x.relay_node)).collect::<Vec<_>>().join(","),
            new_snapshot.relays.iter().map(|x| format!("{:#}",x.relay_node)).collect::<Vec<_>>().join(","));
        }
        if changed.outbound_protocols_changed {
            veilid_log!(self info
                "[PublicInternet] changed network: outbound {:?}->{:?}",
                old_snapshot.outbound_protocols, new_snapshot.outbound_protocols
            );
        }
        if changed.inbound_protocols_changed {
            veilid_log!(self info
                "[PublicInternet] changed network: inbound {:?}->{:?}",
                old_snapshot.inbound_protocols, new_snapshot.inbound_protocols
            );
        }
        if changed.address_types_changed {
            veilid_log!(self info
                "[PublicInternet] changed network: address types {:?}->{:?}",
                old_snapshot.address_types, new_snapshot.address_types
            );
        }
        if changed.capabilities_changed {
            veilid_log!(self info
                "[PublicInternet] changed network: capabilities {:?}->{:?}",
                old_snapshot.capabilities, new_snapshot.capabilities
            );
        }
        if changed.confirmation_changed {
            veilid_log!(self info
                "[PublicInternet] changed confirmation: {:?}->{:?}",
                old_snapshot.confirmed, new_snapshot.confirmed
            );
        }

        if changed.peer_info_changed {
            // Allow signed node info updates at same timestamp for otherwise dead nodes if our network has changed
            routing_table
                .inner
                .write()
                .reset_all_updated_since_last_network_change();

            if let Err(e) = self.event_bus().post(RoutingDomainCommitEvent {
                routing_domain: RoutingDomain::PublicInternet,
                peer_info_changed: changed.peer_info_changed,
                confirmation_changed: changed.confirmation_changed,
                dial_info_changed: changed.dial_info_changed,
                relays_changed: changed.relays_changed,
            }) {
                veilid_log!(self warn "failed to post RoutingDomainCommitEvent: {}", e);
            }
        }
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    fn reset(&mut self) {
        self.clear_dial_info_details(None, None);
        self.set_relay_compilation(None);
        self.set_confirmed(false);
        self.commit();
    }
}
