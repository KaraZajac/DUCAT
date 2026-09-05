use super::*;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCSignerSignature {
    pub signer: PublicKey,
    pub signature: Signature,
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCSigningParams {
    pub signer_keypair: KeyPair,
    pub destination_key: PublicKey,
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCSignedOperation {
    operation_data: Bytes,
    signer_signature: Option<RPCSignerSignature>,
}

impl RPCSignedOperation {
    pub fn new(operation_data: Bytes, signer_signature: Option<RPCSignerSignature>) -> Self {
        Self {
            operation_data,
            signer_signature,
        }
    }

    pub async fn sign(
        crypto: &Crypto,
        operation: &RPCOperation,
        signing_params: Option<RPCSigningParams>,
    ) -> Result<Self, RPCError> {
        let operation_data = {
            let mut message_builder = ::capnp::message::Builder::new_default();
            let mut builder = message_builder.init_root::<veilid_capnp::operation::Builder>();
            operation.encode(&mut builder)?;
            canonical_message_builder_to_bytes_writer_unpacked(message_builder, |size| {
                BytesWriter::with_capacity(size)
            })?
            .into_inner()
            .freeze()
        };

        // Optionally, sign the operation with a signer
        if let Some(signing_params) = signing_params {
            let Some(vcrypto) = crypto.get_async(signing_params.signer_keypair.kind()) else {
                return Err(RPCError::protocol("unsupported cryptosystem"));
            };

            // bind operation data to destination key
            let mut signature_data: Vec<u8> = signing_params.destination_key.into();
            signature_data.extend_from_slice(&operation_data);

            let signature = vcrypto
                .sign(
                    &signing_params.signer_keypair.key(),
                    &signing_params.signer_keypair.secret(),
                    signature_data.into(),
                )
                .await
                .map_err(RPCError::protocol)?;

            Ok(Self::new(
                operation_data,
                Some(RPCSignerSignature {
                    signer: signing_params.signer_keypair.key(),
                    signature,
                }),
            ))
        } else {
            Ok(Self::new(operation_data, None))
        }
    }

    pub async fn validate(
        &self,
        crypto: &Crypto,
        destination_key: PublicKey,
    ) -> Result<(), RPCError> {
        // Validate the signer signature
        if let Some(signer_signature) = &self.signer_signature {
            let Some(vcrypto) = crypto.get_async(signer_signature.signer.kind()) else {
                return Err(RPCError::protocol("unsupported cryptosystem"));
            };

            // bind operation data to destination key
            let mut signature_data: Vec<u8> = destination_key.into();
            signature_data.extend_from_slice(&self.operation_data);

            if !vcrypto
                .verify(
                    &signer_signature.signer,
                    signature_data.into(),
                    &signer_signature.signature,
                )
                .await
                .map_err(RPCError::protocol)?
            {
                return Err(RPCError::protocol("failed to validate signer signature"));
            }
        }
        Ok(())
    }

    pub fn signer_signature(&self) -> Option<&RPCSignerSignature> {
        self.signer_signature.as_ref()
    }

    pub fn decode_operation(
        &self,
        decode_context: &RPCDecodeContext,
    ) -> Result<RPCOperation, RPCError> {
        let mut operation_data_cursor = &mut &self.operation_data[..];
        let tmp_reader = capnp::serialize::read_message(
            &mut operation_data_cursor,
            capnp::message::ReaderOptions::new(),
        )?;
        let operation_reader = tmp_reader.get_root::<veilid_capnp::operation::Reader>()?;
        let operation = RPCOperation::decode(decode_context, &operation_reader)?;

        Ok(operation)
    }

    pub fn decode(
        _decode_context: &RPCDecodeContext,
        reader: &veilid_capnp::signed_operation::Reader,
    ) -> Result<Self, RPCError> {
        rpc_ignore_missing_property!(reader, operation_data);
        let operation_data = Bytes::copy_from_slice(reader.get_operation_data()?);

        if reader.has_signer() {
            let s_reader = reader.get_signer()?;
            let signer = decode_public_key(&s_reader)?;

            rpc_ignore_missing_property!(reader, signature);
            let sig_reader = reader.get_signature()?;
            let signature = decode_signature(&sig_reader)?;
            Ok(Self {
                operation_data,
                signer_signature: Some(RPCSignerSignature { signer, signature }),
            })
        } else {
            Ok(Self {
                operation_data,
                signer_signature: None,
            })
        }
    }

    pub fn encode(
        &self,
        builder: &mut veilid_capnp::signed_operation::Builder,
    ) -> Result<(), RPCError> {
        builder.set_operation_data(&self.operation_data);
        if let Some(signer_signature) = &self.signer_signature {
            let mut s_builder = builder.reborrow().init_signer();
            encode_public_key(&signer_signature.signer, &mut s_builder);

            let mut sig_builder = builder.reborrow().init_signature();
            encode_signature(&signer_signature.signature, &mut sig_builder);
        }

        Ok(())
    }
}
