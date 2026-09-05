use super::*;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCQuestion {
    respond_to: RespondTo,
    detail: RPCQuestionDetail,
}

impl RPCQuestion {
    pub fn new(respond_to: RespondTo, detail: RPCQuestionDetail) -> Self {
        Self { respond_to, detail }
    }
    pub async fn validate(
        &self,
        validate_context: &RPCValidateContext<'_>,
    ) -> Result<(), RPCError> {
        self.detail.validate(validate_context).await
    }
    pub fn respond_to(&self) -> &RespondTo {
        &self.respond_to
    }
    pub fn detail(&self) -> &RPCQuestionDetail {
        &self.detail
    }
    pub fn desc(&self) -> &'static str {
        self.detail.desc()
    }
    pub fn destructure(self) -> (RespondTo, RPCQuestionDetail) {
        (self.respond_to, self.detail)
    }
    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::question::Reader,
    ) -> Result<RPCQuestion, RPCError> {
        let rt_reader = reader.get_respond_to();
        let respond_to = RespondTo::decode(decode_context, &rt_reader)?;
        let d_reader = reader.get_detail();
        let detail = RPCQuestionDetail::decode(decode_context, &d_reader)?;
        Ok(RPCQuestion { respond_to, detail })
    }
    pub fn encode(&self, builder: &mut veilid_capnp::question::Builder) -> Result<(), RPCError> {
        self.respond_to
            .encode(&mut builder.reborrow().init_respond_to())?;
        self.detail.encode(&mut builder.reborrow().init_detail())?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) enum RPCQuestionDetail {
    StatusQ(Box<RPCOperationStatusQ>),
    FindNodeQ(Box<RPCOperationFindNodeQ>),
    AppCallQ(Box<RPCOperationAppCallQ>),
    GetValueQ(Box<RPCOperationGetValueQ>),
    SetValueQ(Box<RPCOperationSetValueQ>),
    WatchValueQ(Box<RPCOperationWatchValueQ>),
    InspectValueQ(Box<RPCOperationInspectValueQ>),
    TransactBeginQ(Box<RPCOperationTransactBeginQ>),
    TransactCommandQ(Box<RPCOperationTransactCommandQ>),
    #[cfg(feature = "unstable-blockstore")]
    SupplyBlockQ(Box<RPCOperationSupplyBlockQ>),
    #[cfg(feature = "unstable-blockstore")]
    FindBlockQ(Box<RPCOperationFindBlockQ>),
    #[cfg(feature = "unstable-tunnels")]
    StartTunnelQ(Box<RPCOperationStartTunnelQ>),
    #[cfg(feature = "unstable-tunnels")]
    CompleteTunnelQ(Box<RPCOperationCompleteTunnelQ>),
    #[cfg(feature = "unstable-tunnels")]
    CancelTunnelQ(Box<RPCOperationCancelTunnelQ>),
}

impl RPCQuestionDetail {
    pub fn desc(&self) -> &'static str {
        match self {
            RPCQuestionDetail::StatusQ(_) => "StatusQ",
            RPCQuestionDetail::FindNodeQ(_) => "FindNodeQ",
            RPCQuestionDetail::AppCallQ(_) => "AppCallQ",
            RPCQuestionDetail::GetValueQ(_) => "GetValueQ",
            RPCQuestionDetail::SetValueQ(_) => "SetValueQ",
            RPCQuestionDetail::WatchValueQ(_) => "WatchValueQ",
            RPCQuestionDetail::InspectValueQ(_) => "InspectValueQ",
            RPCQuestionDetail::TransactBeginQ(_) => "TransactBeginQ",
            RPCQuestionDetail::TransactCommandQ(_) => "TransactCommandQ",
            #[cfg(feature = "unstable-blockstore")]
            RPCQuestionDetail::SupplyBlockQ(_) => "SupplyBlockQ",
            #[cfg(feature = "unstable-blockstore")]
            RPCQuestionDetail::FindBlockQ(_) => "FindBlockQ",
            #[cfg(feature = "unstable-tunnels")]
            RPCQuestionDetail::StartTunnelQ(_) => "StartTunnelQ",
            #[cfg(feature = "unstable-tunnels")]
            RPCQuestionDetail::CompleteTunnelQ(_) => "CompleteTunnelQ",
            #[cfg(feature = "unstable-tunnels")]
            RPCQuestionDetail::CancelTunnelQ(_) => "CancelTunnelQ",
        }
    }
    pub async fn validate(
        &self,
        validate_context: &RPCValidateContext<'_>,
    ) -> Result<(), RPCError> {
        match self {
            RPCQuestionDetail::StatusQ(_r) => Ok(()), // r.validate(validate_context).await,
            RPCQuestionDetail::FindNodeQ(_r) => Ok(()), // r.validate(validate_context).await,
            RPCQuestionDetail::AppCallQ(_r) => Ok(()), // r.validate(validate_context).await,
            RPCQuestionDetail::GetValueQ(_r) => Ok(()), // r.validate(validate_context).await,
            RPCQuestionDetail::SetValueQ(_r) => Ok(()), // r.validate(validate_context).await,
            RPCQuestionDetail::WatchValueQ(_r) => Ok(()), // r.validate(validate_context).await,
            RPCQuestionDetail::InspectValueQ(_r) => Ok(()), // r.validate(validate_context).await,
            RPCQuestionDetail::TransactBeginQ(r) => r.validate(validate_context).await,
            RPCQuestionDetail::TransactCommandQ(r) => r.validate(validate_context),
            #[cfg(feature = "unstable-blockstore")]
            RPCQuestionDetail::SupplyBlockQ(r) => Ok(()), // r.validate(validate_context).await,
            #[cfg(feature = "unstable-blockstore")]
            RPCQuestionDetail::FindBlockQ(r) => Ok(()), // r.validate(validate_context).await,
            #[cfg(feature = "unstable-tunnels")]
            RPCQuestionDetail::StartTunnelQ(r) => Ok(()), // r.validate(validate_context).await,
            #[cfg(feature = "unstable-tunnels")]
            RPCQuestionDetail::CompleteTunnelQ(r) => Ok(()), // r.validate(validate_context).await,
            #[cfg(feature = "unstable-tunnels")]
            RPCQuestionDetail::CancelTunnelQ(r) => Ok(()), // r.validate(validate_context).await,
        }
    }

    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::question::detail::Reader,
    ) -> Result<RPCQuestionDetail, RPCError> {
        let which_reader = reader.which()?;
        let out = match which_reader {
            veilid_capnp::question::detail::StatusQ(r) => {
                let op_reader = r?;
                let out = RPCOperationStatusQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::StatusQ(Box::new(out))
            }
            veilid_capnp::question::detail::FindNodeQ(r) => {
                let op_reader = r?;
                let out = RPCOperationFindNodeQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::FindNodeQ(Box::new(out))
            }
            veilid_capnp::question::detail::Which::AppCallQ(r) => {
                let op_reader = r?;
                let out = RPCOperationAppCallQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::AppCallQ(Box::new(out))
            }
            veilid_capnp::question::detail::GetValueQ(r) => {
                let op_reader = r?;
                let out = RPCOperationGetValueQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::GetValueQ(Box::new(out))
            }
            veilid_capnp::question::detail::SetValueQ(r) => {
                let op_reader = r?;
                let out = RPCOperationSetValueQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::SetValueQ(Box::new(out))
            }
            veilid_capnp::question::detail::WatchValueQ(r) => {
                let op_reader = r?;
                let out = RPCOperationWatchValueQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::WatchValueQ(Box::new(out))
            }
            veilid_capnp::question::detail::InspectValueQ(r) => {
                let op_reader = r?;
                let out = RPCOperationInspectValueQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::InspectValueQ(Box::new(out))
            }
            veilid_capnp::question::detail::TransactBeginQ(r) => {
                let op_reader = r?;
                let out = RPCOperationTransactBeginQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::TransactBeginQ(Box::new(out))
            }
            veilid_capnp::question::detail::TransactCommandQ(r) => {
                let op_reader = r?;
                let out = RPCOperationTransactCommandQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::TransactCommandQ(Box::new(out))
            }
            #[cfg(feature = "unstable-blockstore")]
            veilid_capnp::question::detail::SupplyBlockQ(r) => {
                let op_reader = r?;
                let out = RPCOperationSupplyBlockQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::SupplyBlockQ(Box::new(out))
            }
            #[cfg(feature = "unstable-blockstore")]
            veilid_capnp::question::detail::FindBlockQ(r) => {
                let op_reader = r?;
                let out = RPCOperationFindBlockQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::FindBlockQ(Box::new(out))
            }
            #[cfg(feature = "unstable-tunnels")]
            veilid_capnp::question::detail::StartTunnelQ(r) => {
                let op_reader = r?;
                let out = RPCOperationStartTunnelQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::StartTunnelQ(Box::new(out))
            }
            #[cfg(feature = "unstable-tunnels")]
            veilid_capnp::question::detail::CompleteTunnelQ(r) => {
                let op_reader = r?;
                let out = RPCOperationCompleteTunnelQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::CompleteTunnelQ(Box::new(out))
            }
            #[cfg(feature = "unstable-tunnels")]
            veilid_capnp::question::detail::CancelTunnelQ(r) => {
                let op_reader = r?;
                let out = RPCOperationCancelTunnelQ::decode(decode_context, &op_reader)?;
                RPCQuestionDetail::CancelTunnelQ(Box::new(out))
            }
        };
        Ok(out)
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::question::detail::Builder,
    ) -> Result<(), RPCError> {
        match self {
            RPCQuestionDetail::StatusQ(d) => d.encode(&mut builder.reborrow().init_status_q()),
            RPCQuestionDetail::FindNodeQ(d) => d.encode(&mut builder.reborrow().init_find_node_q()),
            RPCQuestionDetail::AppCallQ(d) => d.encode(&mut builder.reborrow().init_app_call_q()),
            RPCQuestionDetail::GetValueQ(d) => d.encode(&mut builder.reborrow().init_get_value_q()),
            RPCQuestionDetail::SetValueQ(d) => d.encode(&mut builder.reborrow().init_set_value_q()),
            RPCQuestionDetail::WatchValueQ(d) => {
                d.encode(&mut builder.reborrow().init_watch_value_q())
            }
            RPCQuestionDetail::InspectValueQ(d) => {
                d.encode(&mut builder.reborrow().init_inspect_value_q())
            }
            RPCQuestionDetail::TransactBeginQ(d) => {
                d.encode(&mut builder.reborrow().init_transact_begin_q())
            }
            RPCQuestionDetail::TransactCommandQ(d) => {
                d.encode(&mut builder.reborrow().init_transact_command_q())
            }
            #[cfg(feature = "unstable-blockstore")]
            RPCQuestionDetail::SupplyBlockQ(d) => {
                d.encode(&mut builder.reborrow().init_supply_block_q())
            }
            #[cfg(feature = "unstable-blockstore")]
            RPCQuestionDetail::FindBlockQ(d) => {
                d.encode(&mut builder.reborrow().init_find_block_q())
            }
            #[cfg(feature = "unstable-tunnels")]
            RPCQuestionDetail::StartTunnelQ(d) => {
                d.encode(&mut builder.reborrow().init_start_tunnel_q())
            }
            #[cfg(feature = "unstable-tunnels")]
            RPCQuestionDetail::CompleteTunnelQ(d) => {
                d.encode(&mut builder.reborrow().init_complete_tunnel_q())
            }
            #[cfg(feature = "unstable-tunnels")]
            RPCQuestionDetail::CancelTunnelQ(d) => {
                d.encode(&mut builder.reborrow().init_cancel_tunnel_q())
            }
        }
    }
}
