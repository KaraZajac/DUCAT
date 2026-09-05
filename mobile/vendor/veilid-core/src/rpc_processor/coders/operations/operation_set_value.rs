use super::*;
use crate::storage_manager::{SignedValueData, SignedValueDescriptor};

const MAX_SET_VALUE_A_PEERS_LEN: usize = 20;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct ValidateSetValueContext {
    pub opaque_record_key: OpaqueRecordKey,
    pub descriptor: SignedValueDescriptor,
    pub subkey: ValueSubkey,
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationSetValueQ {
    key: OpaqueRecordKey,
    subkey: ValueSubkey,
    value: SignedValueData,
    descriptor: Option<SignedValueDescriptor>,
}

impl RPCOperationSetValueQ {
    pub fn new(
        key: OpaqueRecordKey,
        subkey: ValueSubkey,
        value: SignedValueData,
        descriptor: Option<SignedValueDescriptor>,
    ) -> Self {
        Self {
            key,
            subkey,
            value,
            descriptor,
        }
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     // Validation is performed by StorageManager because descriptor is not always available here
    //     Ok(())
    // }

    pub fn destructure(
        self,
    ) -> (
        OpaqueRecordKey,
        ValueSubkey,
        SignedValueData,
        Option<SignedValueDescriptor>,
    ) {
        (self.key, self.subkey, self.value, self.descriptor)
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_set_value_q::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, key);
        let k_reader = reader.get_key()?;
        let key = decode_opaque_record_key(&k_reader)?;

        let subkey = reader.get_subkey();

        rpc_ignore_missing_property!(reader, value);
        let v_reader = reader.get_value()?;
        let value = decode_signed_value_data(&v_reader)?;

        let descriptor = if reader.has_descriptor() {
            let d_reader = reader.get_descriptor()?;
            let descriptor = decode_signed_value_descriptor(&d_reader)?;
            Some(descriptor)
        } else {
            None
        };
        Ok(Self::new(key, subkey, value, descriptor))
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_set_value_q::Builder,
    ) -> Result<(), RPCError> {
        let mut k_builder = builder.reborrow().init_key();
        encode_opaque_record_key(&self.key, &mut k_builder);
        builder.set_subkey(self.subkey);
        let mut v_builder = builder.reborrow().init_value();
        encode_signed_value_data(&self.value, &mut v_builder)?;
        if let Some(descriptor) = &self.descriptor {
            let mut d_builder = builder.reborrow().init_descriptor();
            encode_signed_value_descriptor(descriptor, &mut d_builder)?;
        }
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationSetValueA {
    accepted: bool,
    need_descriptor: bool,
    value: Option<SignedValueData>,
    peers: Vec<Arc<PeerInfo>>,
}

impl RPCOperationSetValueA {
    pub fn new(
        accepted: bool,
        need_descriptor: bool,
        value: Option<SignedValueData>,
        peers: Vec<Arc<PeerInfo>>,
    ) -> Result<Self, RPCError> {
        if peers.len() > MAX_SET_VALUE_A_PEERS_LEN {
            return Err(RPCError::internal(
                "encoded SetValueA peers length too long",
            ));
        }
        Ok(Self {
            accepted,
            need_descriptor,
            value,
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
            .expect_or_log("SetValueA requires question context");
        let QuestionContext::SetValue(set_value_context) = question_context else {
            return Err(RPCError::internal("Wrong context type for SetValueA"));
        };

        let crypto = validate_context.crypto();
        let Some(vcrypto) = crypto.get_async(set_value_context.opaque_record_key.kind()) else {
            return Err(RPCError::protocol("unsupported cryptosystem"));
        };

        // Ensure the descriptor itself validates
        set_value_context
            .descriptor
            .validate(&vcrypto, &set_value_context.opaque_record_key)
            .await
            .map_err(RPCError::protocol)?;

        if let Some(value) = &self.value {
            // And the signed value data
            if !value
                .validate(
                    set_value_context.descriptor.ref_owner(),
                    set_value_context.subkey,
                    &vcrypto,
                )
                .await
                .map_err(RPCError::protocol)?
            {
                return Err(RPCError::protocol("signed value data did not validate"));
            }
        }

        Ok(())
    }

    pub fn destructure(self) -> (bool, bool, Option<SignedValueData>, Vec<Arc<PeerInfo>>) {
        (self.accepted, self.need_descriptor, self.value, self.peers)
    }

    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_set_value_a::Reader,
    ) -> Result<Self, RPCError> {
        let accepted = reader.get_accepted();
        let need_descriptor = reader.get_need_descriptor();
        let value = if reader.has_value() {
            let v_reader = reader.get_value()?;
            let value = decode_signed_value_data(&v_reader)?;
            Some(value)
        } else {
            None
        };
        let peers_reader = reader.get_peers()?;
        let peers_len = rpc_ignore_max_len!(peers_reader, MAX_SET_VALUE_A_PEERS_LEN);
        let mut peers = Vec::<Arc<PeerInfo>>::with_capacity(peers_len);
        for p in peers_reader.iter() {
            let Some(peer_info) = decode_peer_info(decode_context, &p).ignore_ok()? else {
                continue;
            };
            peers.push(Arc::new(peer_info));
        }

        Self::new(accepted, need_descriptor, value, peers)
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_set_value_a::Builder,
    ) -> Result<(), RPCError> {
        builder.set_accepted(self.accepted);
        builder.set_need_descriptor(self.need_descriptor);

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

        Ok(())
    }
}
