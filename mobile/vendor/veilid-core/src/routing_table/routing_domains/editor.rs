use super::*;

impl_veilid_log_facility!("rtab");

pub trait RoutingDomainEditor {
    fn clear_dial_info_details(
        &mut self,
        address_types: Option<AddressTypeSet>,
        protocol_types: Option<ProtocolTypeSet>,
    );
    fn set_relay_compilation(&mut self, relay_compilation: Option<RelayCompilation>);
    fn set_relay_state(&mut self, relay_id: NodeId, state: RoutingDomainRelayState);
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
    fn add_dial_info_detail(&mut self, dial_info_detail: DialInfoDetail);
    // #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
    // fn remove_dial_info_detail(&mut self, dial_info_detail: DialInfoDetail);
    fn set_network_config(&mut self, network_config: RoutingDomainNetworkConfig);
    fn set_confirmed(&mut self, confirmed: bool);
    fn commit(&mut self);
    fn reset(&mut self);
}

pub(super) trait RoutingDomainDetailApplyChange {
    /// Make a change from the routing domain editor
    fn apply_change(&mut self, change: RoutingDomainChange);
}

impl<T: RoutingDomainDetailAccessors + ?Sized> RoutingDomainDetailApplyChange for T {
    /// Make a change from the routing domain editor
    fn apply_change(&mut self, change: RoutingDomainChange) {
        match change {
            RoutingDomainChange::ClearDialInfoDetails {
                address_types,
                protocol_types,
            } => {
                self.common_mut()
                    .clear_dial_info_details(address_types, protocol_types);
            }

            RoutingDomainChange::SetRelayCompilation { relay_compilation } => {
                self.common_mut().set_relay_compilation(relay_compilation)
            }
            RoutingDomainChange::SetRelayState { relay_id, state } => {
                self.common_mut().set_relay_state(relay_id, state)
            }

            RoutingDomainChange::AddDialInfoDetail { dial_info_detail } => {
                if !self.ensure_dial_info_is_valid(&dial_info_detail.dial_info) {
                    return;
                }

                self.common_mut()
                    .add_dial_info_detail(dial_info_detail.clone());
            }
            // RoutingDomainChange::RemoveDialInfoDetail { dial_info_detail } => {
            //     self.common_mut()
            //         .remove_dial_info_detail(dial_info_detail.clone());
            // }
            RoutingDomainChange::SetNetworkConfig { network_config } => {
                self.common_mut().set_network_config(network_config);
            }
            RoutingDomainChange::SetConfirmed { confirmed } => {
                self.common_mut().set_confirmed(confirmed);
            }
        }
    }
}

#[derive(Debug)]
pub(super) enum RoutingDomainChange {
    ClearDialInfoDetails {
        address_types: Option<AddressTypeSet>,
        protocol_types: Option<ProtocolTypeSet>,
    },
    SetRelayCompilation {
        relay_compilation: Option<RelayCompilation>,
    },
    SetRelayState {
        relay_id: NodeId,
        state: RoutingDomainRelayState,
    },
    AddDialInfoDetail {
        dial_info_detail: DialInfoDetail,
    },
    // #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), expect(dead_code))]
    // RemoveDialInfoDetail {
    //     dial_info_detail: DialInfoDetail,
    // },
    SetNetworkConfig {
        network_config: RoutingDomainNetworkConfig,
    },
    SetConfirmed {
        confirmed: bool,
    },
}

pub(super) struct CommitSnapshot {
    pub dial_info_details: Vec<DialInfoDetail>,
    pub relays: Vec<RoutingDomainRelay>,
    pub outbound_protocols: ProtocolTypeSet,
    pub inbound_protocols: ProtocolTypeSet,
    pub address_types: AddressTypeSet,
    pub capabilities: BTreeSet<VeilidCapability>,
    pub confirmed: bool,
}

pub(super) struct CommitChanged {
    pub dial_info_changed: bool,
    pub relays_changed: bool,
    pub outbound_protocols_changed: bool,
    pub inbound_protocols_changed: bool,
    pub address_types_changed: bool,
    pub capabilities_changed: bool,
    pub confirmation_changed: bool,
    pub peer_info_changed: bool,
}

impl CommitSnapshot {
    pub(super) fn diff(&self, new: &CommitSnapshot) -> CommitChanged {
        let dial_info_changed = self.dial_info_details != new.dial_info_details;
        let relays_changed = self.relays.len() != new.relays.len()
            || self
                .relays
                .iter()
                .zip(new.relays.iter())
                .any(|x| !x.0.relay_node.same_entry(&x.1.relay_node));
        let outbound_protocols_changed = self.outbound_protocols != new.outbound_protocols;
        let inbound_protocols_changed = self.inbound_protocols != new.inbound_protocols;
        let address_types_changed = self.address_types != new.address_types;
        let capabilities_changed = self.capabilities != new.capabilities;
        let confirmation_changed = self.confirmed != new.confirmed;
        let peer_info_changed = dial_info_changed
            || relays_changed
            || outbound_protocols_changed
            || inbound_protocols_changed
            || address_types_changed
            || capabilities_changed
            || confirmation_changed;
        CommitChanged {
            dial_info_changed,
            relays_changed,
            outbound_protocols_changed,
            inbound_protocols_changed,
            address_types_changed,
            capabilities_changed,
            confirmation_changed,
            peer_info_changed,
        }
    }
}
