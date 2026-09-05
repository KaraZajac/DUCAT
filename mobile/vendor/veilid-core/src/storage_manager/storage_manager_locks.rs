use super::*;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum StorageManagerRecordLockPurpose {
    Create,
    Open,
    Close,
    Delete,
    Watch,
    TransactBegin,
    TransactExtend,
    TransactEndAndCommit,
    TransactRollback,
    TransactDrop,
}

impl RecordLockPurpose for StorageManagerRecordLockPurpose {
    fn record_lock_mode(&self) -> RecordLockMode {
        match self {
            Self::Create | Self::Open | Self::Close | Self::Delete => RecordLockMode::Lifetime,
            Self::Watch => RecordLockMode::Watch,
            Self::TransactBegin
            | Self::TransactExtend
            | Self::TransactEndAndCommit
            | Self::TransactRollback
            | Self::TransactDrop => RecordLockMode::Transaction,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum StorageManagerSubkeyLockPurpose {
    Get,
    Set,
    TransactGet,
    TransactSet,
}

impl RecordLockPurpose for StorageManagerSubkeyLockPurpose {
    fn record_lock_mode(&self) -> RecordLockMode {
        RecordLockMode::Lifetime
    }
}

pub type StorageManagerRecordLockTable =
    RecordLockTable<StorageManagerRecordLockPurpose, StorageManagerSubkeyLockPurpose>;

pub type StorageManagerRecordLockGuard =
    RecordLockGuard<StorageManagerRecordLockPurpose, StorageManagerSubkeyLockPurpose>;
pub type StorageManagerRecordsLockGuard =
    RecordsLockGuard<StorageManagerRecordLockPurpose, StorageManagerSubkeyLockPurpose>;
pub type StorageManagerSubkeyLockGuard =
    SubkeyLockGuard<StorageManagerRecordLockPurpose, StorageManagerSubkeyLockPurpose>;
#[expect(dead_code)]
pub type StorageManagerPeekLockGuard =
    PeekLockGuard<StorageManagerRecordLockPurpose, StorageManagerSubkeyLockPurpose>;
pub type StorageManagerPeeksLockGuard =
    PeeksLockGuard<StorageManagerRecordLockPurpose, StorageManagerSubkeyLockPurpose>;
