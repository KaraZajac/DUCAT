use super::*;

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OutboundTransactionHandle {
    txid: u64,
}

impl OutboundTransactionHandle {
    pub(super) fn new(txid: u64) -> Self {
        Self { txid }
    }
}

impl fmt::Display for OutboundTransactionHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "txid-{}", self.txid)
    }
}
