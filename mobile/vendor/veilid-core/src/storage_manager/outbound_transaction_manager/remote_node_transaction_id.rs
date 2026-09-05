use super::*;

/// Server-side transaction id and node id pair
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteNodeTransactionId {
    node_id: NodeId,
    xid: u64,
}

impl RemoteNodeTransactionId {
    pub fn new(node_id: NodeId, xid: u64) -> Self {
        Self { node_id, xid }
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id.clone()
    }

    pub fn xid(&self) -> u64 {
        self.xid
    }
}

impl fmt::Display for RemoteNodeTransactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rnxid-{}:{}", self.node_id, self.xid)
    }
}
