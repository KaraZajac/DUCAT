use super::*;

const MAX_FIND_NODE_A_PEERS_LEN: usize = 20;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationFindNodeQ {
    hash_coordinate: HashCoordinate,
    capabilities: Vec<VeilidCapability>,
}

impl RPCOperationFindNodeQ {
    pub fn new(
        hash_coordinate: HashCoordinate,
        capabilities: Vec<VeilidCapability>,
    ) -> Result<Self, RPCError> {
        if capabilities.len() > MAX_CAPABILITIES {
            return Err(RPCError::internal("capabilities length too long"));
        }

        Ok(Self {
            hash_coordinate,
            capabilities,
        })
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(self) -> (HashCoordinate, Vec<VeilidCapability>) {
        (self.hash_coordinate, self.capabilities)
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_find_node_q::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, node_id);
        let ni_reader = reader.get_node_id()?;
        let node_id = decode_node_id(&ni_reader)?;

        rpc_ignore_missing_property!(reader, capabilities);
        let cap_reader = reader.get_capabilities()?;
        rpc_ignore_max_len!(cap_reader, MAX_CAPABILITIES);
        let capabilities = cap_reader
            .as_slice()
            .map(|s| {
                s.iter()
                    .map(|x| VeilidCapability::from(x.to_be_bytes()))
                    .collect()
            })
            .unwrap_or_default();

        Self::new(node_id.to_hash_coordinate(), capabilities)
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_find_node_q::Builder,
    ) -> Result<(), RPCError> {
        let mut ni_builder = builder.reborrow().init_node_id();
        encode_node_id(
            &NodeId::new(
                self.hash_coordinate.kind(),
                BareNodeId::new_from_bytes(self.hash_coordinate.ref_value().bytes()),
            ),
            &mut ni_builder,
        );

        let mut cap_builder = builder
            .reborrow()
            .init_capabilities(self.capabilities.len() as u32);
        if let Some(s) = cap_builder.as_slice() {
            let capvec: Vec<u32> = self.capabilities.iter().copied().map(u32::from).collect();

            s.clone_from_slice(&capvec);
        }
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationFindNodeA {
    peers: Vec<Arc<PeerInfo>>,
}

impl RPCOperationFindNodeA {
    pub fn new(peers: Vec<Arc<PeerInfo>>) -> Result<Self, RPCError> {
        if peers.len() > MAX_FIND_NODE_A_PEERS_LEN {
            return Err(RPCError::internal(
                "encoded find node peers length too long",
            ));
        }

        Ok(Self { peers })
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(self) -> Vec<Arc<PeerInfo>> {
        self.peers
    }

    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_find_node_a::Reader,
    ) -> Result<RPCOperationFindNodeA, RPCError> {
        rpc_ignore_missing_property!(reader, peers);
        let peers_reader = reader.get_peers()?;
        let peers_len = rpc_ignore_max_len!(peers_reader, MAX_FIND_NODE_A_PEERS_LEN);
        let mut peers = Vec::<Arc<PeerInfo>>::with_capacity(peers_len);
        for p in peers_reader.iter() {
            let Some(peer_info) = decode_peer_info(decode_context, &p).ignore_ok()? else {
                continue;
            };
            peers.push(Arc::new(peer_info));
        }

        Self::new(peers)
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_find_node_a::Builder,
    ) -> Result<(), RPCError> {
        let mut peers_builder = builder.reborrow().init_peers(
            self.peers
                .len()
                .try_into()
                .map_err(RPCError::map_internal("invalid closest nodes list length"))?,
        );
        for (i, peer) in self.peers.iter().enumerate() {
            let mut pi_builder = peers_builder.reborrow().get(i as u32);
            encode_peer_info(peer, &mut pi_builder)?;
        }
        Ok(())
    }
}
