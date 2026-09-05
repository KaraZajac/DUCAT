use super::*;

impl_veilid_log_facility!("stor");

/// An individual transaction
#[derive(Debug, Clone)]
pub struct InboundTransaction {
    /// A unique id per record assigned at transaction begin time. Used to disambiguate a client's version of a transaction
    id: InboundTransactionId,
    /// When this transaction will expire
    expiration: Timestamp,
    /// The signing schema member key, or an anonymous key
    signing_member_id: MemberId,
    /// The descriptor for this record
    descriptor: Arc<SignedValueDescriptor>,
    /// Snapshot of record contents if record exists
    opt_snapshot: Option<Arc<RecordSnapshot>>,
    /// What has changed since snapshot
    changed_subkeys: BTreeMap<ValueSubkey, Arc<SignedValueData>>,
}

impl InboundTransaction {
    pub(super) fn new(
        id: InboundTransactionId,
        expiration: Timestamp,
        signing_member_id: MemberId,
        descriptor: Arc<SignedValueDescriptor>,
        opt_snapshot: Option<Arc<RecordSnapshot>>,
    ) -> Self {
        Self {
            id,
            expiration,
            signing_member_id,
            descriptor,
            opt_snapshot,
            changed_subkeys: Default::default(),
        }
    }

    pub fn id(&self) -> InboundTransactionId {
        self.id
    }
    #[expect(dead_code)]
    pub fn expiration(&self) -> Timestamp {
        self.expiration
    }
    pub fn update_expiration(&mut self, expiration: Timestamp) {
        self.expiration = expiration
    }
    pub fn signing_member_id(&self) -> &MemberId {
        &self.signing_member_id
    }
    pub fn descriptor(&self) -> Arc<SignedValueDescriptor> {
        self.descriptor.clone()
    }
    pub fn snapshot(&self) -> Option<Arc<RecordSnapshot>> {
        self.opt_snapshot.clone()
    }
    pub fn add_changed_subkey(&mut self, subkey: ValueSubkey, value: Arc<SignedValueData>) {
        self.changed_subkeys.insert(subkey, value);
    }
    pub fn remove_changed_subkey(&mut self, subkey: ValueSubkey) {
        self.changed_subkeys.remove(&subkey);
    }
    pub fn has_changed_subkeys(&self) -> bool {
        !self.changed_subkeys.is_empty()
    }
    pub fn changed_subkeys(
        &self,
    ) -> impl Iterator<Item = (ValueSubkey, Arc<SignedValueData>)> + use<'_> {
        self.changed_subkeys.iter().map(|(k, v)| (*k, v.clone()))
    }

    pub fn is_alive(&self, now: Timestamp) -> bool {
        self.expiration > now
    }
}

impl fmt::Display for InboundTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "id={} exp={} signer={}{}{}",
            self.id,
            self.expiration,
            self.signing_member_id,
            if let Some(snapshot) = &self.opt_snapshot {
                format!("snapshot seqs: {}\n", snapshot.seqs().to_table_string())
            } else {
                "".to_owned()
            },
            if self.changed_subkeys.is_empty() {
                "".to_owned()
            } else {
                format!(
                    "change_subkeys seqs: {}\n",
                    self.changed_subkeys
                        .iter()
                        .map(|(sk, svd)| format!("[{}]={}", sk, svd.value_data().seq()))
                        .collect::<Vec<String>>()
                        .join(",")
                )
            }
        )
    }
}
