use super::*;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationValidateDialInfo {
    dial_info: DialInfo,
    receipt: Bytes,
    redirect: bool,
}

impl RPCOperationValidateDialInfo {
    pub fn new(dial_info: DialInfo, receipt: Bytes, redirect: bool) -> Result<Self, RPCError> {
        if receipt.len() < RCP0_MIN_RECEIPT_SIZE {
            return Err(RPCError::internal(
                "ValidateDialInfo receipt too short to set",
            ));
        }
        if receipt.len() > RCP0_MAX_RECEIPT_SIZE {
            return Err(RPCError::internal(
                "ValidateDialInfo receipt too long to set",
            ));
        }

        Ok(Self {
            dial_info,
            receipt,
            redirect,
        })
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(self) -> (DialInfo, Bytes, bool) {
        (self.dial_info, self.receipt, self.redirect)
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_validate_dial_info::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, dial_info);
        let di_reader = reader.get_dial_info()?;
        let dial_info = decode_dial_info(&di_reader)?;

        rpc_ignore_missing_property!(reader, receipt);
        let rcpt_reader = reader.get_receipt()?;
        rpc_ignore_min_max_len!(rcpt_reader, RCP0_MIN_RECEIPT_SIZE, RCP0_MAX_RECEIPT_SIZE);

        let receipt = Bytes::copy_from_slice(rcpt_reader);
        let redirect = reader.get_redirect();

        Self::new(dial_info, receipt, redirect)
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_validate_dial_info::Builder,
    ) -> Result<(), RPCError> {
        let mut di_builder = builder.reborrow().init_dial_info();
        encode_dial_info(&self.dial_info, &mut di_builder)?;
        builder.set_receipt(&self.receipt);
        builder.set_redirect(self.redirect);
        Ok(())
    }
}
