use super::*;

const MAX_WATCH_VALUE_Q_SUBKEY_RANGES_LEN: usize = 512;
const MAX_WATCH_VALUE_A_PEERS_LEN: usize = 20;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationWatchValueQ {
    key: OpaqueRecordKey,
    subkeys: ValueSubkeyRangeSet,
    duration: TimestampDuration,
    count: u32,
    watch_id: Option<u64>,
}

impl RPCOperationWatchValueQ {
    pub fn new(
        key: OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        duration: TimestampDuration,
        count: u32,
        watch_id: Option<u64>,
    ) -> Result<Self, RPCError> {
        if subkeys.ranges_len() > MAX_WATCH_VALUE_Q_SUBKEY_RANGES_LEN {
            return Err(RPCError::internal("WatchValueQ subkeys length too long"));
        }

        // Count is zero means cancelling, so there should always be a watch id in this case
        if count == 0 && watch_id.is_none() {
            return Err(RPCError::internal("can't cancel zero watch id"));
        }

        Ok(Self {
            key,
            subkeys,
            duration,
            count,
            watch_id,
        })
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(
        self,
    ) -> (
        OpaqueRecordKey,
        ValueSubkeyRangeSet,
        TimestampDuration,
        u32,
        Option<u64>,
    ) {
        (
            self.key,
            self.subkeys,
            self.duration,
            self.count,
            self.watch_id,
        )
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_watch_value_q::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, key);
        let k_reader = reader.get_key()?;
        let key = decode_opaque_record_key(&k_reader)?;

        rpc_ignore_missing_property!(reader, subkeys);
        let sk_reader = reader.get_subkeys()?;
        rpc_ignore_max_len!(sk_reader, MAX_WATCH_VALUE_Q_SUBKEY_RANGES_LEN);
        let mut subkeys = ValueSubkeyRangeSet::new();
        for skr in sk_reader.iter() {
            let vskr = (skr.get_start(), skr.get_end());
            if vskr.0 > vskr.1 {
                return Err(RPCError::protocol("invalid subkey range"));
            }
            if let Some(lvskr) = subkeys.last() {
                if lvskr >= vskr.0 {
                    return Err(RPCError::protocol(
                        "subkey range out of order or not merged",
                    ));
                }
            }
            subkeys.ranges_insert(vskr.0..=vskr.1);
        }

        let duration = TimestampDuration::new(reader.get_duration());
        let count = reader.get_count();
        let watch_id = if reader.get_watch_id() != 0 {
            Some(reader.get_watch_id())
        } else {
            None
        };

        Self::new(key, subkeys, duration, count, watch_id)
    }

    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_watch_value_q::Builder,
    ) -> Result<(), RPCError> {
        let mut k_builder = builder.reborrow().init_key();
        encode_opaque_record_key(&self.key, &mut k_builder);

        let mut sk_builder = builder.reborrow().init_subkeys(
            self.subkeys
                .ranges_len()
                .try_into()
                .map_err(RPCError::map_internal("invalid subkey range list length"))?,
        );
        for (i, skr) in self.subkeys.ranges().enumerate() {
            let mut skr_builder = sk_builder.reborrow().get(i as u32);
            skr_builder.set_start(*skr.start());
            skr_builder.set_end(*skr.end());
        }
        builder.set_duration(self.duration.as_u64());
        builder.set_count(self.count);
        builder.set_watch_id(self.watch_id.unwrap_or(0u64));
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationWatchValueA {
    accepted: bool,
    duration: TimestampDuration,
    peers: Vec<Arc<PeerInfo>>,
    watch_id: u64,
}

impl RPCOperationWatchValueA {
    pub fn new(
        accepted: bool,
        duration: TimestampDuration,
        peers: Vec<Arc<PeerInfo>>,
        watch_id: u64,
    ) -> Result<Self, RPCError> {
        if peers.len() > MAX_WATCH_VALUE_A_PEERS_LEN {
            return Err(RPCError::internal("WatchValueA peers length too long"));
        }
        Ok(Self {
            accepted,
            duration,
            peers,
            watch_id,
        })
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(self) -> (bool, TimestampDuration, Vec<Arc<PeerInfo>>, u64) {
        (self.accepted, self.duration, self.peers, self.watch_id)
    }

    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_watch_value_a::Reader,
    ) -> Result<Self, RPCError> {
        let accepted = reader.get_accepted();
        let duration = TimestampDuration::new(reader.get_duration());

        rpc_ignore_missing_property!(reader, peers);
        let peers_reader = reader.get_peers()?;
        let peers_len = rpc_ignore_max_len!(peers_reader, MAX_WATCH_VALUE_A_PEERS_LEN);
        let mut peers = Vec::<Arc<PeerInfo>>::with_capacity(peers_len);
        for p in peers_reader.iter() {
            let Some(peer_info) = decode_peer_info(decode_context, &p).ignore_ok()? else {
                continue;
            };
            peers.push(Arc::new(peer_info));
        }
        let watch_id = reader.get_watch_id();

        Self::new(accepted, duration, peers, watch_id)
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_watch_value_a::Builder,
    ) -> Result<(), RPCError> {
        builder.set_accepted(self.accepted);
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
        builder.set_watch_id(self.watch_id);

        Ok(())
    }
}
