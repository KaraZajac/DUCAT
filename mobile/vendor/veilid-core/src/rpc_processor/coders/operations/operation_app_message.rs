use super::*;

const MAX_APP_MESSAGE_MESSAGE_LEN: usize = 32768;

#[derive(Clone)]
pub(in crate::rpc_processor) struct RPCOperationAppMessage {
    message: Bytes,
}

impl fmt::Debug for RPCOperationAppMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RPCOperationAppMessage")
            .field("message(len)", &self.message.len())
            .field("message", &human_byte_data(&self.message, Some(64)))
            .finish()
    }
}

impl RPCOperationAppMessage {
    pub fn new(message: Bytes) -> Result<Self, RPCError> {
        if message.len() > MAX_APP_MESSAGE_MESSAGE_LEN {
            return Err(RPCError::internal("AppMessage message too long to set"));
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
        reader: &veilid_capnp::operation_app_message::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, message);
        let mr = reader.get_message()?;
        rpc_ignore_max_len!(mr, MAX_APP_MESSAGE_MESSAGE_LEN);
        Self::new(Bytes::copy_from_slice(mr))
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_app_message::Builder,
    ) -> Result<(), RPCError> {
        builder.set_message(&self.message);
        Ok(())
    }
}
