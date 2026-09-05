use super::*;

/// Per-mode lock guards held by a RecordLockGuard.
#[derive(Debug)]
pub(super) enum RecordLockGuards {
    Lifetime {
        _lifetime: AsyncRwLockWriteGuardArc<()>,
    },
    Watch {
        _lifetime: AsyncRwLockReadGuardArc<()>,
        _watch: AsyncMutexGuardArc<()>,
    },
    Transaction {
        _lifetime: AsyncRwLockReadGuardArc<()>,
        _transaction: AsyncRwLockWriteGuardArc<()>,
    },
}

/// Record lock guard for a single record
#[derive(Debug)]
#[must_use]
pub struct RecordLockGuard<R: RecordLockPurpose, S: RecordLockPurpose> {
    record_lock: Arc<RecordLock<R, S>>,
    _guards: RecordLockGuards,
    #[cfg(feature = "debug-locks")]
    id: usize,
    #[cfg(feature = "debug-locks")]
    active_guards: Arc<Mutex<HashMap<usize, backtrace::Backtrace>>>,
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> RecordLockGuard<R, S> {
    pub(super) fn new(record_lock: Arc<RecordLock<R, S>>, guards: RecordLockGuards) -> Self {
        #[cfg(feature = "debug-locks")]
        let (id, active_guards) = {
            let id = GUARD_ID.fetch_add(1, Ordering::AcqRel);
            let active_guards = record_lock.get_active_guards();
            active_guards.lock().insert(id, backtrace::Backtrace::new());
            (id, active_guards)
        };

        Self {
            record_lock,
            _guards: guards,
            #[cfg(feature = "debug-locks")]
            id,
            #[cfg(feature = "debug-locks")]
            active_guards,
        }
    }

    pub fn record(&self) -> OpaqueRecordKey {
        self.record_lock.record()
    }
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> Drop for RecordLockGuard<R, S> {
    fn drop(&mut self) {
        #[cfg(feature = "debug-locks")]
        self.active_guards.lock().remove(&self.id);

        self.record_lock.drop_record_lock_guard();
    }
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> fmt::Display for RecordLockGuard<R, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Record({})", self.record())
    }
}
