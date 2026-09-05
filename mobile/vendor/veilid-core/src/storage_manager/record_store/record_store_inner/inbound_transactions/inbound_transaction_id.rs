use super::*;

impl_veilid_log_facility!("stor");

/// An individual transaction id
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InboundTransactionId(u64);

impl InboundTransactionId {
    pub(super) fn new(raw_id: u64) -> VeilidAPIResult<Self> {
        if raw_id == 0 {
            apibail_internal!("invalid transaction id");
        }

        Ok(Self(raw_id))
    }
}

impl fmt::Display for InboundTransactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<InboundTransactionId> for u64 {
    fn from(value: InboundTransactionId) -> Self {
        value.0
    }
}
