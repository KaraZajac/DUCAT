use super::*;

/// Subkey lock guard: holds Subkey-mode locks (lifetime.read + transaction.read + per-subkey).
#[derive(Debug)]
#[must_use]
pub struct SubkeyLockGuard<R: RecordLockPurpose, S: RecordLockPurpose> {
    record_lock: Arc<RecordLock<R, S>>,
    _lifetime_lock_guard: AsyncRwLockReadGuardArc<()>,
    _transaction_lock_guard: AsyncRwLockReadGuardArc<()>,
    _subkey_lock_guard: AsyncMutexGuardArc<()>,
    subkey: ValueSubkey,
    #[cfg(feature = "debug-locks")]
    id: usize,
    #[cfg(feature = "debug-locks")]
    active_guards: Arc<Mutex<HashMap<usize, backtrace::Backtrace>>>,
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> SubkeyLockGuard<R, S> {
    pub(super) fn new(
        record_lock: Arc<RecordLock<R, S>>,
        lifetime_lock_guard: AsyncRwLockReadGuardArc<()>,
        transaction_lock_guard: AsyncRwLockReadGuardArc<()>,
        subkey_lock_guard: AsyncMutexGuardArc<()>,
        subkey: ValueSubkey,
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
            _transaction_lock_guard: transaction_lock_guard,
            _subkey_lock_guard: subkey_lock_guard,
            subkey,
            #[cfg(feature = "debug-locks")]
            id,
            #[cfg(feature = "debug-locks")]
            active_guards,
        }
    }
    pub fn record(&self) -> OpaqueRecordKey {
        self.record_lock.record()
    }

    pub fn subkey(&self) -> ValueSubkey {
        self.subkey
    }
}
impl<R: RecordLockPurpose, S: RecordLockPurpose> Drop for SubkeyLockGuard<R, S> {
    fn drop(&mut self) {
        #[cfg(feature = "debug-locks")]
        self.active_guards.lock().remove(&self.id);

        self.record_lock.drop_subkey_lock_guard(self.subkey);
    }
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> fmt::Display for SubkeyLockGuard<R, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Subkey({}:{})", self.record(), self.subkey())
    }
}
