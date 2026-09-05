use super::*;
use crate::storage_manager::SignedValueDescriptor;

pub const MAX_TRANSACT_BEGIN_A_SEQS_LEN: usize = DHTSchema::MAX_SUBKEY_COUNT;
const MAX_TRANSACT_BEGIN_A_PEERS_LEN: usize = 20;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct ValidateTransactBeginContext {
    pub opaque_record_key: OpaqueRecordKey,
    pub descriptor_mode: DescriptorMode,
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationTransactBeginQ {
    key: OpaqueRecordKey,
    descriptor: Option<SignedValueDescriptor>,
    want_descriptor: bool,
}

impl RPCOperationTransactBeginQ {
    pub fn new(
        key: OpaqueRecordKey,
        descriptor: Option<SignedValueDescriptor>,
        want_descriptor: bool,
    ) -> Result<Self, RPCError> {
        // Should not provide descriptor and want descriptor
        if descriptor.is_some() && want_descriptor {
            return Err(RPCError::protocol("want_descriptor but already provided"));
        }

        Ok(Self {
            key,
            descriptor,
            want_descriptor,
        })
    }

    pub async fn validate(
        &self,
        validate_context: &RPCValidateContext<'_>,
    ) -> Result<(), RPCError> {
        let crypto = validate_context.crypto();
        let Some(vcrypto) = crypto.get_async(self.key.kind()) else {
            return Err(RPCError::protocol("unsupported cryptosystem"));
        };

        // Validate descriptor
        if let Some(descriptor) = &self.descriptor {
            // Ensure the descriptor itself validates
            descriptor
                .validate(&vcrypto, &self.key)
                .await
                .map_err(RPCError::protocol)?;
        }
        Ok(())
    }

    pub fn destructure(self) -> (OpaqueRecordKey, Option<SignedValueDescriptor>, bool) {
        (self.key, self.descriptor, self.want_descriptor)
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_transact_begin_q::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, key);
        let k_reader = reader.get_key()?;
        let key = decode_opaque_record_key(&k_reader)?;

        let descriptor = if reader.has_descriptor() {
            let d_reader = reader.get_descriptor()?;
            let descriptor = decode_signed_value_descriptor(&d_reader)?;
            Some(descriptor)
        } else {
            None
        };

        let want_descriptor = reader.get_want_descriptor();

        // Should not provide descriptor and want descriptor
        if descriptor.is_some() && want_descriptor {
            return Err(RPCError::protocol("want_descriptor but already provided"));
        }

        Ok(Self {
            key,
            descriptor,
            want_descriptor,
        })
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_transact_begin_q::Builder,
    ) -> Result<(), RPCError> {
        let mut k_builder = builder.reborrow().init_key();
        encode_opaque_record_key(&self.key, &mut k_builder);

        if let Some(descriptor) = &self.descriptor {
            let mut d_builder = builder.reborrow().init_descriptor();
            encode_signed_value_descriptor(descriptor, &mut d_builder)?;
        }

        builder.set_want_descriptor(self.want_descriptor);

        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationTransactBeginA {
    accepted: bool,
    need_descriptor: bool,
    descriptor: Option<SignedValueDescriptor>,
    transaction_id: Option<u64>,
    duration: TimestampDuration,
    seqs: Vec<ValueSeqNum>,
    peers: Vec<Arc<PeerInfo>>,
}

impl RPCOperationTransactBeginA {
    pub fn new(
        accepted: bool,
        need_descriptor: bool,
        descriptor: Option<SignedValueDescriptor>,
        transaction_id: Option<u64>,
        duration: TimestampDuration,
        seqs: Vec<ValueSeqNum>,
        peers: Vec<Arc<PeerInfo>>,
    ) -> Result<Self, RPCError> {
        // Should not reject but also provide other fields
        if !accepted
            && (need_descriptor
                || descriptor.is_some()
                || transaction_id.is_some()
                || !duration.is_zero()
                || !seqs.is_empty())
        {
            return Err(RPCError::internal("not accepted but fields provided"));
        }

        // Should not provide descriptor and need descriptor
        if descriptor.is_some() && need_descriptor {
            return Err(RPCError::internal("need_descriptor but already provided"));
        }

        // Transaction id should never be zero here as that is the sentinel for None
        if transaction_id == Some(0u64) {
            return Err(RPCError::internal("invalid transaction id"));
        }

        if seqs.len() > MAX_TRANSACT_BEGIN_A_SEQS_LEN {
            return Err(RPCError::internal(
                "encoded TransactBeginA seqs length too long",
            ));
        }

        if peers.len() > MAX_TRANSACT_BEGIN_A_PEERS_LEN {
            return Err(RPCError::internal(
                "encoded TransactBeginA peers length too long",
            ));
        }

        Ok(Self {
            accepted,
            need_descriptor,
            descriptor,
            transaction_id,
            duration,
            seqs,
            peers,
        })
    }

    pub async fn validate(
        &self,
        validate_context: &RPCValidateContext<'_>,
    ) -> Result<(), RPCError> {
        let question_context = validate_context
            .question_context
            .as_ref()
            .expect_or_log("TransactBeginA requires question context");
        let QuestionContext::TransactBegin(transact_begin_context) = question_context else {
            return Err(RPCError::internal("Wrong context type for TransactBeginA"));
        };

        let crypto = validate_context.crypto();
        let Some(vcrypto) = crypto.get_async(transact_begin_context.opaque_record_key.kind())
        else {
            return Err(RPCError::protocol("unsupported cryptosystem"));
        };

        // Validate descriptor
        if let Some(descriptor) = &self.descriptor {
            // Ensure the descriptor itself validates
            descriptor
                .validate(&vcrypto, &transact_begin_context.opaque_record_key)
                .await
                .map_err(RPCError::protocol)?;

            // Ensure descriptor matches last one
            if let Some(last_descriptor) =
                transact_begin_context.descriptor_mode.opt_ref_descriptor()
            {
                if descriptor.cmp_no_sig(last_descriptor) != cmp::Ordering::Equal {
                    return Err(RPCError::protocol(
                        "TransactBegin descriptor does not match last descriptor",
                    ));
                }
            }
        }

        Ok(())
    }

    #[expect(clippy::type_complexity)]
    pub fn destructure(
        self,
    ) -> (
        bool,
        bool,
        Option<SignedValueDescriptor>,
        Option<u64>,
        TimestampDuration,
        Vec<ValueSeqNum>,
        Vec<Arc<PeerInfo>>,
    ) {
        (
            self.accepted,
            self.need_descriptor,
            self.descriptor,
            self.transaction_id,
            self.duration,
            self.seqs,
            self.peers,
        )
    }

    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_transact_begin_a::Reader,
    ) -> Result<Self, RPCError> {
        let accepted = reader.get_accepted();
        let need_descriptor = reader.get_need_descriptor();

        let descriptor = if reader.has_descriptor() {
            let d_reader = reader.get_descriptor()?;
            let descriptor = decode_signed_value_descriptor(&d_reader)?;
            Some(descriptor)
        } else {
            None
        };

        let transaction_id = reader.get_transaction_id();
        let transaction_id = if transaction_id == 0 {
            None
        } else {
            Some(transaction_id)
        };

        let duration = TimestampDuration::new(reader.get_duration());

        rpc_ignore_missing_property!(reader, seqs);
        let seqs = {
            let seqs_reader = reader.get_seqs()?;
            rpc_ignore_max_len!(seqs_reader, MAX_TRANSACT_BEGIN_A_SEQS_LEN);
            seqs_reader
                .iter()
                .map(ValueSeqNum::from)
                .collect::<Vec<_>>()
        };

        let peers_reader = reader.get_peers()?;
        let peers_len = rpc_ignore_max_len!(peers_reader, MAX_TRANSACT_BEGIN_A_PEERS_LEN);
        let mut peers = Vec::<Arc<PeerInfo>>::with_capacity(peers_len);
        for p in peers_reader.iter() {
            let Some(peer_info) = decode_peer_info(decode_context, &p).ignore_ok()? else {
                continue;
            };
            peers.push(Arc::new(peer_info));
        }
        // Should not reject but also provide other fields
        if !accepted
            && (need_descriptor
                || descriptor.is_some()
                || transaction_id.is_some()
                || !duration.is_zero()
                || !seqs.is_empty())
        {
            return Err(RPCError::protocol("not accepted but fields provided"));
        }

        // Should not provide descriptor and need descriptor
        if descriptor.is_some() && need_descriptor {
            return Err(RPCError::protocol("need_descriptor but already provided"));
        }

        // Transaction id should never be zero here as that is the sentinel for None
        if transaction_id == Some(0u64) {
            return Err(RPCError::protocol("invalid transaction id"));
        }

        Self::new(
            accepted,
            need_descriptor,
            descriptor,
            transaction_id,
            duration,
            seqs,
            peers,
        )
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_transact_begin_a::Builder,
    ) -> Result<(), RPCError> {
        builder.set_accepted(self.accepted);
        builder.set_need_descriptor(self.need_descriptor);

        if let Some(descriptor) = &self.descriptor {
            let mut d_builder = builder.reborrow().init_descriptor();
            encode_signed_value_descriptor(descriptor, &mut d_builder)?;
        }

        builder.set_transaction_id(self.transaction_id.unwrap_or(0));
        builder.set_duration(self.duration.as_u64());

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

        Ok(())
    }
}
