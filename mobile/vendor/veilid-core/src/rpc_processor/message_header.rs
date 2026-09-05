use super::*;

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCMessageHeaderDetailDirect {
    /// The decoded header of the envelope
    pub envelope: Envelope,
    /// The noderef of the original peer that sent the message (not the relay if it is relayed)
    /// Ensures node doesn't get evicted from routing table until we're done with it
    /// Should be filtered to the routing domain of the peer that we received from
    /// If this is part of a route it is the noderef of the hop closest to our end of the route
    pub sender_noderef: FilteredNodeRef,
    /// The flow from the peer sent the message (possibly a relay)
    pub flow: Flow,
    /// The routing domain of the peer that we received from
    pub routing_domain: RoutingDomain,
}

/// Header details for rpc messages received over only a safety route but not a private route
#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCMessageHeaderDetailSafetyRouted {
    /// Direct header
    pub direct: RPCMessageHeaderDetailDirect,
    /// Remote safety route used
    pub remote_safety_route: PublicKey,
    /// The sequencing used for this route
    pub sequencing: Sequencing,
    /// The route operation id this message came from
    pub route_op_id: OperationId,
}

/// Header details for rpc messages received over a private route
#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct RPCMessageHeaderDetailPrivateRouted {
    /// Direct header
    pub direct: RPCMessageHeaderDetailDirect,
    /// Remote safety route used (or possibly node public key in the case of a stub)
    pub remote_safety_route: PublicKey,
    /// The private route we received the rpc over
    pub private_route: PublicKey,
    // The safety spec for replying to this private routed rpc
    pub safety_spec: SafetySpec,
    /// The route operation id this message came from
    pub route_op_id: OperationId,
}

#[derive(Debug, Clone)]
pub(in crate::rpc_processor) enum RPCMessageHeaderDetail {
    Direct(RPCMessageHeaderDetailDirect),
    SafetyRouted(RPCMessageHeaderDetailSafetyRouted),
    PrivateRouted(RPCMessageHeaderDetailPrivateRouted),
}

impl RPCMessageHeaderDetail {
    pub fn flow(&self) -> Flow {
        match self {
            RPCMessageHeaderDetail::Direct(d) => d.flow,
            RPCMessageHeaderDetail::SafetyRouted(s) => s.direct.flow,
            RPCMessageHeaderDetail::PrivateRouted(p) => p.direct.flow,
        }
    }
}

/// The decoded header of an RPC message
#[derive(Debug, Clone)]
pub(in crate::rpc_processor) struct MessageHeader {
    /// Time the message was received, not sent
    pub timestamp: Timestamp,
    /// The length in bytes of the rpc message body
    pub body_len: ByteCount,
    /// The header detail depending on which way the message was received
    pub detail: RPCMessageHeaderDetail,
}

impl MessageHeader {
    /// The crypto kind used on the RPC
    pub fn crypto_kind(&self) -> CryptoKind {
        match &self.detail {
            RPCMessageHeaderDetail::Direct(d) => d.envelope.get_crypto_kind(),
            RPCMessageHeaderDetail::SafetyRouted(s) => s.direct.envelope.get_crypto_kind(),
            RPCMessageHeaderDetail::PrivateRouted(p) => p.direct.envelope.get_crypto_kind(),
        }
    }
    pub fn routing_domain(&self) -> RoutingDomain {
        match &self.detail {
            RPCMessageHeaderDetail::Direct(d) => d.routing_domain,
            RPCMessageHeaderDetail::SafetyRouted(s) => s.direct.routing_domain,
            RPCMessageHeaderDetail::PrivateRouted(p) => p.direct.routing_domain,
        }
    }
    pub fn is_private_routed(&self) -> bool {
        match &self.detail {
            RPCMessageHeaderDetail::Direct(_) => false,
            RPCMessageHeaderDetail::SafetyRouted(_) => false,
            RPCMessageHeaderDetail::PrivateRouted(_) => true,
        }
    }

    // pub fn is_safety_routed(&self) -> bool {
    //     // XXX: There is no way to determine if a safety route stub was used to connect to a private route
    //     // XXX: or an actual safety route. If your code depends on this idea, you need to rethink it.
    // }

    #[expect(dead_code)]
    pub fn is_direct(&self) -> bool {
        match &self.detail {
            RPCMessageHeaderDetail::Direct(_) => true,
            RPCMessageHeaderDetail::SafetyRouted(_) => false,
            RPCMessageHeaderDetail::PrivateRouted(_) => false,
        }
    }

    pub fn direct_sender_node_id(&self) -> NodeId {
        match &self.detail {
            RPCMessageHeaderDetail::Direct(d) => d.envelope.get_sender_id(),
            RPCMessageHeaderDetail::SafetyRouted(s) => s.direct.envelope.get_sender_id(),
            RPCMessageHeaderDetail::PrivateRouted(p) => p.direct.envelope.get_sender_id(),
        }
    }

    #[expect(dead_code)]
    pub fn direct_sender_public_key(&self) -> PublicKey {
        match &self.detail {
            RPCMessageHeaderDetail::Direct(d) => d
                .sender_noderef
                .public_keys(d.routing_domain)
                .get(d.envelope.get_crypto_kind())
                .unwrap_or_log(),
            RPCMessageHeaderDetail::SafetyRouted(s) => s
                .direct
                .sender_noderef
                .public_keys(s.direct.routing_domain)
                .get(s.direct.envelope.get_crypto_kind())
                .unwrap_or_log(),
            RPCMessageHeaderDetail::PrivateRouted(p) => p
                .direct
                .sender_noderef
                .public_keys(p.direct.routing_domain)
                .get(p.direct.envelope.get_crypto_kind())
                .unwrap_or_log(),
        }
    }
    pub fn get_private_route_public_key(&self) -> Option<PublicKey> {
        match &self.detail {
            RPCMessageHeaderDetail::Direct(_) | RPCMessageHeaderDetail::SafetyRouted(_) => None,
            RPCMessageHeaderDetail::PrivateRouted(p) => Some(p.private_route.clone()),
        }
    }

    #[expect(dead_code)]
    pub fn get_safety_route_public_key(&self) -> Option<PublicKey> {
        match &self.detail {
            RPCMessageHeaderDetail::Direct(_) => None,
            RPCMessageHeaderDetail::SafetyRouted(s) => Some(s.remote_safety_route.clone()),
            RPCMessageHeaderDetail::PrivateRouted(p) => Some(p.remote_safety_route.clone()),
        }
    }
}
