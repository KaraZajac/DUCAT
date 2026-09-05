use super::*;

/// State per subkey
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::storage_manager) struct SubkeyConsensus {
    /// The value seen with the newest sequence number
    pub opt_value: Option<Arc<SignedValueData>>,
    /// The count out of the N closest nodes with the newest value.
    /// Strict consensus means if any node in the N closest are not the newest value, consensus fails
    /// If 0, then strict closest consensus was not achieved
    pub strict_consensus_count: usize,
    /// The count out of all of the nodes in the transaction with the same value.
    /// Loose consensus counts all nodes with the newest value, and does not depend on how close they are to the record key
    pub loose_consensus_count: usize,
}

impl SubkeyConsensus {
    pub fn new(opt_value: Option<Arc<SignedValueData>>) -> Self {
        Self {
            opt_value,
            strict_consensus_count: 1,
            loose_consensus_count: 1,
        }
    }

    /// Add value to consensus
    pub fn add_value(
        &mut self,
        opt_value: Option<Arc<SignedValueData>>,
        required_strict_consensus_count: usize,
    ) {
        let new_seq = opt_value
            .as_ref()
            .map(|v| v.value_data().seq())
            .unwrap_or_default();
        let old_seq = self
            .opt_value
            .as_ref()
            .map(|x| x.value_data().seq())
            .unwrap_or_default();

        if new_seq > old_seq {
            // Newer value found
            self.opt_value = opt_value;
            self.strict_consensus_count = 0;
            self.loose_consensus_count = 1;
        } else if new_seq == old_seq {
            if opt_value.as_ref().map(|x| x.value_data())
                != self.opt_value.as_ref().map(|x| x.value_data())
            {
                // Conflicting value found
                self.opt_value = opt_value;
                self.strict_consensus_count = 0;
                self.loose_consensus_count = 1;
            } else {
                // Equal value found
                if self.strict_consensus_count > 0 {
                    // Equal value found within strict consensus
                    self.strict_consensus_count += 1;
                }
                self.loose_consensus_count += 1;
            }
        } else if self.strict_consensus_count < required_strict_consensus_count {
            // Older value found within strict consensus
            self.strict_consensus_count = 0;
        }
    }
}

/// State of all subkeys
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::storage_manager) struct OutboundTransactionConsensus {
    subkey_consensus: BTreeMap<ValueSubkey, SubkeyConsensus>,
}

impl fmt::Display for OutboundTransactionConsensus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let subkey_consensus = self
            .subkey_consensus
            .iter()
            .map(|(k, v)| {
                format!(
                    "#{}={}",
                    k,
                    v.opt_value
                        .as_ref()
                        .map(|v| v.value_data().seq())
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        write!(f, "[{}]", subkey_consensus)
    }
}

impl OutboundTransactionConsensus {
    pub fn new() -> Self {
        Self {
            subkey_consensus: BTreeMap::new(),
        }
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.subkey_consensus.is_empty()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.subkey_consensus.len()
    }

    pub fn get(&self, subkey: ValueSubkey) -> Option<&SubkeyConsensus> {
        self.subkey_consensus.get(&subkey)
    }

    pub fn record(&mut self, subkey: ValueSubkey, state: Option<SubkeyConsensus>) {
        if let Some(state) = state {
            self.subkey_consensus.insert(subkey, state);
        } else {
            self.subkey_consensus.remove(&subkey);
        }
    }
}
