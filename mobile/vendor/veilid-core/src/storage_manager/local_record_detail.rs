use super::*;

/// Information about nodes that cache a local record remotely
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, GetSize)]
pub(in crate::storage_manager) struct PerNodeRecordDetail {
    pub last_set: Timestamp,
    pub last_seen: Timestamp,
    pub subkeys: ValueSubkeyRangeSet,
}

/// A record's most recent transaction membership: the set it was committed with, plus the signer
/// used (writer if write-opened, else the default signing keypair) so background transactions can be begun
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, GetSize)]
pub(in crate::storage_manager) struct TransactionMembership {
    pub record_set: Vec<OpaqueRecordKey>,
    pub operation_signer: KeyPair,
}

/// Information required to handle locally opened records
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, GetSize)]
pub(in crate::storage_manager) struct LocalRecordDetail {
    /// The last 'safety selection' used when creating/opening this record.
    /// Even when closed, this safety selection applies to re-publication attempts by the system.
    pub safety_selection: SafetySelection,
    /// The nodes that we have seen this record cached on recently
    #[serde(default)]
    pub nodes: HashMap<NodeId, PerNodeRecordDetail>,
    /// Most recent transaction membership; while set, background ops treat the record transactionally
    #[serde(default)]
    pub transaction_membership: Option<TransactionMembership>,
}

impl LocalRecordDetail {
    pub fn new(safety_selection: SafetySelection) -> Self {
        Self {
            safety_selection,
            nodes: Default::default(),
            transaction_membership: None,
        }
    }
}

impl RecordDetail for LocalRecordDetail {
    fn is_new(&self) -> bool {
        self.nodes.is_empty()
    }
}
