use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, GetSize)]
pub struct Record<D>
where
    D: RecordDetail,
{
    descriptor: Arc<SignedValueDescriptor>,
    subkey_count: usize,
    stored_subkeys: ValueSubkeyRangeSet,
    #[serde(default)]
    subkey_seqs: Vec<u32>,
    #[serde(default)]
    subkey_sizes: Vec<u16>,
    last_touched_ts: Timestamp,
    #[serde(skip)]
    record_data_size: usize,
    #[serde(bound(deserialize = "D: RecordDetail"))]
    detail: D,
}

impl<D> Record<D>
where
    D: RecordDetail,
{
    pub fn new(
        cur_ts: Timestamp,
        descriptor: Arc<SignedValueDescriptor>,
        detail: D,
    ) -> VeilidAPIResult<Self> {
        let schema = descriptor.schema()?;
        let subkey_count = schema.subkey_count();
        Ok(Self {
            descriptor,
            subkey_count,
            stored_subkeys: ValueSubkeyRangeSet::new(),
            subkey_seqs: vec![u32::MAX; subkey_count],
            subkey_sizes: vec![0u16; subkey_count],
            last_touched_ts: cur_ts,
            record_data_size: 0,
            detail,
        })
    }

    pub fn post_deserialize(&mut self) {
        self.record_data_size = self
            .subkey_sizes
            .iter()
            .copied()
            .fold(0, |a, b| a + (b as usize))
    }

    pub fn is_new(&self) -> bool {
        self.stored_subkeys.is_empty() && self.record_data_size == 0 && self.detail.is_new()
    }

    pub fn descriptor(&self) -> Arc<SignedValueDescriptor> {
        self.descriptor.clone()
    }

    pub fn owner(&self) -> PublicKey {
        self.descriptor.owner()
    }

    pub fn subkey_count(&self) -> usize {
        self.subkey_count
    }

    pub fn max_subkey(&self) -> ValueSubkey {
        (self.subkey_count - 1) as ValueSubkey
    }

    pub fn stored_subkeys(&self) -> &ValueSubkeyRangeSet {
        &self.stored_subkeys
    }

    pub fn needs_repair(&self) -> bool {
        if self.subkey_seqs.len() != self.subkey_count
            || self.subkey_sizes.len() != self.subkey_count
        {
            return true;
        }
        false
    }

    pub fn repair(&mut self, subkey_info: Vec<(ValueSeqNum, u16)>) {
        self.subkey_seqs = vec![0; self.subkey_count];
        self.subkey_sizes = vec![0; self.subkey_count];
        self.record_data_size = 0;
        self.stored_subkeys.clear();
        for (n, (seq, size)) in subkey_info.iter().copied().enumerate() {
            self.subkey_seqs[n] = u32::from(seq);
            self.subkey_sizes[n] = size;
            self.record_data_size += size as usize;
            if seq.is_some() {
                self.stored_subkeys.insert(n as ValueSubkey);
            }
        }
    }

    /// Returns false if a previous subkey existed or true if this is a new subkey
    pub fn record_stored_subkey(
        &mut self,
        subkey: ValueSubkey,
        data: &RecordData,
        max_record_data_size: usize,
    ) -> VeilidAPIResult<bool> {
        if subkey > self.max_subkey() {
            apibail_invalid_argument!("subkey out of range", "subkey", subkey);
        }
        let seq = data.signed_value_data().value_data().seq();
        let new_subkey_size = data.data_size() as u16;
        let old_subkey_size = self.subkey_sizes[subkey as usize];

        let new_record_data_size = if new_subkey_size > old_subkey_size {
            self.record_data_size + (new_subkey_size - old_subkey_size) as usize
        } else if new_subkey_size < old_subkey_size {
            self.record_data_size - (old_subkey_size - new_subkey_size) as usize
        } else {
            self.record_data_size
        };

        if new_record_data_size > max_record_data_size {
            apibail_internal!(
                "record exceeds maximum data size: {} > {}",
                new_record_data_size,
                max_record_data_size
            );
        }

        // No failures past this point
        self.record_data_size = new_record_data_size;
        let is_new = self.stored_subkeys.insert(subkey);
        self.subkey_seqs.resize(self.subkey_count, 0);
        self.subkey_seqs[subkey as usize] = u32::from(seq);
        self.subkey_sizes.resize(self.subkey_count, 0);
        self.subkey_sizes[subkey as usize] = new_subkey_size;

        Ok(is_new)
    }

    pub fn subkey_sizes(&self) -> &[u16] {
        &self.subkey_sizes
    }

    pub fn subkey_seq(&self, subkey: ValueSubkey) -> VeilidAPIResult<ValueSeqNum> {
        if subkey > self.max_subkey() {
            apibail_invalid_argument!("subkey out of range", "subkey", subkey);
        }
        Ok(ValueSeqNum::from(self.subkey_seqs[subkey as usize]))
    }

    pub fn touch(&mut self) {
        self.last_touched_ts = Timestamp::now_non_decreasing();
    }

    pub fn last_touched(&self) -> Timestamp {
        self.last_touched_ts
    }

    pub fn record_data_size(&self) -> usize {
        self.record_data_size
    }

    pub fn schema(&self) -> DHTSchema {
        // unwrap is safe here because descriptor is immutable and set in new()
        self.descriptor.schema().unwrap_or_log()
    }

    pub fn detail(&self) -> &D {
        &self.detail
    }
    pub fn detail_mut(&mut self) -> &mut D {
        &mut self.detail
    }
}
