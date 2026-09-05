use super::*;
use crate::storage_manager::SignedValueDescriptor;

const MAX_INSPECT_VALUE_Q_SUBKEY_RANGES_LEN: usize = DHTSchema::MAX_SUBKEY_COUNT / 2;
const MAX_INSPECT_VALUE_A_SEQS_LEN: usize = DHTSchema::MAX_SUBKEY_COUNT;
const MAX_INSPECT_VALUE_A_PEERS_LEN: usize = 20;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct ValidateInspectValueContext {
    pub opaque_record_key: OpaqueRecordKey,
    pub descriptor_mode: GetDescriptorMode,
    pub subkeys: ValueSubkeyRangeSet,
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationInspectValueQ {
    key: OpaqueRecordKey,
    subkeys: ValueSubkeyRangeSet,
    want_descriptor: bool,
}

impl RPCOperationInspectValueQ {
    pub fn new(
        key: OpaqueRecordKey,
        subkeys: ValueSubkeyRangeSet,
        want_descriptor: bool,
    ) -> Result<Self, RPCError> {
        Ok(Self {
            key,
            subkeys,
            want_descriptor,
        })
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(self) -> (OpaqueRecordKey, ValueSubkeyRangeSet, bool) {
        (self.key, self.subkeys, self.want_descriptor)
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_inspect_value_q::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, key);
        let k_reader = reader.get_key()?;
        let key = decode_opaque_record_key(&k_reader)?;

        rpc_ignore_missing_property!(reader, subkeys);
        let sk_reader = reader.get_subkeys()?;
        // Maximum number of ranges that can hold the maximum number of subkeys is one subkey per range
        rpc_ignore_max_len!(sk_reader, MAX_INSPECT_VALUE_Q_SUBKEY_RANGES_LEN);

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

        let want_descriptor = reader.get_want_descriptor();
        Ok(Self {
            key,
            subkeys,
            want_descriptor,
        })
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_inspect_value_q::Builder,
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
        builder.set_want_descriptor(self.want_descriptor);
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationInspectValueA {
    accepted: bool,
    seqs: Vec<ValueSeqNum>,
    peers: Vec<Arc<PeerInfo>>,
    descriptor: Option<SignedValueDescriptor>,
}

impl RPCOperationInspectValueA {
    pub fn new(
        accepted: bool,
        seqs: Vec<ValueSeqNum>,
        peers: Vec<Arc<PeerInfo>>,
        descriptor: Option<SignedValueDescriptor>,
    ) -> Result<Self, RPCError> {
        // Validate length of seqs
        if seqs.len() > MAX_INSPECT_VALUE_A_SEQS_LEN {
            return Err(RPCError::protocol(
                "encoded InspectValueA seqs length too long",
            ));
        }
        // Validate length of peers
        if peers.len() > MAX_INSPECT_VALUE_A_PEERS_LEN {
            return Err(RPCError::protocol(
                "encoded InspectValueA peers length too long",
            ));
        }
        Ok(Self {
            accepted,
            seqs,
            peers,
            descriptor,
        })
    }

    pub async fn validate(
        &self,
        validate_context: &RPCValidateContext<'_>,
    ) -> Result<(), RPCError> {
        let question_context = validate_context
            .question_context
            .as_ref()
            .expect_or_log("InspectValueA requires question context");
        let QuestionContext::InspectValue(inspect_value_context) = question_context else {
            return Err(RPCError::internal("Wrong context type for InspectValueA"));
        };

        let crypto = validate_context.crypto();
        let Some(vcrypto) = crypto.get_async(inspect_value_context.opaque_record_key.kind()) else {
            return Err(RPCError::protocol("unsupported cryptosystem"));
        };

        // Ensure seqs returned does not exceeed subkeys requested
        let subkey_count = if inspect_value_context.subkeys.is_empty()
            || inspect_value_context.subkeys.is_full()
            || inspect_value_context.subkeys.len() > MAX_INSPECT_VALUE_A_SEQS_LEN as u64
        {
            MAX_INSPECT_VALUE_A_SEQS_LEN as u64
        } else {
            inspect_value_context.subkeys.len()
        };
        if self.seqs.len() as u64 > subkey_count {
            return Err(RPCError::protocol(format!(
                "InspectValue seqs length is greater than subkeys requested: {} > {}: {:#?}",
                self.seqs.len(),
                subkey_count,
                inspect_value_context
            )));
        }

        // Validate descriptor
        if let Some(descriptor) = &self.descriptor {
            // Ensure the descriptor itself validates
            descriptor
                .validate(&vcrypto, &inspect_value_context.opaque_record_key)
                .await
                .map_err(RPCError::protocol)?;

            // Ensure descriptor matches last one
            if let GetDescriptorMode::HaveDescriptor(last_descriptor) =
                &inspect_value_context.descriptor_mode
            {
                if descriptor.cmp_no_sig(last_descriptor) != cmp::Ordering::Equal {
                    return Err(RPCError::protocol(
                        "InspectValue descriptor does not match last descriptor",
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn destructure(
        self,
    ) -> (
        bool,
        Vec<ValueSeqNum>,
        Vec<Arc<PeerInfo>>,
        Option<SignedValueDescriptor>,
    ) {
        (self.accepted, self.seqs, self.peers, self.descriptor)
    }

    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_inspect_value_a::Reader,
    ) -> Result<Self, RPCError> {
        let accepted = reader.get_accepted();

        rpc_ignore_missing_property!(reader, seqs);
        let seqs = {
            let seqs_reader = reader.get_seqs()?;
            rpc_ignore_max_len!(seqs_reader, MAX_INSPECT_VALUE_A_SEQS_LEN);
            seqs_reader.iter().map(ValueSeqNum::from).collect()
        };

        rpc_ignore_missing_property!(reader, peers);
        let peers_reader = reader.get_peers()?;
        let peers_len = rpc_ignore_max_len!(peers_reader, MAX_INSPECT_VALUE_A_PEERS_LEN);
        let mut peers = Vec::<Arc<PeerInfo>>::with_capacity(peers_len);
        for p in peers_reader.iter() {
            let Some(peer_info) = decode_peer_info(decode_context, &p).ignore_ok()? else {
                continue;
            };
            peers.push(Arc::new(peer_info));
        }

        let descriptor = if reader.has_descriptor() {
            let d_reader = reader.get_descriptor()?;
            let descriptor = decode_signed_value_descriptor(&d_reader)?;
            Some(descriptor)
        } else {
            None
        };

        Self::new(accepted, seqs, peers, descriptor)
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_inspect_value_a::Builder,
    ) -> Result<(), RPCError> {
        builder.set_accepted(self.accepted);

        let mut seqs_builder = builder.reborrow().init_seqs(
            self.seqs
                .len()
                .try_into()
                .map_err(RPCError::map_internal("invalid seqs list length"))?,
        );
        for (i, seq) in self.seqs.iter().enumerate() {
            seqs_builder.set(i as u32, u32::from(*seq));
        }

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

        if let Some(descriptor) = &self.descriptor {
            let mut d_builder = builder.reborrow().init_descriptor();
            encode_signed_value_descriptor(descriptor, &mut d_builder)?;
        }

        Ok(())
    }
}
