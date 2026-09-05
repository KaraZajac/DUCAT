use super::*;
use crate::storage_manager::{SignedValueData, SignedValueDescriptor};

const MAX_GET_VALUE_A_PEERS_LEN: usize = 20;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct ValidateGetValueContext {
    pub opaque_record_key: OpaqueRecordKey,
    pub descriptor_mode: GetDescriptorMode,
    pub subkey: ValueSubkey,
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationGetValueQ {
    key: OpaqueRecordKey,
    subkey: ValueSubkey,
    want_descriptor: bool,
}

impl RPCOperationGetValueQ {
    pub fn new(
        key: OpaqueRecordKey,
        subkey: ValueSubkey,
        want_descriptor: bool,
    ) -> Result<Self, RPCError> {
        Ok(Self {
            key,
            subkey,
            want_descriptor,
        })
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(self) -> (OpaqueRecordKey, ValueSubkey, bool) {
        (self.key, self.subkey, self.want_descriptor)
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_get_value_q::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, key);
        let k_reader = reader.get_key()?;
        let key = decode_opaque_record_key(&k_reader)?;
        let subkey = reader.get_subkey();
        let want_descriptor = reader.get_want_descriptor();
        Ok(Self {
            key,
            subkey,
            want_descriptor,
        })
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_get_value_q::Builder,
    ) -> Result<(), RPCError> {
        let mut k_builder = builder.reborrow().init_key();
        encode_opaque_record_key(&self.key, &mut k_builder);
        builder.set_subkey(self.subkey);
        builder.set_want_descriptor(self.want_descriptor);
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationGetValueA {
    accepted: bool,
    value: Option<SignedValueData>,
    peers: Vec<Arc<PeerInfo>>,
    descriptor: Option<SignedValueDescriptor>,
}

impl RPCOperationGetValueA {
    pub fn new(
        accepted: bool,
        value: Option<SignedValueData>,
        peers: Vec<Arc<PeerInfo>>,
        descriptor: Option<SignedValueDescriptor>,
    ) -> Result<Self, RPCError> {
        if peers.len() > MAX_GET_VALUE_A_PEERS_LEN {
            return Err(RPCError::protocol(
                "encoded GetValueA peers length too long",
            ));
        }
        Ok(Self {
            accepted,
            value,
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
            .expect_or_log("GetValueA requires question context");
        let QuestionContext::GetValue(get_value_context) = question_context else {
            return Err(RPCError::internal("Wrong context type for GetValueA"));
        };

        let crypto = validate_context.crypto();
        let Some(vcrypto) = crypto.get_async(get_value_context.opaque_record_key.kind()) else {
            return Err(RPCError::protocol("unsupported cryptosystem"));
        };

        // Validate descriptor
        if let Some(descriptor) = &self.descriptor {
            // Ensure the descriptor itself validates
            descriptor
                .validate(&vcrypto, &get_value_context.opaque_record_key)
                .await
                .map_err(RPCError::protocol)?;

            // Ensure descriptor matches last one
            if let Some(last_descriptor) = get_value_context.descriptor_mode.opt_ref_descriptor() {
                if descriptor.cmp_no_sig(last_descriptor) != cmp::Ordering::Equal {
                    return Err(RPCError::protocol(
                        "GetValue descriptor does not match last descriptor",
                    ));
                }
            }
        }

        // Ensure the value validates
        if let Some(value) = &self.value {
            // Get descriptor to validate with
            let Some(descriptor) = self
                .descriptor
                .as_ref()
                .or(get_value_context.descriptor_mode.opt_ref_descriptor())
            else {
                return Err(RPCError::protocol(
                    "no last descriptor, requires a descriptor",
                ));
            };

            // And the signed value data
            if !value
                .validate(descriptor.ref_owner(), get_value_context.subkey, &vcrypto)
                .await
                .map_err(RPCError::protocol)?
            {
                return Err(RPCError::protocol("signed value data did not validate"));
            }
        }

        Ok(())
    }

    pub fn destructure(
        self,
    ) -> (
        bool,
        Option<SignedValueData>,
        Vec<Arc<PeerInfo>>,
        Option<SignedValueDescriptor>,
    ) {
        (self.accepted, self.value, self.peers, self.descriptor)
    }

    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_get_value_a::Reader,
    ) -> Result<Self, RPCError> {
        let accepted = reader.get_accepted();

        let value = if reader.has_value() {
            let value_reader = reader.get_value()?;
            let value = decode_signed_value_data(&value_reader)?;
            Some(value)
        } else {
            None
        };

        rpc_ignore_missing_property!(reader, peers);
        let peers_reader = reader.get_peers()?;
        let peers_len = rpc_ignore_max_len!(peers_reader, MAX_GET_VALUE_A_PEERS_LEN);
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

        Self::new(accepted, value, peers, descriptor)
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_get_value_a::Builder,
    ) -> Result<(), RPCError> {
        builder.set_accepted(self.accepted);

        if let Some(value) = &self.value {
            let mut v_builder = builder.reborrow().init_value();
            encode_signed_value_data(value, &mut v_builder)?;
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
