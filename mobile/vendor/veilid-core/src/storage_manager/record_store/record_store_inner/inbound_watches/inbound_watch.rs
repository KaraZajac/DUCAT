use super::*;

/// An individual watch
#[derive(Debug, Clone)]
pub struct InboundWatch {
    /// The configuration of the watch
    params: InboundWatchParameters,
    /// A unique id per record assigned at watch creation time. Used to disambiguate a client's version of a watch
    id: InboundWatchId,
    /// What has changed in the watched range since the last update.
    /// May include non-watched ranges if they were changed as part of an overlapping transaction
    changed_subkeys: ValueSubkeyRangeSet,
}

impl InboundWatch {
    pub(super) fn new(id: InboundWatchId, params: InboundWatchParameters) -> Self {
        Self {
            id,
            params,
            changed_subkeys: Default::default(),
        }
    }

    pub fn id(&self) -> InboundWatchId {
        self.id
    }
    pub fn params(&self) -> &InboundWatchParameters {
        &self.params
    }

    pub fn update_params(&mut self, params: InboundWatchParameters) {
        self.params = params;
    }

    pub fn update_count(&mut self, count: u32) {
        self.params.count = count;
    }

    pub fn add_changed_subkey(&mut self, subkey: ValueSubkey) {
        self.changed_subkeys.insert(subkey);
    }
    #[expect(dead_code)]
    pub fn remove_changed_subkey(&mut self, subkey: ValueSubkey) {
        self.changed_subkeys.remove(subkey);
    }
    #[expect(dead_code)]
    pub fn has_changed_subkeys(&self) -> bool {
        !self.changed_subkeys.is_empty()
    }
    pub fn take_changed_subkeys(&mut self) -> ValueSubkeyRangeSet {
        let out = self.changed_subkeys.clone();
        self.changed_subkeys.clear();
        out
    }

    pub fn is_alive(&self, now: Timestamp) -> bool {
        self.params.count != 0 && self.params.expiration > now && !self.params.subkeys.is_empty()
    }
}

impl fmt::Display for InboundWatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "id={} exp={} cnt={} signer={} subkeys={} changed={} target={:?}",
            self.id,
            self.params.expiration,
            self.params.count,
            self.params.watcher_member_id,
            self.params.subkeys,
            self.changed_subkeys,
            self.params.target
        )
    }
}
