mod editor;
mod local_network;
mod public_internet;
mod relay_requirements;
mod routing_domain_controller;
mod routing_domain_detail;
mod routing_domain_detail_common;
mod routing_domain_low_water_mark;
mod routing_domain_relay;
mod routing_domain_state;

use super::*;

pub use editor::*;
pub use local_network::*;
pub use public_internet::*;
pub use relay_requirements::*;
pub use routing_domain_controller::*;
pub use routing_domain_detail::*;
pub use routing_domain_detail_common::*;
pub use routing_domain_low_water_mark::*;
pub use routing_domain_relay::*;
pub use routing_domain_state::*;

impl_veilid_log_facility!("rtab");

impl RoutingTable {
    pub fn get_routing_domain_controllers(
        &self,
        routing_domain_set: RoutingDomainSet,
    ) -> Vec<RoutingDomainControllerGuard<'_>> {
        self.routing_domains
            .lock()
            .iter()
            .filter_map(|(k, v)| {
                routing_domain_set
                    .contains(*k)
                    .then_some(RoutingDomainControllerGuard::new(v.clone()))
            })
            .collect()
    }

    pub fn get_routing_domain_controller(
        &self,
        routing_domain: RoutingDomain,
    ) -> RoutingDomainControllerGuard<'_> {
        self.routing_domains
            .lock()
            .get(&routing_domain)
            .map(|v| RoutingDomainControllerGuard::new(v.clone()))
            .expect_or_log(&format!("routing domain not found: {}", routing_domain))
    }

    pub fn get_specific_routing_domain_controller<C: SpecificRoutingDomainController>(
        &self,
    ) -> SpecificRoutingDomainControllerGuard<'_, C> {
        self.routing_domains
            .lock()
            .get(&C::ROUTING_DOMAIN)
            .map(|v| {
                SpecificRoutingDomainControllerGuard::new(
                    Arc::downcast::<C>(v.clone()).expect_or_log(&format!(
                        "routing domain not a specific routing domain controller: {}",
                        C::ROUTING_DOMAIN
                    )),
                )
            })
            .expect_or_log(&format!("routing domain not found: {}", C::ROUTING_DOMAIN))
    }

    pub fn routing_domain_for_address(&self, address: Address) -> Option<RoutingDomain> {
        self.routing_domains
            .lock()
            .iter()
            .find_map(|(k, v)| v.read_dyn().can_contain_address(address).then_some(*k))
    }

    /// Select the most specific compatible routing domain for a flow.
    ///
    /// Local-address handling for ranges that don't represent real network membership
    /// (translator endpoints, transitional encapsulation, fallbacks) defers to the remote:
    /// - `is_unspecified` (`0.0.0.0` / `::`)
    /// - `is_synthetic_local` (CLAT46, CGNAT, NAT64-well-known, Teredo, 6to4)
    /// - APIPA `169.254/16` when remote is global (DHCP failed but route exists)
    /// - IPv6 link-local `fe80::/10` when remote is global
    ///
    /// Otherwise, pick the most specific domain compatible with both ends.
    pub fn routing_domain_for_flow(&self, flow: Flow) -> Option<RoutingDomain> {
        let remote_address = flow.remote_address().address();
        let rd_remote = self.routing_domain_for_address(remote_address)?;
        let Some(local_address) = flow.local().map(|x| x.address()) else {
            return Some(rd_remote);
        };
        if local_address.is_unspecified() || local_address.is_synthetic_local() {
            return Some(rd_remote);
        }
        if local_address.is_ipv4_link_local() && !remote_address.is_ipv4_link_local() {
            return Some(rd_remote);
        }
        if local_address.is_ipv6_link_local() && !remote_address.is_ipv6_link_local() {
            return Some(rd_remote);
        }
        let rd_local = self.routing_domain_for_address(local_address)?;

        // If the local and remote domains are the same, then return that domain
        if rd_local == rd_remote {
            return Some(rd_local);
        }

        // Get the domain details for the local and remote domains
        let local_origin_routing_domains = self
            .get_routing_domain_controller(rd_local)
            .read_dyn()
            .origin_routing_domains();
        let remote_origin_routing_domains = self
            .get_routing_domain_controller(rd_remote)
            .read_dyn()
            .origin_routing_domains();

        // Check if the local routing domain can accept flows from the remote domain, if so, pick the most specific domain (greatest in preference order)
        let opt_local_rd = local_origin_routing_domains
            .contains(rd_remote)
            .then_some(rd_local);
        let opt_remote_rd = remote_origin_routing_domains
            .contains(rd_local)
            .then_some(rd_remote);

        match (opt_local_rd, opt_remote_rd) {
            (Some(local_rd), Some(remote_rd)) => Some(local_rd.max(remote_rd)),
            (Some(local_rd), None) => Some(local_rd),
            (None, Some(remote_rd)) => Some(remote_rd),
            (None, None) => None,
        }
    }

    pub fn relays(&self, domain: RoutingDomain) -> Vec<RoutingDomainRelay> {
        self.get_routing_domain_controller(domain)
            .read_dyn()
            .relays()
    }

    pub fn dial_info_details(&self, domain: RoutingDomain) -> Vec<DialInfoDetail> {
        self.get_routing_domain_controller(domain)
            .read_dyn()
            .dial_info_details()
            .clone()
    }

    #[expect(dead_code)]
    pub fn routing_domain_debug(&self, domain: RoutingDomain, alt: bool) -> String {
        self.get_routing_domain_controller(domain)
            .read_dyn()
            .debug(alt)
    }

    #[expect(dead_code)]
    pub fn all_filtered_dial_info_details(
        &self,
        routing_domain_set: RoutingDomainSet,
        filter: &DialInfoFilter,
    ) -> Vec<DialInfoDetail> {
        let mut ret = Vec::new();
        if filter.is_dead() || routing_domain_set.is_empty() {
            return ret;
        }
        for rdd in self.get_routing_domain_controllers(routing_domain_set) {
            let rdd = rdd.read_dyn();
            for did in rdd.dial_info_details() {
                if did.matches_filter(filter) {
                    ret.push(did.clone());
                }
            }
        }
        ret.remove_duplicates();
        ret
    }

    pub fn get_node_info_routing_domains(&self, node_info: &NodeInfo) -> RoutingDomainSet {
        let mut valid_routing_domain_set = RoutingDomainSet::new();

        'skip: for rdd in self.get_routing_domain_controllers(RoutingDomainSet::all()) {
            let rdd = rdd.read_dyn();
            let mut rd_valid = false;

            for did in node_info.dial_info_detail_list() {
                let valid = rdd.ensure_dial_info_is_valid(&did.dial_info);

                if !valid {
                    continue 'skip;
                }
                rd_valid = true;
            }

            // Ensure the relay is also valid in this routing domain if it is provided
            for ri in node_info.relay_info_list() {
                for did in ri.dial_info_detail_list() {
                    let valid = rdd.ensure_dial_info_is_valid(&did.dial_info);
                    if !valid {
                        continue 'skip;
                    }
                    rd_valid = true;
                }
            }

            if rd_valid {
                valid_routing_domain_set.insert(rdd.routing_domain());
            }
        }

        // Return the valid routing domains
        valid_routing_domain_set
    }
    pub fn origin_routing_domains(&self, routing_domain: RoutingDomain) -> RoutingDomainSet {
        let rdd = self.get_routing_domain_controller(routing_domain);
        let rdd = rdd.read_dyn();
        rdd.origin_routing_domains()
    }

    /// Look up the best way for two nodes to reach each other over a specific routing domain
    pub fn get_best_contact_method(
        &self,
        routing_domain: RoutingDomain,
        contact_method_request: ContactMethodRequest,
    ) -> Option<ContactMethod> {
        let rdc = self.get_routing_domain_controller(routing_domain);
        rdc.get_best_contact_method(contact_method_request)
    }

    /// Look up all the ways for two nodes to reach each other over a specific routing domain
    pub fn get_contact_methods(
        &self,
        routing_domain: RoutingDomain,
        contact_method_request: ContactMethodRequest,
    ) -> Vec<ContactMethod> {
        let rdc = self.get_routing_domain_controller(routing_domain);
        rdc.get_contact_methods(contact_method_request)
    }

    /// Get the current published peer info
    pub fn get_published_peer_info(&self, routing_domain: RoutingDomain) -> Option<Arc<PeerInfo>> {
        let rdc = self.get_routing_domain_controller(routing_domain);
        rdc.get_published_peer_info()
    }

    /// Return a copy of our node's current peerinfo (may not yet be published)
    pub fn get_current_peer_info(&self, routing_domain: RoutingDomain) -> Arc<PeerInfo> {
        let rdc = self.get_routing_domain_controller(routing_domain);
        let rdd = rdc.read_dyn();
        rdd.get_peer_info()
    }

    /// Return a list of the current valid bootstrap peers in a particular routing domain
    #[expect(dead_code)]
    pub fn get_bootstrap_peers(&self, routing_domain: RoutingDomain) -> Vec<NodeRef> {
        let rdc = self.get_routing_domain_controller(routing_domain);
        let rdd = rdc.read_dyn();
        rdd.get_bootstrap_peers()
    }

    /// Return the domain's filter for what we can receivein the form of a dial info filter
    pub fn get_inbound_dial_info_filter(&self, routing_domain: RoutingDomain) -> DialInfoFilter {
        let rdc = self.get_routing_domain_controller(routing_domain);
        let rdd = rdc.read_dyn();
        rdd.inbound_dial_info_filter()
    }

    /// Return the domain's filter for what we can receive in the form of a node ref filter
    #[expect(dead_code)]
    pub fn get_inbound_node_ref_filter(&self, routing_domain: RoutingDomain) -> NodeRefFilter {
        let dif = self.get_inbound_dial_info_filter(routing_domain);
        NodeRefFilter::new()
            .with_routing_domain(routing_domain)
            .with_dial_info_filter(dif)
    }

    /// Return the domain's filter for what we can send out in the form of a dial info filter
    pub fn get_outbound_dial_info_filter(&self, routing_domain: RoutingDomain) -> DialInfoFilter {
        let rdc = self.get_routing_domain_controller(routing_domain);
        let rdd = rdc.read_dyn();
        rdd.outbound_dial_info_filter()
    }
    /// Return the domain's filter for what we can receive in the form of a node ref filter
    pub fn get_outbound_node_ref_filter(&self, routing_domain: RoutingDomain) -> NodeRefFilter {
        let dif = self.get_outbound_dial_info_filter(routing_domain);
        NodeRefFilter::new()
            .with_routing_domain(routing_domain)
            .with_dial_info_filter(dif)
    }
}
