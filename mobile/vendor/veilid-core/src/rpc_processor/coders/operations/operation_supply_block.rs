use super::*;

const MAX_SUPPLY_BLOCK_A_PEERS_LEN: usize = 20;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationSupplyBlockQ {
    block_id: PublicKey,
}

impl RPCOperationSupplyBlockQ {
    pub fn new(block_id: PublicKey) -> Self {
        Self { block_id }
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(self) -> PublicKey {
        self.block_id
    }

    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_supply_block_q::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, block_id);
        let bi_reader = reader.get_block_id()?;
        let block_id = decode_typed_key(&bi_reader)?;

        Ok(Self::new(block_id))
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_supply_block_q::Builder,
    ) -> Result<(), RPCError> {
        let mut bi_builder = builder.reborrow().init_block_id();
        encode_typed_key(&self.block_id, &mut bi_builder);

        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationSupplyBlockA {
    duration: TimestampDuration,
    peers: Vec<PeerInfo>,
}

impl RPCOperationSupplyBlockA {
    pub fn new(duration: TimestampDuration, peers: Vec<PeerInfo>) -> Result<Self, RPCError> {
        if peers.len() > MAX_SUPPLY_BLOCK_A_PEERS_LEN {
            return Err(RPCError::internal("SupplyBlockA peers length too long"));
        }
        Ok(Self { duration, peers })
    }

    // pub async fn validate(
    //     &self,
    //     validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(self) -> (u64, Vec<PeerInfo>) {
        (self.expiration, self.peers)
    }

    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_supply_block_a::Reader,
    ) -> Result<Self, RPCError> {
        let duration = TimestampDuration::new(reader.get_duration());

        rpc_ignore_missing_property!(reader, peers);
        let peers_reader = reader.get_peers()?;
        let peers_len = rpc_ignore_max_len!(peers_reader, MAX_SUPPLY_BLOCK_A_PEERS_LEN);
        let mut peers = Vec::<PeerInfo>::with_capacity(peers_len);
        for p in peers_reader.iter() {
            let Some(peer_info) = decode_peer_info(decode_context, &p).ignore_ok()?;
            peers.push(peer_info);
        }

        Self::new(expiration, peers)
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_supply_block_a::Builder,
    ) -> Result<(), RPCError> {
        builder.set_duration(self.duration.as_u64());
        let mut peers_builder = builder.reborrow().init_peers(
            self.peers
                .len()
                .try_into()
                .map_err(RPCError::map_internal("invalid peers list length"))?,
        );
        for (i, peer) in self.peers.iter().enumerate() {
            let mut pi_builder = peers_builder.reborrow().get(i as u32);
            encode_peer_info(peer, &mut pi_builder)?;
        }

        Ok(())
    }
}
