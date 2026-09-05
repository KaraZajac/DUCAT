use super::*;

/// The result of the do_get_value_operation
#[derive(Default, Clone, Debug)]
pub struct GetResult {
    /// The subkey value if we got one
    pub opt_value: Option<Arc<SignedValueData>>,
    /// The descriptor if wanted
    pub opt_descriptor: Option<Arc<SignedValueDescriptor>>,
}

/// The result of the do_inspect_value_operation
#[derive(Default, Clone, Debug)]
pub struct InspectResult {
    /// The actual in-schema subkey range being reported on
    subkeys: ValueSubkeyRangeSet,
    /// The sequence map
    seqs: Vec<ValueSeqNum>,
    /// The descriptor if wanted
    opt_descriptor: Option<Arc<SignedValueDescriptor>>,
}

impl InspectResult {
    pub fn new(
        registry_accessor: &impl VeilidComponentRegistryAccessor,
        requested_subkeys: ValueSubkeyRangeSet,
        log_context: &str,
        subkeys: ValueSubkeyRangeSet,
        seqs: Vec<ValueSeqNum>,
        opt_descriptor: Option<Arc<SignedValueDescriptor>>,
    ) -> VeilidAPIResult<Self> {
        #[allow(clippy::unnecessary_cast)]
        {
            if subkeys.len() as u64 != seqs.len() as u64 {
                veilid_log!(registry_accessor error "{}: mismatch between subkeys returned and sequence number list returned: {}!={}", log_context, subkeys.len(), seqs.len());
                apibail_internal!("list length mismatch");
            }
        }
        if !subkeys.is_subset(&requested_subkeys) {
            veilid_log!(registry_accessor error "{}: more subkeys returned than requested: {} not a subset of {}", log_context, subkeys, requested_subkeys);
            apibail_internal!("invalid subkeys returned");
        }
        Ok(Self {
            subkeys,
            seqs,
            opt_descriptor,
        })
    }

    pub fn strip_none_seqs(&self) -> Self {
        // Trim inspected subkey range to subkeys we have data for locally
        let mut trimmed_subkeys = ValueSubkeyRangeSet::new();
        let mut trimmed_seqs = vec![];
        for (skn, sk) in self.subkeys.iter().enumerate() {
            let seq = self.seqs[skn];
            if seq.is_some() {
                trimmed_seqs.push(seq);
                trimmed_subkeys.insert(sk);
            }
        }

        Self {
            subkeys: trimmed_subkeys,
            seqs: trimmed_seqs,
            opt_descriptor: self.opt_descriptor.clone(),
        }
    }

    pub fn subkeys(&self) -> &ValueSubkeyRangeSet {
        &self.subkeys
    }
    pub fn seqs(&self) -> &[ValueSeqNum] {
        &self.seqs
    }
    pub fn seqs_mut(&mut self) -> &mut [ValueSeqNum] {
        &mut self.seqs
    }
    pub fn opt_descriptor(&self) -> Option<Arc<SignedValueDescriptor>> {
        self.opt_descriptor.clone()
    }
}
