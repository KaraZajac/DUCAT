use super::*;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RecordStoreRecordLockPurpose {
    New,
    Delete,
    Watch,
    Set,
    Snapshot,
    TransactBegin,
    TransactEnd,
    TransactCommit,
    TransactRollback,
    TransactSet,
    TransactGet,
}

impl RecordLockPurpose for RecordStoreRecordLockPurpose {
    fn record_lock_mode(&self) -> RecordLockMode {
        RecordLockMode::Lifetime
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RecordStoreSubkeyLockPurpose {
    Get,
    Peek,
    Set,
}

impl RecordLockPurpose for RecordStoreSubkeyLockPurpose {
    fn record_lock_mode(&self) -> RecordLockMode {
        RecordLockMode::Lifetime
    }
}

pub type RecordStoreRecordLockTable =
    RecordLockTable<RecordStoreRecordLockPurpose, RecordStoreSubkeyLockPurpose>;
pub type RecordStoreRecordLockGuard =
    RecordLockGuard<RecordStoreRecordLockPurpose, RecordStoreSubkeyLockPurpose>;
pub type RecordStoreRecordsLockGuard =
    RecordsLockGuard<RecordStoreRecordLockPurpose, RecordStoreSubkeyLockPurpose>;
// pub type RecordStoreSubkeyLockGuard =
//     SubkeyLockGuard<RecordStoreRecordLockPurpose, RecordStoreSubkeyLockPurpose>;
