use super::*;

/// Peek lock guard for multiple records
#[derive(Debug)]
#[must_use]
pub struct PeeksLockGuard<R: RecordLockPurpose, S: RecordLockPurpose> {
    peek_lock_guards: Vec<PeekLockGuard<R, S>>,
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> PeeksLockGuard<R, S> {
    pub(super) fn new(peek_lock_guards: Vec<PeekLockGuard<R, S>>) -> Self {
        Self { peek_lock_guards }
    }

    #[expect(dead_code)]
    pub fn records(&self) -> Vec<OpaqueRecordKey> {
        self.peek_lock_guards.iter().map(|x| x.record()).collect()
    }
    #[expect(dead_code)]
    pub fn peek_lock_guards(&self) -> impl Iterator<Item = &PeekLockGuard<R, S>> {
        self.peek_lock_guards.iter()
    }
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> fmt::Display for PeeksLockGuard<R, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let records = self
            .peek_lock_guards
            .iter()
            .map(|x| x.record().to_string())
            .collect::<Vec<_>>()
            .join(",");
        write!(f, "[{}]", records)
    }
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> From<PeekLockGuard<R, S>>
    for PeeksLockGuard<R, S>
{
    fn from(value: PeekLockGuard<R, S>) -> Self {
        Self {
            peek_lock_guards: vec![value],
        }
    }
}
