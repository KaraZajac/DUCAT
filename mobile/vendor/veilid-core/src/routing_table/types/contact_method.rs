use super::*;

/// Mechanism required to contact another node
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ContactMethod {
    /// Connection should have already existed
    Existing,
    /// Contact the node directly
    Direct { target_di: DialInfo },
    /// Request via signal the node connect back directly
    SignalReverse { relay_di: DialInfo },
    /// Request via signal the node negotiate a hole punch
    SignalHolePunch {
        relay_di: DialInfo,
        hole_punch_di: DialInfo,
        reverse_hole_punch_di: DialInfo,
    },
    /// Must use an inbound relay to reach the node
    InboundRelay { relay_di: DialInfo },
    /// Must use outbound relay to reach the node
    OutboundRelay { relay_di: DialInfo },
}

impl fmt::Display for ContactMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContactMethod::Existing => write!(f, "Existing"),
            ContactMethod::Direct { target_di } => {
                write!(f, "Direct({})", f.to_string(target_di))
            }
            ContactMethod::SignalReverse { relay_di } => {
                write!(f, "SignalReverse(relay={})", f.to_string(relay_di))
            }
            ContactMethod::SignalHolePunch {
                relay_di,
                hole_punch_di,
                reverse_hole_punch_di,
            } => write!(
                f,
                "SignalHolePunch(relay={}, hole_punch={}, reverse_hole_punch={})",
                f.to_string(relay_di),
                f.to_string(hole_punch_di),
                f.to_string(reverse_hole_punch_di),
            ),
            ContactMethod::InboundRelay { relay_di } => {
                write!(f, "InboundRelay(relay={})", f.to_string(relay_di))
            }
            ContactMethod::OutboundRelay { relay_di } => {
                write!(f, "OutboundRelay(relay={})", f.to_string(relay_di))
            }
        }
    }
}

impl ContactMethod {
    pub fn direct_dial_info(&self) -> Option<DialInfo> {
        match &self {
            ContactMethod::Direct { target_di } => Some(target_di.clone()),
            _ => None,
        }
    }

    /// Transport credited to the target's per-transport stats for this send attempt.
    /// A target accepts responsibility for the relay it published, so we always
    /// attribute to the target — never to the relay.
    pub fn opt_transport_type(&self) -> Option<TransportType> {
        match self {
            ContactMethod::Existing => None,
            ContactMethod::Direct { target_di } => Some(TransportType::from(target_di)),
            ContactMethod::InboundRelay { relay_di }
            | ContactMethod::OutboundRelay { relay_di }
            | ContactMethod::SignalReverse { relay_di }
            | ContactMethod::SignalHolePunch { relay_di, .. } => {
                Some(TransportType::from(relay_di))
            }
        }
    }
}

/// How to request a contact method
pub struct ContactMethodRequest {
    pub peer_a: Arc<PeerInfo>,
    pub peer_a_published: bool,
    pub peer_b: Arc<PeerInfo>,
    pub dial_info_filter: DialInfoFilter,
    pub sequencing: Sequencing,
}

impl fmt::Debug for ContactMethodRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContactMethodRequest")
            .field("peer_a", &self.peer_a)
            .field("peer_a_published", &self.peer_a_published)
            .field("peer_b", &self.peer_b)
            .field("dial_info_filter", &self.dial_info_filter)
            .field("sequencing", &self.sequencing)
            .finish()
    }
}
