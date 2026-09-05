use super::*;

#[derive(Debug)]
pub(in crate::rpc_processor) struct MessageData {
    contents: Bytes, // rpc messages must be a canonicalized single segment
}

impl MessageData {
    pub fn new(contents: Bytes) -> Self {
        Self { contents }
    }

    pub fn get_reader(
        &self,
    ) -> Result<capnp::message::Reader<capnp::serialize::OwnedSegments>, RPCError> {
        capnp::serialize_packed::read_message(
            self.contents.as_ref(),
            capnp::message::ReaderOptions::new(),
        )
        .map_err(RPCError::protocol)
    }
}

/// RPC Message with only header decoded, data is still encoded
#[derive(Debug)]
pub(in crate::rpc_processor) struct MessageEncoded {
    /// Decoded RPC message header
    pub header: MessageHeader,
    /// Encoded RPCSignedOperation
    pub data: MessageData,
}

/// Fully decoded and validated RPC message
#[derive(Debug)]
pub(in crate::rpc_processor) struct Message {
    /// Decoded RPC message header
    pub header: MessageHeader,
    /// Decoded RPC operation, extracted from RPCSignedOperation
    pub operation: RPCOperation,
    /// Decoded and validated signer, extracted from RPCSignedOperation
    pub opt_signer: Option<PublicKey>,
    /// Sender noderef if this came from a node
    pub opt_sender_nr: Option<NodeRef>,
}
