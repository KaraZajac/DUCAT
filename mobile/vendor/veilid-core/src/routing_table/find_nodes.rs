//! Utilities to search for peers on the network and add them to our routing table

use super::*;

impl_veilid_log_facility!("rtab");

const DEFAULT_WIDE_FIND_NODE_SEGMENTS: u8 = 16;

impl RoutingTable {
    /// Finds nodes near a particular hash coordinate
    /// Ensures all returned nodes have a set of capabilities enabled
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn find_nodes_close_to_hash_coordinate(
        &self,
        node_ref: FilteredNodeRef,
        hash_coordinate: HashCoordinate,
        capabilities: Vec<VeilidCapability>,
    ) -> EyreResult<NetworkResult<Vec<NodeRef>>> {
        let rpc_processor = self.rpc_processor();

        let res = network_result_try!(
            Box::pin(rpc_processor.rpc_call_find_node(
                Destination::direct(node_ref, None),
                hash_coordinate,
                capabilities
            ))
            .await?
        );

        // register nodes we'd found
        Ok(NetworkResult::value(
            self.register_nodes_with_peer_info_list(res.answer),
        ))
    }

    /// Ask a remote node to list the nodes it has around the current node
    /// Ensures all returned nodes have a set of capabilities enabled
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn find_new_nodes_close_to_self(
        &self,
        crypto_kind: CryptoKind,
        existing_node_ids: &HashSet<NodeId>,
        node_ref: FilteredNodeRef,
        capabilities: Vec<VeilidCapability>,
    ) -> EyreResult<NetworkResult<Vec<NodeRef>>> {
        let self_node_id = self.node_id(crypto_kind);
        let found_node_refs = network_result_try!(
            Box::pin(self.find_nodes_close_to_hash_coordinate(
                node_ref,
                self_node_id.to_hash_coordinate(),
                capabilities,
            ))
            .await?
        );

        // Filter out nodes that we already knew about
        // Include dead nodes that may be back online, but leave out punished nodes we don't care about
        let cur_ts = Timestamp::now();
        let new_node_refs = found_node_refs
            .into_iter()
            .filter(|nr| {
                !nr.node_ids()
                    .contains_any_from_iter(existing_node_ids.iter())
                    && nr.state(cur_ts) != BucketEntryState::Punished
            })
            .collect();

        Ok(NetworkResult::value(new_node_refs))
    }

    /// Ask a remote node to list the nodes it has around itself
    /// Ensures all returned nodes have a set of capabilities enabled
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn find_nodes_close_to_node_ref(
        &self,
        crypto_kind: CryptoKind,
        node_ref: FilteredNodeRef,
        capabilities: Vec<VeilidCapability>,
    ) -> EyreResult<NetworkResult<Vec<NodeRef>>> {
        let Some(target_node_id) = node_ref.node_ids().get(crypto_kind) else {
            bail!("no target node ids for this crypto kind");
        };
        Box::pin(self.find_nodes_close_to_hash_coordinate(
            node_ref,
            target_node_id.to_hash_coordinate(),
            capabilities,
        ))
        .await
    }

    /// Ask for nodes spaced out all over the routing table, centered on our own node
    /// 'segments' is the number of segments to divide the routing table into (2-64 segments)
    /// For example, if segments is 4, we will ask for 4 nodes, one in each segment of the routing table
    /// The nodes will be spaced out as evenly as possible
    /// Will only return nodes that are not already in the routing table and are not punished
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn find_new_nodes_wide(
        &self,
        crypto_kind: CryptoKind,
        existing_node_ids: &HashSet<NodeId>,
        routing_domain: RoutingDomain,
        segments: Option<u8>,
        capabilities: Vec<VeilidCapability>,
    ) -> EyreResult<Vec<NodeRef>> {
        let segments = segments.unwrap_or(DEFAULT_WIDE_FIND_NODE_SEGMENTS);
        // Get our own node id
        let self_node_id = self.node_id(crypto_kind);

        // Get the spacing between nodes
        if !(2..=64).contains(&segments) {
            bail!("segments must be between 2 and 64");
        }
        let spacing = 256usize / (segments as usize);
        let shift = spacing / 2;

        let mut shift_segment_bytes = BytesMut::zeroed(HASH_COORDINATE_LENGTH);
        shift_segment_bytes[0] = shift as u8;

        let mut spacing_segment_bytes = BytesMut::zeroed(HASH_COORDINATE_LENGTH);
        spacing_segment_bytes[0] = spacing as u8;

        let shift_hash_distance = HashDistance::new_from_bytes(shift_segment_bytes.freeze());
        let spacing_hash_distance = HashDistance::new_from_bytes(spacing_segment_bytes.freeze());

        // Get target hash coordinates for each segment
        let mut current_hash_distance = shift_hash_distance;
        let mut target_hash_coordinates = Vec::new();
        for _ in 0..segments {
            let target_hash_coordinate = self_node_id
                .to_hash_coordinate()
                .offset(&current_hash_distance);
            target_hash_coordinates.push(target_hash_coordinate);
            current_hash_distance += spacing_hash_distance.clone();
        }

        // Get the node we know about that is closest to each target hash coordinate
        // Uses a brief snapshot for the sync lookup, then drops it before async work
        let target_node_refs = {
            let snapshot = self.snapshot_entries(Timestamp::now(), BucketEntryState::Initial);
            let closest_results = self.get_nodes_closest_to_multiple_hash_coordinates(
                routing_domain,
                &target_hash_coordinates,
                &capabilities,
                &snapshot,
            );
            let mut target_node_refs = HashSet::new();
            for (i, result) in closest_results.into_iter().enumerate() {
                if let Some(target_node_ref) = result {
                    if !target_node_refs.insert(target_node_ref) {
                        veilid_log!(self trace
                            "find_nodes_wide: duplicate node found: {:?}",
                            target_hash_coordinates[i]
                        );
                    }
                } else {
                    veilid_log!(self trace
                        "find_nodes_wide: no node found closest to hash coordinate: {:?}",
                        target_hash_coordinates[i]
                    );
                }
            }
            target_node_refs
        };

        // Ask each nodes near themselves
        let mut unord = FuturesUnordered::new();

        // Track nodes we've found
        let mut found_node_refs = HashSet::new();

        for target_node_ref in target_node_refs {
            let capabilities = capabilities.clone();
            unord.push(async move {
                network_result_value_or_log!(self match pin_future!(self.find_nodes_close_to_node_ref(crypto_kind, target_node_ref.clone(), capabilities.clone())).await {
                    Err(e) => {
                        veilid_log!(self error
                            "find_nodes_close_to_node_ref failed for {:?}: {:?}",
                            target_node_ref, e
                        );
                        return vec![];
                    }
                    Ok(v) => v,
                } => [ format!(": crypto_kind={} target_node_ref={} capabilities={:?}", crypto_kind, target_node_ref, capabilities) ] {
                    vec![]
                })
            });
        }
        while let Some(nodes) = unord.next().await {
            for node in nodes {
                if !found_node_refs.insert(node.clone()) {
                    veilid_log!(self trace
                        "find_nodes_wide: duplicate node found near target node: {:?}",
                        node
                    );
                }
            }
        }

        // Filter out nodes that we already knew about
        // Include dead nodes that may be back online, but leave out punished nodes we don't care about
        let cur_ts = Timestamp::now();
        let new_node_refs = found_node_refs
            .into_iter()
            .filter(|nr| {
                !nr.node_ids()
                    .contains_any_from_iter(existing_node_ids.iter())
                    && nr.state(cur_ts) != BucketEntryState::Punished
            })
            .collect();

        Ok(new_node_refs)
    }
}
