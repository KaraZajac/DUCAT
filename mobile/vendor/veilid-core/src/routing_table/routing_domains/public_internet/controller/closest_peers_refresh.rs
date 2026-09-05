use super::*;

impl_veilid_log_facility!("rtab");

/// How many nodes to consult for closest peers simultaneously
pub const CLOSEST_PEERS_REQUEST_COUNT: usize = 5;

use futures_util::stream::{FuturesUnordered, StreamExt};
use stop_token::future::FutureExt as StopFutureExt;

impl PublicInternetRoutingDomainController {
    /// Ask our closest peers to give us more peers close to ourselves. This will
    /// assist with the DHT and other algorithms that utilize the distance metric.
    /// This only finds nodes in the PublicInternet domain.
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn closest_peers_refresh_task_routine(
        &self,
        stop_token: StopToken,
        _last_ts: Timestamp,
        cur_ts: Timestamp,
    ) -> EyreResult<()> {
        let routing_table = self.routing_table();

        // Snapshot once and extract existing node ids for dedup filtering
        let existing_node_ids = Arc::new(
            routing_table
                .snapshot_entries(cur_ts, BucketEntryState::Unreliable)
                .existing_node_ids(None),
        );

        let mut unord = FuturesUnordered::new();

        for crypto_kind in VALID_CRYPTO_KINDS {
            // Get our node id for this cryptokind
            let self_node_id = routing_table.node_id(crypto_kind);

            let mut filters = VecDeque::new();
            let filter = Box::new(
                move |opt_snap: &Option<BucketEntrySnapshot>, _cur_ts: Timestamp| {
                    // Exclude our own node
                    let Some(snap) = opt_snap else {
                        return false;
                    };
                    // Keep only the entries that contain the crypto kind we're looking for
                    if !snap.node_ids.kinds().contains(&crypto_kind) {
                        return false;
                    }
                    // Keep only the entries that participate in distance-metric relevant capabilities
                    // This would be better to be 'has_any_capabilities' but for now until out capnp gets
                    // this ability, it will do.
                    if !snap.has_all_capabilities(
                        RoutingDomain::PublicInternet,
                        DISTANCE_METRIC_CAPABILITIES,
                    ) {
                        return false;
                    }

                    // Keep only the entries that have responded to some answer consecutively
                    if snap.rpc_stats.first_steady_answer_ts.is_none() {
                        return false;
                    }

                    true
                },
            ) as RoutingTableEntryFilter;
            filters.push_front(filter);

            let closest_node_refs = routing_table.get_preferred_closest_nodes(
                CLOSEST_PEERS_REQUEST_COUNT,
                self_node_id.to_hash_coordinate(),
                filters,
                |opt_snap: Option<BucketEntrySnapshot>| opt_snap.unwrap_or_log().node_ref.clone(),
            );

            for closest_node_ref in closest_node_refs {
                let registry = self.registry();
                let existing_node_ids = existing_node_ids.clone();
                unord.push(
                    async move {
                        let routing_table = registry.routing_table();

                        // This would be better if it were 'any' instead of 'all' capabilities
                        // but that requires extending the capnp to support it.
                        let close_node_refs = network_result_value_or_log!(closest_node_ref match pin_future!(
                                routing_table.find_new_nodes_close_to_self(
                                    crypto_kind,
                                    &existing_node_ids,
                                    closest_node_ref.routing_domain_filtered(RoutingDomain::PublicInternet).with_sequencing(Sequencing::PreferOrdered),
                                    DISTANCE_METRIC_CAPABILITIES.to_vec())
                                ).await {
                            Err(e) => {
                                veilid_log!(closest_node_ref debug "failed to find nodes close to self: {}", e);
                                NetworkResult::value(vec![])
                            }
                            Ok(v) => v,
                        } => {
                            vec![]
                        });
                        if !close_node_refs.is_empty() {
                            veilid_log!(closest_node_ref debug target:"network_result", "closest_peers_refresh: found {} nodes close to self for {} in {} with {:?}", close_node_refs.len(), crypto_kind, RoutingDomain::PublicInternet, CONNECTIVITY_CAPABILITIES);
                        }
                    }
                    .instrument(Span::current()),
                );
            }
        }

        // do closest peers search in parallel
        while let Ok(Some(_)) = unord.next().timeout_at(stop_token.clone()).await {}

        Ok(())
    }
}
