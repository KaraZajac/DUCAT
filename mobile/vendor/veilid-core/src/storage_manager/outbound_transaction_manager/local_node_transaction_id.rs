use super::*;

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LocalNodeTransactionId {
    lnxid: u64,
}

impl LocalNodeTransactionId {
    pub fn new(lnxid: u64) -> Self {
        Self { lnxid }
    }
}

impl fmt::Display for LocalNodeTransactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lnxid-{}", self.lnxid)
    }
}

pub type LocalNodeTransactionIdSet = BTreeSet<LocalNodeTransactionId>;
