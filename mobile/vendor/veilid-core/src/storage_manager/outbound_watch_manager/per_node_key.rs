use super::*;

/// A pair of a record key and the node that is caching it
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub(in crate::storage_manager) struct PerNodeKey {
    /// Record key
    pub record_key: RecordKey,
    /// Remote node caching the record key
    pub node_id: NodeId,
}

impl fmt::Display for PerNodeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.record_key, self.node_id)
    }
}

impl FromStr for PerNodeKey {
    type Err = VeilidAPIError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (rkey, nid) = s
            .split_once('@')
            .ok_or_else(|| VeilidAPIError::parse_error("invalid per-node key", s))?;
        Ok(PerNodeKey {
            record_key: RecordKey::from_str(rkey)?,
            node_id: NodeId::from_str(nid)?,
        })
    }
}
