use super::*;

/// DHT Record Report
#[derive(Default, Clone, PartialOrd, Ord, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(Tsify),
    tsify(from_wasm_abi, into_wasm_abi)
)]
#[cfg_attr(feature = "json-camel-case", serde(rename_all = "camelCase"))]
#[must_use]
pub struct DHTRecordReport {
    /// The actual subkey range within the schema being reported on
    /// This may be a subset of the requested range if it exceeds the schema limits
    /// or has more than `DHTSchema::MAX_SUBKEY_COUNT` (1024) subkeys
    subkeys: ValueSubkeyRangeSet,
    /// The subkeys that have been writen offline that still need to be flushed
    offline_subkeys: ValueSubkeyRangeSet,
    /// The sequence numbers of each subkey requested from a locally stored DHT Record
    local_seqs: Vec<ValueSeqNum>,
    /// The sequence numbers of each subkey requested from the DHT over the network
    network_seqs: Vec<ValueSeqNum>,
}

impl DHTRecordReport {
    pub(crate) fn new(
        subkeys: ValueSubkeyRangeSet,
        offline_subkeys: ValueSubkeyRangeSet,
        local_seqs: Vec<ValueSeqNum>,
        network_seqs: Vec<ValueSeqNum>,
    ) -> VeilidAPIResult<Self> {
        if subkeys.is_full() {
            apibail_invalid_argument!("subkeys range should be exact", "subkeys", subkeys);
        }
        if subkeys.is_empty() {
            apibail_invalid_argument!("subkeys range should not be empty", "subkeys", subkeys);
        }
        if subkeys.len() > DHTSchema::MAX_SUBKEY_COUNT as u64 {
            apibail_invalid_argument!("subkeys range is too large", "subkeys", subkeys);
        }
        if subkeys.len() != local_seqs.len() as u64 {
            apibail_invalid_argument!(
                "local seqs list does not match subkey length",
                "local_seqs",
                local_seqs.len()
            );
        }
        if subkeys.len() != network_seqs.len() as u64 {
            apibail_invalid_argument!(
                "network seqs list does not match subkey length",
                "network_seqs",
                network_seqs.len()
            );
        }
        if !offline_subkeys.is_subset(&subkeys) {
            apibail_invalid_argument!(
                "offline subkeys is not a subset of the whole subkey set",
                "offline_subkeys",
                offline_subkeys
            );
        }

        Ok(Self {
            subkeys,
            offline_subkeys,
            local_seqs,
            network_seqs,
        })
    }

    /// The subkey range the report covers, possibly trimmed from the requested range to fit schema limits
    pub fn subkeys(&self) -> &ValueSubkeyRangeSet {
        &self.subkeys
    }
    /// The subkeys written offline that still need to be flushed to the network
    pub fn offline_subkeys(&self) -> &ValueSubkeyRangeSet {
        &self.offline_subkeys
    }
    /// The local sequence number of each subkey in [DHTRecordReport::subkeys], in ascending subkey order
    #[must_use]
    pub fn local_seqs(&self) -> &[ValueSeqNum] {
        &self.local_seqs
    }
    /// The network sequence number of each subkey in [DHTRecordReport::subkeys], in ascending subkey order
    #[must_use]
    pub fn network_seqs(&self) -> &[ValueSeqNum] {
        &self.network_seqs
    }
    /// The subkeys whose network sequence number is newer than the local copy
    pub fn newer_online_subkeys(&self) -> ValueSubkeyRangeSet {
        let mut newer_online = ValueSubkeyRangeSet::new();
        for ((sk, lseq), nseq) in self
            .subkeys
            .iter()
            .zip(self.local_seqs.iter())
            .zip(self.network_seqs.iter())
        {
            if let Some(nseq) = nseq.to_option() {
                if lseq.is_none() || nseq > lseq.to_option().unwrap_or_log() {
                    newer_online.insert(sk);
                }
            }
        }
        newer_online
    }
}

impl fmt::Debug for DHTRecordReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "DHTRecordReport {{\n  subkeys: {:?}\n  offline_subkeys: {:?}\n  local_seqs: {}\n  remote_seqs: {}\n}}\n",
            &self.subkeys,
            &self.offline_subkeys,
            self.local_seqs.to_table_string(),
            self.network_seqs.to_table_string(),
        )
    }
}

/// DHT Record Report Scope
#[apply(api_data_enum!)]
#[api(
    eq,
    copy,
    ord,
    hash,
    default,
    ts(from_wasm_abi, into_wasm_abi, namespace)
)]
pub enum DHTReportScope {
    /// Return only the local copy sequence numbers
    /// Useful for seeing what subkeys you have locally and which ones have not been retrieved
    #[default]
    Local = 0,
    /// Return the local sequence numbers and the network sequence numbers with GetValue fanout parameters
    /// Provides an independent view of both the local sequence numbers and the network sequence numbers for nodes that
    /// would be reached as if the local copy did not exist locally.
    /// Useful for determining if the current local copy should be updated from the network.
    SyncGet = 1,
    /// Return the local sequence numbers and the network sequence numbers with SetValue fanout parameters
    /// Provides an independent view of both the local sequence numbers and the network sequence numbers for nodes that
    /// would be reached as if the local copy did not exist locally.
    /// Useful for determining if the unchanged local copy should be pushed to the network.
    SyncSet = 2,
    /// Return the local sequence numbers and the network sequence numbers with GetValue fanout parameters
    /// Provides an view of both the local sequence numbers and the network sequence numbers for nodes that
    /// would be reached as if a GetValue operation were being performed, including accepting newer values from the network.
    /// Useful for determining which subkeys would change with a GetValue operation
    UpdateGet = 3,
    /// Return the local sequence numbers and the network sequence numbers with SetValue fanout parameters
    /// Provides an view of both the local sequence numbers and the network sequence numbers for nodes that
    /// would be reached as if a SetValue operation were being performed, including accepting newer values from the network.
    /// This simulates a SetValue with the initial sequence number incremented by 1, like a real SetValue would when updating.
    /// Useful for determine which subkeys would change on an SetValue operation
    UpdateSet = 4,
}
