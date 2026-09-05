use super::*;

const MAX_APP_CALL_Q_MESSAGE_LEN: usize = 32768;
const MAX_APP_CALL_A_MESSAGE_LEN: usize = 32768;

#[derive(Clone)]
pub(in crate::rpc_processor) struct RPCOperationAppCallQ {
    message: Bytes,
}

impl fmt::Debug for RPCOperationAppCallQ {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RPCOperationAppCallQ")
            .field("message(len)", &self.message.len())
            .field("message", &human_byte_data(&self.message, Some(64)))
            .finish()
    }
}

impl RPCOperationAppCallQ {
    pub fn new(message: Bytes) -> Result<Self, RPCError> {
        if message.len() > MAX_APP_CALL_Q_MESSAGE_LEN {
            return Err(RPCError::internal("AppCallQ message too long to set"));
        }
        Ok(Self { message })
    }
    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(self) -> Bytes {
        self.message
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_app_call_q::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, message);
        let mr = reader.get_message()?;
        rpc_ignore_max_len!(mr, MAX_APP_CALL_Q_MESSAGE_LEN);

        RPCOperationAppCallQ::new(Bytes::copy_from_slice(mr))
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_app_call_q::Builder,
    ) -> Result<(), RPCError> {
        builder.set_message(&self.message);
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub(in crate::rpc_processor) struct RPCOperationAppCallA {
    message: Bytes,
}

impl fmt::Debug for RPCOperationAppCallA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RPCOperationAppCallA")
            .field("message(len)", &self.message.len())
            .field("message", &human_byte_data(&self.message, Some(64)))
            .finish()
    }
}

impl RPCOperationAppCallA {
    pub fn new(message: Bytes) -> Result<Self, RPCError> {
        if message.len() > MAX_APP_CALL_A_MESSAGE_LEN {
            return Err(RPCError::ignore("AppCallA message too long to set"));
        }
        Ok(Self { message })
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(self) -> Bytes {
        self.message
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_app_call_a::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, message);
        let mr = reader.get_message()?;
        rpc_ignore_max_len!(mr, MAX_APP_CALL_A_MESSAGE_LEN);
        Self::new(Bytes::copy_from_slice(mr))
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_app_call_a::Builder,
    ) -> Result<(), RPCError> {
        builder.set_message(&self.message);
        Ok(())
    }
}
