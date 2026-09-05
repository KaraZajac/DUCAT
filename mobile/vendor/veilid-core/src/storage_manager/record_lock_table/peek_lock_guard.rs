use super::*;

/// Peek lock guard: holds Peek-mode lock (lifetime.read only). Excludes Lifetime ops; concurrent with everything else.
#[derive(Debug)]
#[must_use]
pub struct PeekLockGuard<R: RecordLockPurpose, S: RecordLockPurpose> {
    record_lock: Arc<RecordLock<R, S>>,
    _lifetime_lock_guard: AsyncRwLockReadGuardArc<()>,
    #[cfg(feature = "debug-locks")]
    id: usize,
    #[cfg(feature = "debug-locks")]
    active_guards: Arc<Mutex<HashMap<usize, backtrace::Backtrace>>>,
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> PeekLockGuard<R, S> {
    pub(super) fn new(
        record_lock: Arc<RecordLock<R, S>>,
        lifetime_lock_guard: AsyncRwLockReadGuardArc<()>,
    ) -> Self {
        #[cfg(feature = "debug-locks")]
        let (id, active_guards) = {
            let id = GUARD_ID.fetch_add(1, Ordering::AcqRel);
            let active_guards = record_lock.get_active_guards();
            active_guards.lock().insert(id, backtrace::Backtrace::new());
            (id, active_guards)
        };

        Self {
            record_lock,
            _lifetime_lock_guard: lifetime_lock_guard,
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

impl<R: RecordLockPurpose, S: RecordLockPurpose> Drop for PeekLockGuard<R, S> {
    fn drop(&mut self) {
        #[cfg(feature = "debug-locks")]
        self.active_guards.lock().remove(&self.id);

        self.record_lock.drop_peek_lock_guard();
    }
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> fmt::Display for PeekLockGuard<R, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Peek({})", self.record())
    }
}
