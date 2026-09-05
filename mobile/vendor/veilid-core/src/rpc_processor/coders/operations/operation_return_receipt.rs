use super::*;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationReturnReceipt {
    receipt: Bytes,
}

impl RPCOperationReturnReceipt {
    pub fn new(receipt: Bytes) -> Result<Self, RPCError> {
        if receipt.len() < RCP0_MIN_RECEIPT_SIZE {
            return Err(RPCError::protocol("ReturnReceipt receipt too short to set"));
        }
        if receipt.len() > RCP0_MAX_RECEIPT_SIZE {
            return Err(RPCError::protocol("ReturnReceipt receipt too long to set"));
        }

        Ok(Self { receipt })
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn destructure(self) -> Bytes {
        self.receipt
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_return_receipt::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, receipt);
        let rr = reader.get_receipt()?;
        rpc_ignore_min_max_len!(rr, RCP0_MIN_RECEIPT_SIZE, RCP0_MAX_RECEIPT_SIZE);

        Self::new(Bytes::copy_from_slice(rr))
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_return_receipt::Builder,
    ) -> Result<(), RPCError> {
        builder.set_receipt(&self.receipt);
        Ok(())
    }
}
