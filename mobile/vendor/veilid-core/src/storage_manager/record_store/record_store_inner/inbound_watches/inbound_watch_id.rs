use super::*;

/// An individual watch id
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InboundWatchId(u64);

impl InboundWatchId {
    pub(super) fn new(raw_id: u64) -> VeilidAPIResult<Self> {
        if raw_id == 0 {
            apibail_internal!("invalid watch id");
        }

        Ok(Self(raw_id))
    }
}

impl fmt::Display for InboundWatchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<InboundWatchId> for u64 {
    fn from(value: InboundWatchId) -> Self {
        value.0
    }
}
