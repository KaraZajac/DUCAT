use super::*;

#[derive(Clone)]
pub(in crate::rpc_processor) struct RoutedOperation {
    origin_routing_domain: RoutingDomain,
    sequencing: Sequencing,
    signatures: Vec<Signature>,
    nonce: Nonce,
    data: Bytes,
}

impl fmt::Debug for RoutedOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoutedOperation")
            .field("origin_routing_domain", &self.origin_routing_domain)
            .field("sequencing", &self.sequencing)
            .field("signatures.len", &self.signatures.len())
            .field("nonce", &self.nonce)
            .field("data(len)", &self.data.len())
            .finish()
    }
}

impl RoutedOperation {
    pub fn new(
        routing_domain: RoutingDomain,
        sequencing: Sequencing,
        signatures: Vec<Signature>,
        nonce: Nonce,
        data: Bytes,
    ) -> Result<Self, RPCError> {
        if signatures.len() > MAX_CRYPTO_KINDS {
            return Err(RPCError::protocol("too many signatures"));
        }
        Ok(Self {
            origin_routing_domain: routing_domain,
            sequencing,
            signatures,
            nonce,
            data,
        })
    }

    // pub async fn validate(
    //     &self,
    //     _validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     Ok(())
    // }

    pub fn routing_domain(&self) -> RoutingDomain {
        self.origin_routing_domain
    }
    pub fn sequencing(&self) -> Sequencing {
        self.sequencing
    }
    pub fn signatures(&self) -> &[Signature] {
        &self.signatures
    }

    pub fn add_signature(&mut self, signature: Signature) -> Result<(), RPCError> {
        if self.signatures.len() >= MAX_CRYPTO_KINDS {
            return Err(RPCError::protocol("too many signatures"));
        }
        self.signatures.push(signature);
        Ok(())
    }

    pub fn nonce(&self) -> &Nonce {
        &self.nonce
    }
    pub fn data(&self) -> Bytes {
        self.data.clone()
    }

    // pub fn destructure(self) -> (Sequencing, Vec<BareSignature>, BareNonce, Vec<u8>) {
    //     (self.sequencing, self.signatures, self.nonce, self.data)
    // }

    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::routed_operation::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, signatures);
        let sigs_reader = reader.get_signatures()?;
        let mut signatures = Vec::<Signature>::with_capacity(
            sigs_reader
                .len()
                .try_into()
                .map_err(RPCError::map_internal("too many signatures"))?,
        );
        for s in sigs_reader.iter() {
            let Some(sig) = decode_signature(&s).ignore_ok()? else {
                continue;
            };
            signatures.push(sig);
        }

        let sequencing = decode_sequencing(reader.get_sequencing())?;
        rpc_ignore_missing_property!(reader, nonce);
        let n_reader = reader.get_nonce()?;
        let nonce = decode_nonce(&n_reader)?;
        rpc_ignore_missing_property!(reader, data);
        let data = Bytes::copy_from_slice(reader.get_data()?);

        Self::new(
            decode_context.origin_routing_domain,
            sequencing,
            signatures,
            nonce,
            data,
        )
    }

    pub fn encode(
        &self,
        builder: &mut veilid_capnp::routed_operation::Builder,
    ) -> Result<(), RPCError> {
        builder
            .reborrow()
            .set_sequencing(encode_sequencing(self.sequencing));
        let mut sigs_builder = builder.reborrow().init_signatures(
            self.signatures
                .len()
                .try_into()
                .map_err(RPCError::map_internal("invalid signatures list length"))?,
        );
        for (i, sig) in self.signatures.iter().enumerate() {
            let mut sig_builder = sigs_builder.reborrow().get(i as u32);
            encode_signature(sig, &mut sig_builder);
        }
        let mut n_builder = builder.reborrow().init_nonce();
        encode_nonce(&self.nonce, &mut n_builder);
        builder.set_data(&self.data);

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCOperationRoute {
    safety_route: Arc<SafetyRoute>,
    operation: RoutedOperation,
}

impl RPCOperationRoute {
    pub fn new(safety_route: Arc<SafetyRoute>, operation: RoutedOperation) -> Self {
        Self {
            safety_route,
            operation,
        }
    }

    // pub async fn validate(
    //     &self,
    //     validate_context: &RPCValidateContext<'_>,
    // ) -> Result<(), RPCError> {
    //     self.operation.validate(validate_context).await
    // }

    pub fn destructure(self) -> (Arc<SafetyRoute>, RoutedOperation) {
        (self.safety_route, self.operation)
    }

    pub fn decode(
        decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::operation_route::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, safety_route);
        let sr_reader = reader.get_safety_route()?;
        let safety_route = Arc::new(decode_safety_route(decode_context, &sr_reader)?);

        rpc_ignore_missing_property!(reader, operation);
        let o_reader = reader.get_operation()?;
        let operation = RoutedOperation::decode(decode_context, &o_reader)?;

        Ok(Self {
            safety_route,
            operation,
        })
    }
    pub fn encode(
        &self,
        builder: &mut veilid_capnp::operation_route::Builder,
    ) -> Result<(), RPCError> {
        let mut sr_builder = builder.reborrow().init_safety_route();
        encode_safety_route(&self.safety_route, &mut sr_builder)?;
        let mut o_builder = builder.reborrow().init_operation();
        self.operation.encode(&mut o_builder)?;
        Ok(())
    }
}
