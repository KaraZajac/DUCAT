use super::*;

#[cfg(feature = "debug-locks")]
pub(super) static GUARD_ID: AtomicUsize = AtomicUsize::new(0);

/// Subkey lock management structure
#[derive(Debug)]
struct RecordLockInner<R: RecordLockPurpose, S: RecordLockPurpose> {
    subkey_lock_table: WeakValueHashMap<ValueSubkey, Weak<AsyncMutex<()>>>,
    purpose_state: RecordLockPurposeState<R, S>,
    peek_count: usize,
}

/// Record lock management structure
#[derive(Debug)]
pub(super) struct RecordLock<R: RecordLockPurpose, S: RecordLockPurpose> {
    registry: VeilidComponentRegistry,
    record: OpaqueRecordKey,
    /// Write = Lifetime mode. Read = any non-Lifetime mode.
    lifetime_lock: Arc<AsyncRwLock<()>>,
    /// Write = Transaction mode. Read = Subkey mode.
    transaction_lock: Arc<AsyncRwLock<()>>,
    /// Mutual exclusion among Watch ops on this record.
    watch_lock: Arc<AsyncMutex<()>>,
    inner: Mutex<RecordLockInner<R, S>>,
    #[cfg(feature = "debug-locks")]
    active_guards: Arc<Mutex<HashMap<usize, backtrace::Backtrace>>>,
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> VeilidComponentRegistryAccessor
    for RecordLock<R, S>
{
    fn registry(&self) -> VeilidComponentRegistry {
        self.registry.clone()
    }
}

impl<R: RecordLockPurpose, S: RecordLockPurpose> RecordLock<R, S> {
    pub fn new(registry: VeilidComponentRegistry, record: OpaqueRecordKey) -> Self {
        Self {
            registry,
            record,
            lifetime_lock: Arc::new(AsyncRwLock::new(())),
            transaction_lock: Arc::new(AsyncRwLock::new(())),
            watch_lock: Arc::new(AsyncMutex::new(())),
            inner: Mutex::new(RecordLockInner {
                subkey_lock_table: WeakValueHashMap::new(),
                purpose_state: RecordLockPurposeState {
                    whole_record_lock_purpose: None,
                    subkey_lock_purpose: BTreeMap::new(),
                },
                peek_count: 0,
            }),
            #[cfg(feature = "debug-locks")]
            active_guards: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn record(&self) -> OpaqueRecordKey {
        self.record.clone()
    }
    pub fn get_lifetime_lock(&self) -> Arc<AsyncRwLock<()>> {
        self.lifetime_lock.clone()
    }
    pub fn get_transaction_lock(&self) -> Arc<AsyncRwLock<()>> {
        self.transaction_lock.clone()
    }
    pub fn get_watch_lock(&self) -> Arc<AsyncMutex<()>> {
        self.watch_lock.clone()
    }
    pub fn purpose_state(&self) -> RecordLockPurposeState<R, S> {
        self.inner.lock().purpose_state.clone()
    }
    pub fn set_record_purpose(&self, purpose: R) {
        let mut inner = self.inner.lock();
        inner.purpose_state.whole_record_lock_purpose = Some(purpose);
        inner.purpose_state.subkey_lock_purpose.clear();
    }
    pub fn get_subkey_lock(&self, subkey: ValueSubkey) -> Arc<AsyncMutex<()>> {
        let mut inner = self.inner.lock();
        inner.subkey_lock_table.remove_expired();
        inner
            .subkey_lock_table
            .entry(subkey)
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
    }

    pub fn set_subkey_purpose(&self, subkey: ValueSubkey, purpose: S) {
        let mut inner = self.inner.lock();
        inner.purpose_state.whole_record_lock_purpose = None;
        inner
            .purpose_state
            .subkey_lock_purpose
            .insert(subkey, purpose);
    }

    #[expect(dead_code)]
    pub fn get_peek_count(&self) -> usize {
        self.inner.lock().peek_count
    }

    pub fn add_peek(&self) {
        self.inner.lock().peek_count += 1;
    }

    #[cfg(feature = "debug-locks")]
    pub(super) fn get_active_guards(&self) -> Arc<Mutex<HashMap<usize, backtrace::Backtrace>>> {
        self.active_guards.clone()
    }

    pub(super) fn drop_record_lock_guard(&self) {
        self.inner.lock().purpose_state.whole_record_lock_purpose = None;
    }

    pub(super) fn drop_subkey_lock_guard(&self, subkey: ValueSubkey) {
        self.inner
            .lock()
            .purpose_state
            .subkey_lock_purpose
            .remove(&subkey);
    }

    pub(super) fn drop_peek_lock_guard(&self) {
        self.inner.lock().peek_count -= 1;
    }
}
