use super::*;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) enum RespondTo {
    Sender,
    PrivateRoute(Arc<PrivateRoute>),
}

impl RespondTo {
    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::question::respond_to::Reader,
    ) -> Result<Self, RPCError> {
        let respond_to = match reader.which()? {
            veilid_capnp::question::respond_to::Sender(()) => RespondTo::Sender,
            veilid_capnp::question::respond_to::PrivateRoute(pr_reader) => {
                let pr_reader = pr_reader?;
                let pr = decode_private_route(decode_context, &pr_reader)?;
                RespondTo::PrivateRoute(Arc::new(pr))
            }
        };
        Ok(respond_to)
    }

    pub fn encode(
        &self,
        builder: &mut veilid_capnp::question::respond_to::Builder,
    ) -> Result<(), RPCError> {
        match self {
            Self::Sender => {
                builder.reborrow().set_sender(());
            }
            Self::PrivateRoute(pr) => {
                let mut pr_builder = builder.reborrow().init_private_route();
                encode_private_route(pr, &mut pr_builder)?;
            }
        };
        Ok(())
    }
}
