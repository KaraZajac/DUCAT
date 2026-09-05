use super::*;

// Field accessors
pub(crate) trait NodeRefAccessorsTrait {
    fn entry(&self) -> Arc<BucketEntry>;
    fn sequencing(&self) -> Sequencing;
    fn routing_domain_set(&self) -> RoutingDomainSet;
    fn filter(&self) -> NodeRefFilter;
    fn take_filter(&mut self) -> NodeRefFilter;
    fn dial_info_filter(&self) -> DialInfoFilter;
}

// Operate on entry
pub(crate) trait NodeRefOperateTrait {
    fn operate<T, F>(&self, f: F) -> T
    where
        F: FnOnce(&BucketEntryInner) -> T;
    fn operate_mut<T, F>(&self, f: F) -> T
    where
        F: FnOnce(&mut BucketEntryInner) -> T;
}

// Common Operations
pub(crate) trait NodeRefCommonTrait:
    NodeRefAccessorsTrait + NodeRefOperateTrait + VeilidComponentRegistryAccessor
{
    fn same_entry<T: NodeRefAccessorsTrait + ?Sized>(&self, other: &T) -> bool {
        Arc::ptr_eq(&self.entry(), &other.entry())
    }

    fn same_bucket_entry(&self, entry: &Arc<BucketEntry>) -> bool {
        Arc::ptr_eq(&self.entry(), entry)
    }

    fn equivalent<T: NodeRefAccessorsTrait + ?Sized>(&self, other: &T) -> bool {
        self.same_entry(other)
            && self.filter() == other.filter()
            && self.sequencing() == other.sequencing()
    }

    fn node_ids(&self) -> NodeIdGroup {
        self.operate(|e| e.node_ids())
    }
    fn public_keys(&self, routing_domain: RoutingDomain) -> PublicKeyGroup {
        self.operate(|e| e.public_keys(routing_domain))
    }
    fn best_node_id(&self) -> NodeId {
        self.operate(|e| e.best_node_id())
    }
    fn best_public_key(&self, routing_domain: RoutingDomain) -> Option<PublicKey> {
        self.operate(|e| e.best_public_key(routing_domain))
    }

    fn update_node_status(&self, routing_domain: RoutingDomain, node_status: NodeStatus) {
        self.operate_mut(|e| {
            e.update_node_status(routing_domain, node_status);
        });
    }
    fn best_routing_domain(&self) -> Option<RoutingDomain> {
        self.operate(|e| {
            let routing_table = self.routing_table();
            e.best_routing_domain(&routing_table, self.routing_domain_set())
        })
    }
    fn add_envelope_version(&self, envelope_version: EnvelopeVersion) {
        self.operate_mut(|e| e.add_envelope_version(envelope_version))
    }
    fn best_envelope_version(&self) -> Option<EnvelopeVersion> {
        self.operate(|e| e.best_envelope_version())
    }
    fn state_reason(&self, cur_ts: Timestamp) -> BucketEntryStateReason {
        self.operate(|e| e.state_reason(cur_ts))
    }
    fn state(&self, cur_ts: Timestamp) -> BucketEntryState {
        self.operate(|e| e.state(cur_ts))
    }
    fn peer_stats(&self) -> PeerStats {
        self.operate(|e| e.peer_stats().clone())
    }
    fn rpc_stats(&self) -> RPCStats {
        self.operate(|e| e.rpc_stats().clone())
    }
    fn connection_stats(&self) -> ConnectionStats {
        self.operate(|e| e.connection_stats().clone())
    }

    fn get_peer_info(&self, routing_domain: RoutingDomain) -> Option<Arc<PeerInfo>> {
        self.operate(|e| e.get_peer_info(routing_domain))
    }
    fn node_info(&self, routing_domain: RoutingDomain) -> Option<NodeInfo> {
        self.operate(|e| e.node_info(routing_domain).cloned())
    }
    fn peer_info_has_valid_signature(&self, routing_domain: RoutingDomain) -> bool {
        self.operate(|e| {
            e.get_peer_info(routing_domain)
                .map(|pi| !pi.signatures().is_empty())
                .unwrap_or(false)
        })
    }
    fn node_info_ts(&self, routing_domain: RoutingDomain) -> Timestamp {
        self.operate(|e| {
            e.node_info(routing_domain)
                .map(|ni| ni.timestamp())
                .unwrap_or(0u64.into())
        })
    }
    fn has_seen_our_node_info_ts(&self, routing_domain: RoutingDomain) -> bool {
        self.operate(|e| {
            let routing_table = self.routing_table();
            let Some(our_node_info_ts) = routing_table
                .get_published_peer_info(routing_domain)
                .map(|pi| pi.node_info().timestamp())
            else {
                return false;
            };
            e.has_seen_our_node_info_ts(routing_domain, our_node_info_ts)
        })
    }
    fn set_seen_our_node_info_ts(
        &self,
        routing_domain: RoutingDomain,
        seen_ts: Timestamp,
    ) -> Option<Timestamp> {
        self.operate_mut(|e| e.set_seen_our_node_info_ts(routing_domain, seen_ts))
    }
    // DialInfo
    fn first_dial_info_detail(&self) -> Option<DialInfoDetail> {
        let routing_domain_set = self.routing_domain_set();
        let dial_info_filter = self.dial_info_filter();
        let sequencing = self.sequencing();
        let (ordering, dial_info_filter) = dial_info_filter.apply_sequencing(sequencing);
        let sort = DialInfoDetail::get_ordering_sort(ordering);

        if dial_info_filter.is_dead() {
            return None;
        }

        let filter = |did: &DialInfoDetail| did.matches_filter(&dial_info_filter);

        self.operate(|e| {
            for routing_domain in routing_domain_set {
                if let Some(ni) = e.node_info(routing_domain) {
                    if let Some(did) = ni.first_filtered_dial_info_detail(sort.as_deref(), &filter)
                    {
                        return Some(did);
                    }
                }
            }
            None
        })
    }

    fn dial_info_details(&self) -> Vec<DialInfoDetail> {
        let routing_domain_set = self.routing_domain_set();
        let dial_info_filter = self.dial_info_filter();
        let sequencing = self.sequencing();
        let (ordering, dial_info_filter) = dial_info_filter.apply_sequencing(sequencing);
        let sort = DialInfoDetail::get_ordering_sort(ordering);

        let mut out = Vec::new();

        if dial_info_filter.is_dead() {
            return out;
        }

        let filter = |did: &DialInfoDetail| did.matches_filter(&dial_info_filter);

        self.operate(|e| {
            for routing_domain in routing_domain_set {
                if let Some(ni) = e.node_info(routing_domain) {
                    let mut dids = ni.filtered_dial_info_details(sort.as_deref(), &filter);
                    out.append(&mut dids);
                }
            }
        });
        out.remove_duplicates();
        out
    }

    /// Get the most recent 'last connection' to this node matching the node ref filter
    /// Filtered first and then sorted by ordering preference and then by most recent
    fn last_flow(&self) -> Option<Flow> {
        self.operate(|e| {
            // apply sequencing to filter and get sort
            let routing_table = self.routing_table();
            let sequencing = self.sequencing();
            let filter = self.filter();
            let (ordering, filter) = filter.apply_sequencing(sequencing);
            let mut last_flows = e.last_flows(&routing_table, true, filter);

            if let Some(sort) = ProtocolType::get_ordering_sort(ordering) {
                last_flows
                    .sort_unstable_by(|a, b| sort(&a.0.protocol_type(), &b.0.protocol_type()));
            }

            last_flows.first().map(|x| x.0)
        })
    }

    /// Get all the 'last connection' flows for this node matching the node ref filter
    /// Filtered first and then sorted by ordering preference and then by most recent
    #[expect(dead_code)]
    fn last_flows(&self) -> Vec<Flow> {
        self.operate(|e| {
            let routing_table = self.routing_table();
            // apply sequencing to filter and get sort
            let sequencing = self.sequencing();
            let filter = self.filter();
            let (ordering, filter) = filter.apply_sequencing(sequencing);
            let mut last_flows = e.last_flows(&routing_table, true, filter);

            if let Some(sort) = ProtocolType::get_ordering_sort(ordering) {
                last_flows
                    .sort_unstable_by(|a, b| sort(&a.0.protocol_type(), &b.0.protocol_type()));
            }

            last_flows.into_iter().map(|x| x.0).collect()
        })
    }

    /// Get the most recent 'last connection' flow whose remote matches the given address.
    /// Prefers an exact (IP, port) match. Falls back to any flow with the same remote IP,
    /// which covers incoming connections from a peer (ephemeral source port) where exact
    /// port match would miss. Relay-mediated flows have a different remote IP and don't match.
    #[expect(dead_code)]
    fn last_flow_to(&self, remote: SocketAddress) -> Option<Flow> {
        self.operate(|e| {
            let routing_table = self.routing_table();
            let sequencing = self.sequencing();
            let filter = self.filter();
            let (_ordering, filter) = filter.apply_sequencing(sequencing);
            let mut last_flows = e.last_flows(&routing_table, true, filter);
            last_flows.retain(|(flow, _)| flow.remote_address().address() == remote.address());
            // Prefer exact port match
            if let Some(exact) = last_flows
                .iter()
                .find(|(flow, _)| flow.remote_address() == &remote)
            {
                return Some(exact.0);
            }
            last_flows.first().map(|x| x.0)
        })
    }

    fn clear_last_flows(&self) {
        self.operate_mut(|e| e.clear_last_flows(self.dial_info_filter()))
    }

    fn set_last_flow(&self, flow: Flow, ts: Timestamp) {
        let (best_node_id, flow) = self.operate_mut(|e| {
            e.set_last_flow(flow, ts);
            let best_node_id = e.best_node_id();
            (best_node_id, flow)
        });

        self.routing_table().touch_recent_peer(best_node_id, flow);
    }

    fn clear_last_flow(&self, flow: Flow) {
        self.operate_mut(|e| {
            e.remove_last_flow(flow);
        })
    }

    fn is_relaying(&self, routing_domain: RoutingDomain) -> bool {
        self.operate(|e| {
            let routing_table = self.routing_table();
            let Some(relay_ids) = e
                .node_info(routing_domain)
                .map(|node_info| node_info.relay_ids())
            else {
                return false;
            };
            let our_node_ids = routing_table.node_ids();
            our_node_ids.contains_any_from_iter(relay_ids.iter())
        })
    }

    fn has_any_dial_info(&self) -> bool {
        self.operate(|e| {
            for rtd in RoutingDomain::all() {
                if let Some(ni) = e.node_info(rtd) {
                    if ni.has_any_dial_info() {
                        return true;
                    }
                }
            }
            false
        })
    }

    fn record_protected_connection_drop(&self, lifetime: TimestampDuration) {
        self.operate_mut(|e| e.record_protected_connection_drop(lifetime));
    }

    fn record_protected_connection_dead(&self, transport: TransportType) {
        self.stats_failed_to_send(Timestamp::now_non_decreasing(), false, transport);
    }

    fn report_failed_route_test(&self, transport: TransportType) {
        self.stats_failed_to_send(Timestamp::now_non_decreasing(), false, transport);
    }

    fn report_contact_method_result(&self, cm: &ContactMethod, success: bool) {
        self.operate_mut(|e| {
            e.report_contact_method_result(cm, success);
        })
    }

    fn get_contact_method_failure_ts(&self, cm: &ContactMethod) -> Option<Timestamp> {
        self.operate(|e| e.get_contact_method_failure_ts(cm))
    }

    fn stats_question_sent(
        &self,
        ts: Timestamp,
        bytes: ByteCount,
        expects_answer: bool,
        transport: TransportType,
    ) {
        self.operate_mut(|e| {
            self.routing_table().record_sent_bytes(bytes);
            e.question_sent(ts, bytes, expects_answer, transport);
        })
    }
    fn stats_question_rcvd(&self, ts: Timestamp, bytes: ByteCount, transport: TransportType) {
        self.routing_table().record_received_bytes(bytes);
        self.operate_mut(|e| {
            e.question_rcvd(ts, bytes, transport);
        })
    }
    fn stats_answer_sent(&self, bytes: ByteCount, transport: TransportType) {
        self.routing_table().record_sent_bytes(bytes);
        self.operate_mut(|e| {
            e.answer_sent(bytes, transport);
        })
    }
    fn stats_answer_rcvd(
        &self,
        send_ts: Timestamp,
        recv_ts: Timestamp,
        bytes: ByteCount,
        transport: TransportType,
    ) {
        self.operate_mut(|e| {
            self.routing_table().record_received_bytes(bytes);
            self.routing_table()
                .record_latency(recv_ts.duration_since(send_ts));
            e.answer_rcvd(send_ts, recv_ts, bytes, transport);
        })
    }
    fn stats_lost_question(&self, transport: TransportType) {
        self.operate_mut(|e| {
            e.lost_question(transport);
        })
    }
    fn stats_routed_up(&self, key: PublicKey, bytes: ByteCount) {
        self.operate_mut(|e| e.record_routed_up(key, bytes))
    }
    fn stats_routed_round_trip(
        &self,
        key: PublicKey,
        send_ts: Timestamp,
        recv_ts: Timestamp,
        bytes: ByteCount,
    ) {
        self.operate_mut(|e| e.record_routed_round_trip(key, send_ts, recv_ts, bytes))
    }
    fn stats_failed_to_send(&self, ts: Timestamp, expects_answer: bool, transport: TransportType) {
        self.operate_mut(|e| {
            e.failed_to_send(ts, expects_answer, transport);
        })
    }
    fn stats_unreachable(&self) {
        self.operate_mut(|e| {
            e.unreachable();
        })
    }
    fn report_sender_info(
        &self,
        routing_domain: RoutingDomain,
        unique_flow: UniqueFlow,
        sender_info: SenderInfo,
    ) -> Option<SenderInfo> {
        self.operate_mut(|e| {
            e.report_sender_info(
                LastSenderInfoKey {
                    routing_domain,
                    transport: unique_flow.flow.transport_type(),
                },
                sender_info,
            )
        })
    }

    fn set_punished(&self, reason: Option<PunishmentReason>) {
        self.operate_mut(|e| e.set_punished(reason));
    }
}
