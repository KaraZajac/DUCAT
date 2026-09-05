use super::*;

/// The state associated with a local record when it is opened
/// This is not serialized to storage as it is ephemeral for the lifetime of the opened record
#[derive(Clone, Debug)]
pub(in crate::storage_manager) struct OpenedRecord {
    /// The key pair used to perform writes to subkey on this opened record
    /// Without this, set_value() will fail regardless of which key or subkey is being written to
    /// as all writes are signed
    writer: Option<KeyPair>,

    /// The safety selection in current use
    safety_selection: SafetySelection,

    /// Encryption key, for newer records
    encryption_key: Option<BareSharedSecret>,
}

impl OpenedRecord {
    pub fn new(
        writer: Option<KeyPair>,
        safety_selection: SafetySelection,
        encryption_key: Option<BareSharedSecret>,
    ) -> Self {
        Self {
            writer,
            safety_selection,
            encryption_key,
        }
    }

    pub fn writer(&self) -> Option<&KeyPair> {
        self.writer.as_ref()
    }
    pub fn set_writer(&mut self, writer: Option<KeyPair>) {
        self.writer = writer;
    }

    pub fn safety_selection(&self) -> SafetySelection {
        self.safety_selection.clone()
    }
    pub fn set_safety_selection(&mut self, safety_selection: SafetySelection) {
        self.safety_selection = safety_selection;
    }

    pub fn encryption_key(&self) -> Option<BareSharedSecret> {
        self.encryption_key.clone()
    }
    pub fn set_encryption_key(&mut self, encryption_key: Option<BareSharedSecret>) {
        self.encryption_key = encryption_key;
    }
}
