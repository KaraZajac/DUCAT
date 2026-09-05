use super::*;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCStatement {
    detail: RPCStatementDetail,
}

impl RPCStatement {
    pub fn new(detail: RPCStatementDetail) -> Self {
        Self { detail }
    }
    pub fn validate(&self, validate_context: &RPCValidateContext<'_>) -> Result<(), RPCError> {
        self.detail.validate(validate_context)
    }
    pub fn detail(&self) -> &RPCStatementDetail {
        &self.detail
    }
    pub fn desc(&self) -> &'static str {
        self.detail.desc()
    }
    pub fn destructure(self) -> RPCStatementDetail {
        self.detail
    }
    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::statement::Reader,
    ) -> Result<RPCStatement, RPCError> {
        let d_reader = reader.get_detail();
        let detail = RPCStatementDetail::decode(decode_context, &d_reader)?;
        Ok(RPCStatement { detail })
    }
    pub fn encode(&self, builder: &mut veilid_capnp::statement::Builder) -> Result<(), RPCError> {
        self.detail.encode(&mut builder.reborrow().init_detail())?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) enum RPCStatementDetail {
    ValidateDialInfo(Box<RPCOperationValidateDialInfo>),
    Route(Box<RPCOperationRoute>),
    ValueChanged(Box<RPCOperationValueChanged>),
    Signal(Box<RPCOperationSignal>),
    ReturnReceipt(Box<RPCOperationReturnReceipt>),
    AppMessage(Box<RPCOperationAppMessage>),
}

impl RPCStatementDetail {
    pub fn desc(&self) -> &'static str {
        match self {
            RPCStatementDetail::ValidateDialInfo(_) => "ValidateDialInfo",
            RPCStatementDetail::Route(_) => "Route",
            RPCStatementDetail::ValueChanged(_) => "ValueChanged",
            RPCStatementDetail::Signal(_) => "Signal",
            RPCStatementDetail::ReturnReceipt(_) => "ReturnReceipt",
            RPCStatementDetail::AppMessage(_) => "AppMessage",
        }
    }
    pub fn validate(&self, validate_context: &RPCValidateContext<'_>) -> Result<(), RPCError> {
        match self {
            RPCStatementDetail::ValidateDialInfo(_r) => Ok(()), // r.validate(validate_context).await,
            RPCStatementDetail::Route(_r) => Ok(()), // r.validate(validate_context).await,
            RPCStatementDetail::ValueChanged(r) => r.validate(validate_context),
            RPCStatementDetail::Signal(r) => r.validate(validate_context),
            RPCStatementDetail::ReturnReceipt(_r) => Ok(()), // r.validate(validate_context).await,
            RPCStatementDetail::AppMessage(_r) => Ok(()),    // r.validate(validate_context).await,
        }
    }
    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::statement::detail::Reader,
    ) -> Result<RPCStatementDetail, RPCError> {
        let which_reader = reader.which()?;
        let out = match which_reader {
            veilid_capnp::statement::detail::ValidateDialInfo(r) => {
                let op_reader = r?;
                let out = RPCOperationValidateDialInfo::decode(decode_context, &op_reader)?;
                RPCStatementDetail::ValidateDialInfo(Box::new(out))
            }
            veilid_capnp::statement::detail::Route(r) => {
                let op_reader = r?;
                let out = RPCOperationRoute::decode(decode_context, &op_reader)?;
                RPCStatementDetail::Route(Box::new(out))
            }
            veilid_capnp::statement::detail::ValueChanged(r) => {
                let op_reader = r?;
                let out = RPCOperationValueChanged::decode(decode_context, &op_reader)?;
                RPCStatementDetail::ValueChanged(Box::new(out))
            }
            veilid_capnp::statement::detail::Signal(r) => {
                let op_reader = r?;
                let out = RPCOperationSignal::decode(decode_context, &op_reader)?;
                RPCStatementDetail::Signal(Box::new(out))
            }
            veilid_capnp::statement::detail::ReturnReceipt(r) => {
                let op_reader = r?;
                let out = RPCOperationReturnReceipt::decode(decode_context, &op_reader)?;
                RPCStatementDetail::ReturnReceipt(Box::new(out))
            }
            veilid_capnp::statement::detail::AppMessage(r) => {
                let op_reader = r?;
                let out = RPCOperationAppMessage::decode(decode_context, &op_reader)?;
                RPCStatementDetail::AppMessage(Box::new(out))
            }
        };
        Ok(out)
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::statement::detail::Builder,
    ) -> Result<(), RPCError> {
        match self {
            RPCStatementDetail::ValidateDialInfo(d) => {
                d.encode(&mut builder.reborrow().init_validate_dial_info())
            }
            RPCStatementDetail::Route(d) => d.encode(&mut builder.reborrow().init_route()),
            RPCStatementDetail::ValueChanged(d) => {
                d.encode(&mut builder.reborrow().init_value_changed())
            }
            RPCStatementDetail::Signal(d) => d.encode(&mut builder.reborrow().init_signal()),
            RPCStatementDetail::ReturnReceipt(d) => {
                d.encode(&mut builder.reborrow().init_return_receipt())
            }
            RPCStatementDetail::AppMessage(d) => {
                d.encode(&mut builder.reborrow().init_app_message())
            }
        }
    }
}
