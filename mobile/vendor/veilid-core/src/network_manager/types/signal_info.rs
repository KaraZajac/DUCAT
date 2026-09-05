use super::*;

/// Parameter for Signal operation
#[derive(Clone, Debug)]
pub(crate) enum SignalInfo {
    /// UDP Hole Punch Request
    HolePunch {
        /// /// Receipt to be returned after the hole punch
        receipt: Bytes,
        /// Sender's peer info
        peer_info: Arc<PeerInfo>,
        /// Hole punch dial info to use (optional, UDP only if not provided)
        opt_dial_info: Option<DialInfo>,
    },
    /// Reverse Connection Request
    ReverseConnect {
        /// Receipt to be returned by the reverse connection
        receipt: Bytes,
        /// Sender's peer info
        peer_info: Arc<PeerInfo>,
    },
    // XXX: WebRTC
}

impl SignalInfo {
    pub fn validate(&self) -> Result<(), RPCError> {
        match self {
            SignalInfo::HolePunch {
                receipt,
                peer_info,
                opt_dial_info,
            } => {
                if receipt.len() < RCP0_MIN_RECEIPT_SIZE {
                    return Err(RPCError::protocol("SignalInfo HolePunch receipt too short"));
                }
                if receipt.len() > RCP0_MAX_RECEIPT_SIZE {
                    return Err(RPCError::protocol("SignalInfo HolePunch receipt too long"));
                }

                // Dial info, if present, must be present in the peer info as well
                if let Some(dial_info) = &opt_dial_info {
                    peer_info
                        .node_info()
                        .dial_info_detail_list()
                        .iter()
                        .find(|d| d.dial_info == *dial_info)
                        .ok_or(RPCError::protocol("Dial info not found in peer info"))?;
                }

                Ok(())
            }
            SignalInfo::ReverseConnect {
                receipt,
                peer_info: _,
            } => {
                if receipt.len() < RCP0_MIN_RECEIPT_SIZE {
                    return Err(RPCError::protocol(
                        "SignalInfo ReverseConnect receipt too short",
                    ));
                }
                if receipt.len() > RCP0_MAX_RECEIPT_SIZE {
                    return Err(RPCError::protocol(
                        "SignalInfo ReverseConnect receipt too long",
                    ));
                }
                Ok(())
            }
        }
    }
}
