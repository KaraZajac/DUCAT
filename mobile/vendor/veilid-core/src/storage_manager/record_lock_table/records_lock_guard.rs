use super::*;

/// Record lock guard for multiple records
#[derive(Debug)]
#[must_use]
pub struct RecordsLockGuard<R: RecordLockPurpose, S: RecordLockPurpose> {
    record_lock_guards: Vec<RecordLockGuard<R, S>>,
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> RecordsLockGuard<R, S> {
    pub(super) fn new(record_lock_guards: Vec<RecordLockGuard<R, S>>) -> Self {
        Self { record_lock_guards }
    }

    pub fn records(&self) -> Vec<OpaqueRecordKey> {
        self.record_lock_guards.iter().map(|x| x.record()).collect()
    }
    pub fn record_lock_guards(&self) -> impl Iterator<Item = &RecordLockGuard<R, S>> {
        self.record_lock_guards.iter()
    }
    // pub fn record_lock_guard(&self, record: &OpaqueRecordKey) -> Option<&RecordLockGuard<R, S>> {
    //     self.record_lock_guards
    //         .iter()
    //         .find(|x| &x.record() == record)
    // }
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> fmt::Display for RecordsLockGuard<R, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let records = self
            .record_lock_guards
            .iter()
            .map(|x| x.record().to_string())
            .collect::<Vec<_>>()
            .join(",");
        write!(f, "[{}]", records)
    }
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> From<RecordLockGuard<R, S>>
    for RecordsLockGuard<R, S>
{
    fn from(value: RecordLockGuard<R, S>) -> Self {
        Self {
            record_lock_guards: vec![value],
        }
    }
}
