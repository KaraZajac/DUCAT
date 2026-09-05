use super::*;

impl_veilid_log_facility!("stor");

/// Number of uncommitted records to allow before forcing a flush to disk
/// This naturally constrains the subkey limit as well, since each uncommitted record
/// can have up to 1024 subkeys, but no more than 1 MB of changes per record, so a maximum of 32MB of subkey memory here
const UNCOMMITTED_RECORDS_LIMIT: usize = 32;

pub struct RecordIndex<D>
where
    D: RecordDetail,
{
    unlocked_inner: Arc<RecordStoreUnlockedInner>,

    /// The in-memory cache that keeps track of what records are in the tabledb
    record_cache: LruCache<RecordTableKey, Record<D>>,
    /// The in-memory cache of commonly accessed subkey data so we don't have to keep hitting the db
    subkey_cache: LruCache<SubkeyTableKey, RecordData>,
    /// Uncommitted changes to records that need to be flushed to the database
    uncommitted_record_changes: UncommittedRecordChanges<D>,
    /// Uncommitted changes to subkeys that need to be flushed to the database
    uncommitted_subkey_changes: UncommittedSubkeyChanges,
    /// Pending in a commit action uncommitted changes to records that need to be flushed to the database
    pending_uncommitted_record_changes: Option<Arc<UncommittedRecordChanges<D>>>,
    /// Pending in a commit action uncommitted changes to subkeys that need to be flushed to the database
    pending_uncommitted_subkey_changes: Option<Arc<UncommittedSubkeyChanges>>,
    /// Total storage space or subkey data inclusive of structures in memory
    subkey_cache_space: LimitedSize<usize>,
    /// Total storage space of records in the tabledb inclusive of subkey data and structures
    record_cache_space: LimitedSize<u64>,
}

pub struct RecordSizeEstimator<D>
where
    D: RecordDetail,
{
    record_table: TableDB,
    _marker: core::marker::PhantomData<D>,
}

impl<D> RecordSizeEstimator<D>
where
    D: RecordDetail,
{
    fn new(record_table: TableDB) -> Self {
        Self {
            record_table,
            _marker: Default::default(),
        }
    }

    pub fn estimate(&self, rtk: &RecordTableKey, record: &Record<D>) -> VeilidAPIResult<u64> {
        let record_size = self
            .record_table
            .estimate_storage_size_json(0, &rtk.bytes(), record)?;

        let subkey_size = record
            .subkey_sizes()
            .iter()
            .fold(0u64, |acc, x| acc.saturating_add(*x as u64));

        // Subkeys are basically random data after encryption, and then base64 encoded
        // and then compressed, but the compression is effectively useless on that
        let post_b64_subkey_size = subkey_size.saturating_mul(3) / 2;

        Ok(record_size + post_b64_subkey_size)
    }
}

impl<D> VeilidComponentRegistryAccessor for RecordIndex<D>
where
    D: RecordDetail,
{
    fn registry(&self) -> VeilidComponentRegistry {
        self.unlocked_inner.registry.clone()
    }
}

impl<D> fmt::Debug for RecordIndex<D>
where
    D: RecordDetail,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecordIndex")
            .field("record_cache", &self.record_cache)
            .field("subkey_cache", &self.subkey_cache)
            .field(
                "uncommitted_record_changes",
                &self.uncommitted_record_changes,
            )
            .field(
                "uncommitted_subkey_changes",
                &self.uncommitted_subkey_changes,
            )
            .field(
                "pending_uncommitted_record_changes",
                &self.pending_uncommitted_record_changes,
            )
            .field(
                "pending_uncommitted_subkey_changes",
                &self.pending_uncommitted_subkey_changes,
            )
            .field("subkey_cache_total_size", &self.subkey_cache_space)
            .field("total_storage_space", &self.record_cache_space)
            .finish()
    }
}

impl<D> RecordIndex<D>
where
    D: RecordDetail,
{
    pub async fn try_new(unlocked_inner: Arc<RecordStoreUnlockedInner>) -> EyreResult<Self> {
        let subkey_cache_size = unlocked_inner.limits.subkey_cache_size;
        let limit_subkey_cache_total_size = unlocked_inner
            .limits
            .max_subkey_cache_memory_mb
            .map(|mb| mb * 1_048_576usize);
        let limit_max_storage_space = unlocked_inner
            .limits
            .max_storage_space_mb
            .map(|mb| mb as u64 * 1_048_576u64);
        let limit_max_records = unlocked_inner.limits.max_records.unwrap_or(usize::MAX);

        let mut record_index = Self {
            record_cache: LruCache::new(limit_max_records),
            subkey_cache: LruCache::new(subkey_cache_size),
            uncommitted_record_changes: BTreeMap::new(),
            uncommitted_subkey_changes: BTreeMap::new(),
            pending_uncommitted_record_changes: None,
            pending_uncommitted_subkey_changes: None,
            subkey_cache_space: LimitedSize::try_new(
                unlocked_inner.registry.clone(),
                "subkey_cache_total_size",
                limit_subkey_cache_total_size,
                0,
            )?,
            record_cache_space: LimitedSize::try_new(
                unlocked_inner.registry.clone(),
                "total_storage_space",
                limit_max_storage_space,
                0,
            )?,
            unlocked_inner,
        };

        record_index.load_db().await?;

        Ok(record_index)
    }

    /// Create a new record
    pub fn create_record(
        &mut self,
        key: OpaqueRecordKey,
        record: Record<D>,
    ) -> VeilidAPIResult<()> {
        let record_size_estimator = self.record_size_estimator();

        let rtk = RecordTableKey {
            record_key: key.clone(),
        };

        // Ensure this record is actually new
        if !record.is_new() {
            apibail_internal!(
                "record was not new during create: key={}: {:?}",
                key,
                record
            );
        }

        // If record already exists, fail early
        if let Some(prev_record) = self.record_cache.get(&rtk) {
            veilid_log!(self error "RecordIndex({}): Record already existed with key {}: {:?}", self.unlocked_inner.name, key, prev_record.clone());
            apibail_internal!("record already exists");
        }

        // Make room here or reject create
        let record_size = record_size_estimator.estimate(&rtk, &record)?;
        self.make_room_for_record(record_size, None)?;

        // Add to record cache
        let mut opt_lru_out = None;
        if let Some(prev_record) =
            self.record_cache
                .insert_with_callback(rtk.clone(), record.clone(), |dead_k, dead_v| {
                    opt_lru_out = Some((dead_k, dead_v));
                })
        {
            // Should not happen, log it
            veilid_log!(self error "RecordIndex({}): Consistency failure, check for existing record failed {}: {:?}", self.unlocked_inner.name, key, prev_record);
            apibail_internal!("record existed despite previous check");
        }

        // Purge LRU out if it happene
        if let Some(lru_out) = opt_lru_out {
            // LRU out should not happen due to make_room_for_record
            veilid_log!(self error "RecordIndex({}): Consistency failure, not enough room made for new record {}: lru_out={}", self.unlocked_inner.name, key, lru_out.0);
            self.purge_record_and_subkeys(lru_out.0, lru_out.1, true)?;
        }

        // Update total space
        let mut space = self.record_cache_space.modify().unwrap_or_log();
        space.add(record_size)?;
        space.commit()?;

        // Add uncommited record create
        self.add_uncommitted_record_create(rtk, record, record_size);

        Ok(())
    }

    /// Delete a record
    pub fn delete_record(&mut self, key: OpaqueRecordKey) -> VeilidAPIResult<()> {
        let rtk = RecordTableKey {
            record_key: key.clone(),
        };

        let Some(record) = self.record_cache.remove(&rtk) else {
            apibail_key_not_found!(key.clone());
        };

        self.purge_record_and_subkeys(rtk, record, false)
    }

    /// Access a record
    ///
    /// If the record exists, passes it to a function and marks the record as recently used
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub(super) fn with_record<R, F>(
        &mut self,
        opaque_record_key: &OpaqueRecordKey,
        func: F,
    ) -> VeilidAPIResult<Option<R>>
    where
        F: FnOnce(&Record<D>) -> R,
    {
        let record_size_estimator = self.record_size_estimator();

        // Get record from index
        let rtk = RecordTableKey {
            record_key: opaque_record_key.clone(),
        };
        let Some(record) = self.record_cache.get(&rtk) else {
            return Ok(None);
        };

        let old_record = record.clone();
        let old_record_size = record_size_estimator.estimate(&rtk, &old_record)?;

        let mut new_record = record.clone();

        // LRU touch
        new_record.touch();

        // Callback
        let out = Some(func(&new_record));

        // Make room for any record changes
        let new_record_size = record_size_estimator.estimate(&rtk, &new_record)?;
        self.make_room_for_record(new_record_size, Some(old_record_size))?;

        // Store record and adjust total storage
        let mut space = self.record_cache_space.modify().unwrap_or_log();
        space.sub(old_record_size)?;
        space.add(new_record_size)?;
        space.commit()?;

        // No failures after commit point
        self.record_cache.insert(rtk.clone(), new_record.clone());
        self.add_uncommitted_record_update(
            rtk,
            new_record,
            new_record_size,
            old_record,
            old_record_size,
        );

        Ok(out)
    }

    /// See if a record exists
    pub fn contains_record(&self, opaque_record_key: &OpaqueRecordKey) -> bool {
        let rtk = RecordTableKey {
            record_key: opaque_record_key.clone(),
        };
        self.record_cache.contains_key(&rtk)
    }

    /// Access a record
    ///
    /// If the record exists, passes it to a function but does not marks the record as recently used
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub(super) fn peek_record<R, F>(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        func: F,
    ) -> Option<R>
    where
        F: FnOnce(&Record<D>) -> R,
    {
        // Get record from index
        let mut out = None;
        let rtk = RecordTableKey {
            record_key: opaque_record_key.clone(),
        };
        if !self.record_cache.contains_key(&rtk) {
            return None;
        }
        if let Some(record) = self.record_cache.peek(&rtk) {
            // Callback
            out = Some(func(record));
        }

        out
    }

    /// Access the least recently used record
    ///
    /// If the record exists, passes it to a function but does not marks the record as recently used
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub(super) fn peek_lru<R, F>(&self, func: F) -> Option<R>
    where
        F: FnOnce(&OpaqueRecordKey, &Record<D>) -> R,
    {
        // Get the least recently used record
        let (k, v) = self.record_cache.peek_lru()?;

        Some(func(&k.record_key, v))
    }

    /// Modify a record's detail
    ///
    /// If the record exists, passes a mutable reference of its detail to a function and marks the record as recently used
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub(super) fn with_record_detail_mut<R, F>(
        &mut self,
        opaque_record_key: &OpaqueRecordKey,
        func: F,
    ) -> VeilidAPIResult<Option<R>>
    where
        F: FnOnce(Arc<SignedValueDescriptor>, &mut D) -> R,
    {
        let record_size_estimator = self.record_size_estimator();

        // Get record from index
        let rtk = RecordTableKey {
            record_key: opaque_record_key.clone(),
        };
        let Some(record) = self.record_cache.get(&rtk).cloned() else {
            return Ok(None);
        };

        let old_record = record.clone();
        let old_record_size = record_size_estimator.estimate(&rtk, &old_record)?;

        let mut new_record = record.clone();

        // LRU touch
        new_record.touch();

        // Callback
        let out = Some(func(new_record.descriptor(), new_record.detail_mut()));

        // Make room for any record changes
        let new_record_size = record_size_estimator.estimate(&rtk, &new_record)?;
        self.make_room_for_record(new_record_size, Some(old_record_size))?;

        // Store record and adjust total storage
        let mut space = self.record_cache_space.modify()?;
        space.sub(old_record_size)?;
        space.add(new_record_size)?;
        space.commit()?;

        // No failures after commit point
        self.record_cache.insert(rtk.clone(), new_record.clone());
        self.add_uncommitted_record_update(
            rtk,
            new_record,
            new_record_size,
            old_record,
            old_record_size,
        );

        Ok(out)
    }

    /// Get a subkey value
    ///
    /// Does not perform database operations if the subkey does not exist in the cache.
    pub fn prepare_load_action(
        &mut self,
        key: OpaqueRecordKey,
        subkey: ValueSubkey,
        peek: bool,
    ) -> VeilidAPIResult<LoadActionResult> {
        let rtk = RecordTableKey {
            record_key: key.clone(),
        };

        let Some(record) = self.record_cache.get(&rtk) else {
            return Ok(LoadActionResult::NoRecord);
        };

        // Check if the subkey is out of range
        if subkey > record.max_subkey() {
            apibail_invalid_argument!("subkey out of range", "subkey", subkey);
        }

        if !record.stored_subkeys().contains(subkey) {
            return Ok(LoadActionResult::NoSubkey {
                descriptor: record.descriptor(),
            });
        }

        let stk = SubkeyTableKey {
            record_key: key.clone(),
            subkey,
        };

        // Look through any uncommitted changes to see if we have the data already before it hits the DB
        let opt_cached_record_data = self.subkey_cache.get(&stk).cloned().or_else(|| {
            self.uncommitted_subkey_changes
                .get(&stk)
                .or_else(|| {
                    self.pending_uncommitted_subkey_changes
                        .as_ref()
                        .and_then(|x| x.get(&stk))
                })
                .and_then(|v| match v {
                    UncommittedSubkeyChange::Create { new_data } => Some(new_data.clone()),
                    UncommittedSubkeyChange::Update {
                        new_data,
                        opt_old_data: _,
                    } => Some(new_data.clone()),
                    UncommittedSubkeyChange::Delete {
                        opt_old_data: _,
                        is_lru: _,
                    } => None,
                })
        });

        Ok(LoadActionResult::Subkey {
            descriptor: record.descriptor(),
            load_action: LoadAction::new(
                self.unlocked_inner.subkey_table.clone(),
                stk,
                opt_cached_record_data,
                peek,
            ),
        })
    }

    /// Finalize a load action
    ///
    /// If the load action pulled a value from the database, it stores a subkey in
    /// the cache only if it isn't already there
    pub fn finish_load_action(&mut self, load_action: LoadAction) {
        if load_action.is_peek() {
            return;
        }

        let (stk, opt_cached_record_data) = load_action.into_cached_record_data();

        if let Some(cached_record_data) = opt_cached_record_data {
            // This automatically checks if the data is in the cache before inserting it
            self.cache_subkey(stk, cached_record_data);
        }
    }

    /// Update a record subkey
    /// If the update fails, records may still be purged from the cache if there was not room
    pub fn set_single_subkey(
        &mut self,
        key: &OpaqueRecordKey,
        subkey: ValueSubkey,
        value: Arc<SignedValueData>,
    ) -> VeilidAPIResult<()> {
        let record_size_estimator = self.record_size_estimator();

        // Get the record we're updating
        let rtk = RecordTableKey {
            record_key: key.clone(),
        };

        // Make a RecordData for the value
        let new_data = self.make_record_data(value)?;

        // Get the current record from the cache
        let Some(old_record) = self.record_cache.get(&rtk).cloned() else {
            apibail_key_not_found!(key.clone());
        };

        // Make a copy of the record to edit
        let mut new_record = old_record.clone();

        // Change the record to reflect the new data
        let is_new_subkey = new_record.record_stored_subkey(
            subkey,
            &new_data,
            self.unlocked_inner.limits.max_record_data_size,
        )?;

        // Update the record's touch timestamp for LRU sorting
        new_record.touch();

        // Make room here or reject update
        let old_record_size = record_size_estimator.estimate(&rtk, &old_record)?;
        let new_record_size = record_size_estimator.estimate(&rtk, &new_record)?;
        self.make_room_for_record(new_record_size, Some(old_record_size))?;

        // Adjust total storage
        // Should not fail because we made room
        let mut space = self.record_cache_space.modify()?;
        space = self.sub_from_record_cache_space(space, old_record_size)?;
        space = self.add_to_record_cache_space(space, new_record_size)?;
        space.verify_commit()?;

        // Put the new record back
        match self.record_cache.insert(rtk.clone(), new_record.clone()) {
            Some(previous) => {
                // Must return the old record because of record_cache.get() succeeding above
                if previous != old_record {
                    apibail_internal!(
                        "Inconsistent record cache for key {}: {:?} != {:?}",
                        rtk,
                        previous,
                        old_record
                    );
                }
            }
            None => {
                // Must return the old record because of record_cache.get() succeeding above
                apibail_internal!(
                    "Missing record cache for key {}: None != {:?}",
                    rtk,
                    old_record
                );
            }
        }

        // Cache the new subkey data
        let stk = SubkeyTableKey {
            record_key: key.clone(),
            subkey,
        };
        let opt_old_data = self.cache_subkey(stk.clone(), new_data.clone());

        // Commit the space change now
        if let Err(e) = space.commit() {
            // Should not fail because we verified the commit earlier
            veilid_log!(self error "Failed to commit record space: {}", e);
            self.uncache_subkey(&stk);
            return Err(e.into());
        }

        // No failures beyond this point
        if let Some(old_data) = opt_old_data {
            // If it was in the cache, it must have been on disk too, so just update
            self.add_uncommitted_subkey_update(stk, new_data, Some(old_data));
        } else {
            // If it wasn't in the cache, then it may exist on disk if it was stored
            if is_new_subkey {
                self.add_uncommitted_subkey_create(stk, new_data);
            } else {
                self.add_uncommitted_subkey_update(stk, new_data, None);
            }
        }

        // Queue the record db update
        self.add_uncommitted_record_update(
            rtk,
            new_record,
            new_record_size,
            old_record,
            old_record_size,
        );

        Ok(())
    }

    /// Update multiple subkeys on a single record
    /// If any updates fail, records may still be purged from the cache if there was not room
    pub fn set_subkeys_single_record(
        &mut self,
        key: &OpaqueRecordKey,
        subkey_values: &SubkeyValueList,
    ) -> VeilidAPIResult<()> {
        let record_size_estimator = self.record_size_estimator();

        // Get the record we're updating
        let rtk = RecordTableKey {
            record_key: key.clone(),
        };

        // Get the current record from the cache
        let Some(old_record) = self.record_cache.get(&rtk).cloned() else {
            apibail_key_not_found!(key.clone());
        };

        // Make a copy of the record to edit
        let mut new_record = old_record.clone();

        let mut new_data_list = Vec::with_capacity(subkey_values.len());
        for (subkey, value) in subkey_values.iter().cloned() {
            // Change the record to reflect the new data
            let new_data = self.make_record_data(value)?;
            let is_new_subkey = new_record.record_stored_subkey(
                subkey,
                &new_data,
                self.unlocked_inner.limits.max_record_data_size,
            )?;
            let stk = SubkeyTableKey {
                record_key: key.clone(),
                subkey,
            };
            new_data_list.push((stk, new_data, is_new_subkey));
        }

        // Update the record's touch timestamp for LRU sorting
        new_record.touch();

        // Make room here or reject update
        let old_record_size = record_size_estimator.estimate(&rtk, &old_record)?;
        let new_record_size = record_size_estimator.estimate(&rtk, &new_record)?;
        self.make_room_for_record(new_record_size, Some(old_record_size))?;

        // Adjust total storage
        // Should not fail because we made room
        let mut space = self.record_cache_space.modify()?;
        space = self.sub_from_record_cache_space(space, old_record_size)?;
        space = self.add_to_record_cache_space(space, new_record_size)?;
        space.verify_commit()?;

        // Put the new record back
        match self.record_cache.insert(rtk.clone(), new_record.clone()) {
            Some(previous) => {
                // Must return the old record because of record_cache.get() succeeding above
                if previous != old_record {
                    apibail_internal!(
                        "Inconsistent record cache for key {}: {:?} != {:?}",
                        rtk,
                        previous,
                        old_record
                    );
                }
            }
            None => {
                // Must return the old record because of record_cache.get() succeeding above
                apibail_internal!(
                    "Missing record cache for key {}: None != {:?}",
                    rtk,
                    old_record
                );
            }
        }

        let mut modified_subkeys = vec![];
        let mut opt_old_data_list = vec![];

        // Cache all the new subkey data
        for (stk, new_data, _is_new_subkey) in &new_data_list {
            let opt_old_data = self.cache_subkey(stk.clone(), new_data.clone());
            modified_subkeys.push(stk.clone());
            opt_old_data_list.push(opt_old_data);
        }

        // Commit the space change now
        if let Err(e) = space.commit() {
            // Should not fail because we verified the commit earlier
            veilid_log!(self error "Failed to commit record space: {}", e);
            for (stk, _new_data, _is_new_subkey) in &new_data_list {
                self.uncache_subkey(stk);
            }
            return Err(e.into());
        }

        // No failures beyond this point
        for ((stk, new_data, is_new_subkey), opt_old_data) in
            new_data_list.into_iter().zip(opt_old_data_list)
        {
            if let Some(old_data) = opt_old_data {
                // If it was in the cache, it must have been on disk too, so just update
                self.add_uncommitted_subkey_update(stk, new_data, Some(old_data));
            } else {
                // If it wasn't in the cache, then it may exist on disk if it was stored
                if is_new_subkey {
                    self.add_uncommitted_subkey_update(stk, new_data, None);
                } else {
                    self.add_uncommitted_subkey_create(stk, new_data);
                }
            }
        }

        // Queue the record db update
        self.add_uncommitted_record_update(
            rtk,
            new_record,
            new_record_size,
            old_record,
            old_record_size,
        );

        Ok(())
    }

    // Update multiple subkeys on a multiple record
    /// If any updates fail, records may still be purged from the cache if there was not room
    pub fn set_subkeys_multiple_records(
        &mut self,
        keys_and_subkeys: &RecordSubkeyValueList,
    ) -> VeilidAPIResult<()> {
        let record_size_estimator = self.record_size_estimator();

        struct RecordUpdate<D: RecordDetail> {
            rtk: RecordTableKey,
            old_record: Record<D>,
            old_record_size: u64,
            new_record: Record<D>,
            new_record_size: u64,
            new_data_list: Vec<(SubkeyTableKey, RecordData, bool)>,
            opt_old_data_list: Vec<Option<RecordData>>,
        }
        let mut record_updates = Vec::with_capacity(keys_and_subkeys.len());

        for (key, subkey_values) in keys_and_subkeys {
            // Get the record we're updating
            let rtk = RecordTableKey {
                record_key: key.clone(),
            };

            // Get the current record from the cache
            let Some(old_record) = self.record_cache.get(&rtk).cloned() else {
                apibail_key_not_found!(key.clone());
            };

            // Make a copy of the record to edit
            let mut new_record = old_record.clone();

            let mut new_data_list = Vec::with_capacity(subkey_values.len());
            for (subkey, value) in subkey_values.iter().cloned() {
                // Change the record to reflect the new data
                let new_data = self.make_record_data(value)?;
                let is_new_subkey = new_record.record_stored_subkey(
                    subkey,
                    &new_data,
                    self.unlocked_inner.limits.max_record_data_size,
                )?;

                // Keep the new data for later
                let stk = SubkeyTableKey {
                    record_key: key.clone(),
                    subkey,
                };
                new_data_list.push((stk, new_data, is_new_subkey));
            }

            // Update the record's touch timestamp for LRU sorting
            new_record.touch();

            // Make room here or reject update
            let old_record_size = record_size_estimator.estimate(&rtk, &old_record)?;
            let new_record_size = record_size_estimator.estimate(&rtk, &new_record)?;
            self.make_room_for_record(new_record_size, Some(old_record_size))?;

            record_updates.push(RecordUpdate {
                rtk,
                old_record,
                old_record_size,
                new_record,
                new_record_size,
                new_data_list,
                opt_old_data_list: vec![],
            });
        }

        // Adjust total storage
        // Should not fail because we made room
        let mut space = self.record_cache_space.modify()?;
        for record_update in &record_updates {
            space = self.sub_from_record_cache_space(space, record_update.old_record_size)?;
            space = self.add_to_record_cache_space(space, record_update.new_record_size)?;
        }
        space.verify_commit()?;

        // Put the record back
        let mut modified_subkeys = vec![];
        for RecordUpdate {
            rtk,
            old_record,
            old_record_size: _,
            new_record,
            new_record_size: _,
            new_data_list,
            opt_old_data_list,
        } in &mut record_updates
        {
            // Put the new record back
            match self.record_cache.insert(rtk.clone(), new_record.clone()) {
                Some(previous) => {
                    // Must return the old record because of record_cache.get() succeeding above
                    if &previous != old_record {
                        apibail_internal!(
                            "Inconsistent record cache for key {}: {:?} != {:?}",
                            rtk,
                            previous,
                            old_record
                        );
                    }
                }
                None => {
                    // Must return the old record because of record_cache.get() succeeding above
                    apibail_internal!(
                        "Missing record cache for key {}: None != {:?}",
                        rtk,
                        old_record
                    );
                }
            }

            // Cache all the new subkey data
            for (stk, new_data, _is_new_subkey) in new_data_list.iter() {
                let opt_old_data = self.cache_subkey(stk.clone(), new_data.clone());
                modified_subkeys.push(stk.clone());
                opt_old_data_list.push(opt_old_data);
            }
        }

        // Commit the space change now
        if let Err(e) = space.commit() {
            // Should not fail because we verified the commit earlier
            veilid_log!(self error "Failed to commit record space: {}", e);
            for stk in modified_subkeys {
                self.uncache_subkey(&stk);
            }
            return Err(e.into());
        }

        // No failures beyond this point
        for RecordUpdate {
            rtk,
            old_record,
            old_record_size,
            new_record,
            new_record_size,
            new_data_list,
            opt_old_data_list,
        } in record_updates
        {
            for ((stk, new_data, is_new_subkey), opt_old_data) in
                new_data_list.into_iter().zip(opt_old_data_list)
            {
                if let Some(old_data) = opt_old_data {
                    // If it was in the cache, it must have been on disk too, so just update
                    self.add_uncommitted_subkey_update(stk, new_data, Some(old_data));
                } else {
                    // If it wasn't in the cache, then it may exist on disk if it was stored
                    if is_new_subkey {
                        self.add_uncommitted_subkey_update(stk, new_data, None);
                    } else {
                        self.add_uncommitted_subkey_create(stk, new_data);
                    }
                }
            }

            // Queue the record db update
            self.add_uncommitted_record_update(
                rtk,
                new_record,
                new_record_size,
                old_record,
                old_record_size,
            );
        }

        Ok(())
    }

    /// Write out a transaction to be committed if there are more than
    /// the desired number of actions in the uncommitted queue
    pub fn maybe_prepare_commit_action(&mut self) -> Option<CommitAction<D>> {
        if self.uncommitted_record_changes.len() >= UNCOMMITTED_RECORDS_LIMIT {
            self.prepare_commit_action()
        } else {
            None
        }
    }

    /// Write out a transaction to be committed
    pub fn prepare_commit_action(&mut self) -> Option<CommitAction<D>> {
        // Ensure we don't have multiple commit actions pending
        if self.pending_uncommitted_record_changes.is_some()
            || self.pending_uncommitted_subkey_changes.is_some()
        {
            return None;
        }

        // If there's no work to be done, then return None
        if self.uncommitted_record_changes.is_empty() && self.uncommitted_subkey_changes.is_empty()
        {
            return None;
        }

        // Make a transaction and save off the actions to be performed into the CommitAction
        let rt_xact = self.unlocked_inner.record_table.transact();
        let st_xact = self.unlocked_inner.subkey_table.transact();
        let uncommitted_record_changes =
            Arc::new(std::mem::take(&mut self.uncommitted_record_changes));
        let uncommitted_subkey_changes =
            Arc::new(std::mem::take(&mut self.uncommitted_subkey_changes));

        // Record commit action as pending
        self.pending_uncommitted_record_changes = Some(uncommitted_record_changes.clone());
        self.pending_uncommitted_subkey_changes = Some(uncommitted_subkey_changes.clone());

        let commit_action = CommitAction::new(
            rt_xact,
            st_xact,
            uncommitted_record_changes,
            uncommitted_subkey_changes,
        );

        veilid_log!(self debug target: "stor::record_index", "prepare_commit_action: {:#?}", commit_action);

        Some(commit_action)
    }

    /// Roll back any part of the commit action that did not complete
    /// in the event of a db commit failure
    pub fn finish_commit_action(&mut self, commit_action: CommitAction<D>) {
        veilid_log!(self debug target: "stor::record_index", "finish_commit_action: {:#?}", commit_action);

        // XXX: if we ever support 'rollback points', this will become a list of CommitAction<D>
        // XXX: and we'll have to assign IDs and pass around CommitActionId to avoid having too much cloning
        if self.pending_uncommitted_record_changes.is_none()
            && self.pending_uncommitted_subkey_changes.is_none()
        {
            veilid_log!(self error "commit action was not pending");
        }

        // Record commit as having finished
        self.pending_uncommitted_record_changes = None;
        self.pending_uncommitted_subkey_changes = None;

        // See if any rollbacks need to be done
        if let Some((uncommitted_record_changes, uncommitted_subkey_changes)) =
            commit_action.into_rollback_changes()
        {
            // First roll back any unprepared, uncommitted actions that have take place since this commit
            // action was prepared since they will all depend on this action succeeding
            let unprepared_uncommitted_record_changes =
                std::mem::take(&mut self.uncommitted_record_changes);
            let unprepared_uncommitted_subkey_changes =
                std::mem::take(&mut self.uncommitted_subkey_changes);
            self.rollback_record_changes(unprepared_uncommitted_record_changes);
            self.rollback_subkey_changes(unprepared_uncommitted_subkey_changes);

            // Then roll back the prepared, uncommited actions that must be part of this commit action as well
            let uncommitted_record_changes =
                Arc::into_inner(uncommitted_record_changes).unwrap_or_log();
            let uncommitted_subkey_changes =
                Arc::into_inner(uncommitted_subkey_changes).unwrap_or_log();
            self.rollback_record_changes(uncommitted_record_changes);
            self.rollback_subkey_changes(uncommitted_subkey_changes);
        }
    }

    /// Returns the total amount of used space
    pub fn total_storage_space(&self) -> u64 {
        self.record_cache_space.with_value(|v| v).unwrap_or_log()
    }

    //////////////////////////////////////////////////////////////////////////////////////////

    fn record_size_estimator(&self) -> RecordSizeEstimator<D> {
        RecordSizeEstimator::<D>::new(self.unlocked_inner.record_table.clone())
    }

    fn make_record_data(&self, value: Arc<SignedValueData>) -> VeilidAPIResult<RecordData> {
        if value.data_size() > self.unlocked_inner.limits.max_subkey_size {
            apibail_internal!(
                "record data too large for record index {}: {} > {}",
                self.unlocked_inner.name,
                value.data_size(),
                self.unlocked_inner.limits.max_subkey_size,
            );
        }
        Ok(RecordData::new(value))
    }

    async fn load_db(&mut self) -> EyreResult<()> {
        let record_size_estimator = self.record_size_estimator();

        let start_ts = Timestamp::now();
        veilid_log!(self info "Loading record index: {}", self.unlocked_inner.name);

        // Pull record index from table into a vector to ensure we sort them
        // If they don't load, delete 'em.
        let record_table_keys = self.unlocked_inner.record_table.get_keys(0).await?;

        // Start transactions for repairs and drops
        let rt_xact = self.unlocked_inner.record_table.transact();

        let record_table_keys_count = record_table_keys.len();
        let mut record_index_sorted: Vec<(RecordTableKey, Record<D>)> =
            Vec::with_capacity(record_table_keys_count);

        let mut last_check_ts = Timestamp::now();
        let mut repaired_count = AtomicUsize::new(0);
        for (n, k) in record_table_keys.into_iter().enumerate() {
            let Ok(rtk) = RecordTableKey::try_from(k.as_slice()) else {
                rt_xact.delete(0, &k).await?;
                continue;
            };
            let Ok(record) = self
                .load_record_from_db(&rtk, &rt_xact, &mut repaired_count)
                .await
            else {
                rt_xact.delete(0, &k).await?;
                continue;
            };

            let rtk = RecordTableKey::try_from(k.as_ref())?;
            record_index_sorted.push((rtk, record));

            let check_ts = Timestamp::now();
            if check_ts.duration_since(last_check_ts) > TimestampDuration::new_secs(1) {
                last_check_ts = check_ts;
                veilid_log!(self info "  Records loaded: ({} / {}) {} repaired {}", n, record_table_keys_count, repaired_count.load(Ordering::Acquire), check_ts.duration_since(start_ts));
            }
        }

        rt_xact.commit().await?;

        // Make new transactions for the subkey cleanup and space purge
        let rt_xact = self.unlocked_inner.record_table.transact();
        let st_xact = self.unlocked_inner.subkey_table.transact();

        // Sort the record index by reverse last touched time (newest first)
        record_index_sorted.sort_unstable_by_key(|b| std::cmp::Reverse(b.1.last_touched()));

        // Truncate the record list to the max record count
        let record_index_limit = self.record_cache.capacity();

        if record_index_sorted.len() > record_index_limit {
            // Drop excess records
            for (rtk, record) in record_index_sorted[record_index_limit..].iter() {
                self.delete_record_from_db_transaction(rtk, record, &rt_xact, &st_xact)
                    .await?;
            }
            record_index_sorted.truncate(record_index_limit);
        }

        // Figure out which records might overflow any of our limits
        let mut dead_records = HashSet::new();
        for (n, (rtk, record)) in record_index_sorted.iter().enumerate() {
            // If we can't get the record size, drop it
            let record_size = match record_size_estimator.estimate(rtk, record) {
                Ok(v) => v,
                Err(e) => {
                    veilid_log!(self error "RecordIndex({}): Failed to estimate record storage size while loading db ({}): {} {:?}", self.unlocked_inner.name, rtk, e, record);
                    self.delete_record_from_db_transaction(rtk, record, &rt_xact, &st_xact)
                        .await?;
                    dead_records.insert(rtk.clone());
                    continue;
                }
            };

            // Total the storage space
            let hit_limit = {
                let mut space = self.record_cache_space.modify()?;
                space = self.add_to_record_cache_space(space, record_size)?;
                let hit_limit = !space.check_limit();
                if hit_limit {
                    // Revert from the storage total
                    space = self.sub_from_record_cache_space(space, record_size)?;
                }
                space.commit().unwrap_or_log();

                hit_limit
            };

            // See if we need to drop records to fit
            if hit_limit {
                // Drop excess records
                let excess_records = &record_index_sorted[n..];

                veilid_log!(self info "  Purging {} excess records", excess_records.len());

                for (rtk, record) in excess_records.iter() {
                    self.delete_record_from_db_transaction(rtk, record, &rt_xact, &st_xact)
                        .await?;
                }
                record_index_sorted.truncate(n);
                break;
            }
        }

        // Commit purges
        rt_xact.commit().await?;
        st_xact.commit().await?;

        // Now insert records in reverse order from oldest to newest to preserve LRU
        for (rtk, record) in record_index_sorted.into_iter().rev() {
            // Skip any that we marked as dead
            if dead_records.contains(&rtk) {
                continue;
            }
            self.record_cache.insert(rtk, record);
        }

        let end_ts = Timestamp::now();
        veilid_log!(self info "Finished loading {} in {:#}", self.unlocked_inner.name, end_ts.duration_since(start_ts));

        Ok(())
    }

    fn rollback_record_changes(&mut self, uncommitted_record_changes: UncommittedRecordChanges<D>) {
        veilid_log!(self debug target: "stor::record_index", "rollback_record_changes: {:#?}", uncommitted_record_changes);

        // Rollback creates first so we don't have to worry about LRU
        for (rtk, urc) in uncommitted_record_changes.iter().rev() {
            match urc {
                UncommittedRecordChange::Create {
                    new_record,
                    new_record_size,
                } => {
                    let mut space = self.record_cache_space.modify().unwrap_or_log();
                    space = match self.sub_from_record_cache_space(space, *new_record_size) {
                        Ok(v) => v,
                        Err(e) => {
                            veilid_log!(self error "UncommittedRecordChange::Create rollback: {} cache space underflow: {}", rtk, e);
                            continue;
                        }
                    };
                    if let Err(e) = space.commit() {
                        veilid_log!(self error "UncommittedRecordChange::Create rollback: {} cache space commit failure: {}", rtk, e);
                        continue;
                    }

                    let opt_created_record = self.record_cache.remove(rtk);

                    // Validate
                    if let Some(created_record) = opt_created_record {
                        if &created_record != new_record {
                            veilid_log!(self error "UncommittedRecordChange::Create rollback: {} had unexpected created record", rtk);
                        }
                    } else {
                        veilid_log!(self error "UncommittedRecordChange::Create rollback: {} had missing created record", rtk);
                    }
                }
                UncommittedRecordChange::Update {
                    new_record: _,
                    new_record_size: _,
                    old_record: _,
                    old_record_size: _,
                }
                | UncommittedRecordChange::Delete {
                    old_record: _,
                    old_record_size: _,
                    is_lru: _,
                } => {
                    // Skip for now
                }
            }
        }

        // Rollback updates and deletions second
        for (rtk, urc) in uncommitted_record_changes.into_iter().rev() {
            match urc {
                UncommittedRecordChange::Update {
                    new_record,
                    new_record_size,
                    old_record,
                    old_record_size,
                } => {
                    let mut space = self.record_cache_space.modify().unwrap_or_log();
                    space = match self.sub_from_record_cache_space(space, new_record_size) {
                        Ok(v) => v,
                        Err(e) => {
                            veilid_log!(self error "UncommittedRecordChange::Update rollback: {} cache space underflow: {}", rtk, e);
                            continue;
                        }
                    };
                    space = match self.add_to_record_cache_space(space, old_record_size) {
                        Ok(v) => v,
                        Err(e) => {
                            veilid_log!(self error "UncommittedRecordChange::Update rollback: {} cache space underflow: {}", rtk, e);
                            continue;
                        }
                    };
                    if let Err(e) = space.commit() {
                        veilid_log!(self error "UncommittedRecordChange::Update rollback: {} cache space commit failure: {}", rtk, e);
                        continue;
                    }

                    let opt_updated_record = self.record_cache.insert(rtk.clone(), old_record);

                    // Validate
                    if opt_updated_record != Some(new_record) {
                        veilid_log!(self error "UncommittedRecordChange::Update rollback: {} had unexpected updated value", &rtk);
                    }
                }
                UncommittedRecordChange::Delete {
                    old_record,
                    old_record_size,
                    is_lru,
                } => {
                    let mut space = self.record_cache_space.modify().unwrap_or_log();
                    space = match self.add_to_record_cache_space(space, old_record_size) {
                        Ok(v) => v,
                        Err(e) => {
                            veilid_log!(self error "UncommittedRecordChange::Delete rollback: {} cache space underflow: {}", rtk, e);
                            continue;
                        }
                    };
                    if let Err(e) = space.commit() {
                        veilid_log!(self error "UncommittedRecordChange::Delete rollback: {} cache space commit failure: {}", rtk, e);
                        continue;
                    }

                    let opt_deleted_record = self.record_cache.insert(rtk.clone(), old_record);

                    // Validate
                    if opt_deleted_record.is_some() {
                        veilid_log!(self error "UncommittedRecordChange::Delete rollback: {} had unexpected deleted value", &rtk);
                    }
                    if is_lru {
                        match self.record_cache.entry(rtk) {
                            hashlink::lru_cache::Entry::Occupied(mut occupied_entry) => {
                                // Move to LRU position
                                occupied_entry.to_front();
                            }
                            hashlink::lru_cache::Entry::Vacant(vacant_entry) => {
                                // Validate
                                veilid_log!(self error "UncommittedRecordChange::Delete rollback: {} was not present directly after insertion", vacant_entry.into_key());
                            }
                        }
                    }
                }
                UncommittedRecordChange::Create {
                    new_record: _,
                    new_record_size: _,
                } => {
                    // Already did these
                }
            }
        }
    }

    fn rollback_subkey_changes(&mut self, uncommitted_subkey_changes: UncommittedSubkeyChanges) {
        veilid_log!(self debug target: "stor::record_index", "rollback_subkey_changes: {:#?}", uncommitted_subkey_changes);
        // Process all uncaches first so we don't have to worry about LRU
        for (stk, usc) in uncommitted_subkey_changes.iter().rev() {
            match usc {
                UncommittedSubkeyChange::Create { new_data } => {
                    let opt_prev_data = self.uncache_subkey(stk);

                    // Validate
                    if let Some(prev_data) = opt_prev_data {
                        if &prev_data != new_data {
                            veilid_log!(self error "UncommittedSubkeyChange::Create rollback: {} had mismatched previous data", stk);
                        }
                    } else {
                        // If rollback of create had no cached value, it may be due to LRU
                        // For now we don't reload the subkey cache because it is only in-memory and will be reloaded upon the next subkey get
                    }
                }
                UncommittedSubkeyChange::Update {
                    new_data,
                    opt_old_data,
                } => {
                    // Update of an uncached subkey could have no old data, so just uncache it
                    if opt_old_data.is_none() {
                        let opt_prev_data = self.uncache_subkey(stk);

                        // Validate
                        if let Some(prev_data) = opt_prev_data {
                            if &prev_data != new_data {
                                veilid_log!(self error "UncommittedSubkeyChange::Update (without data) rollback: {} had mismatched previous data", stk);
                            }
                        } else {
                            // If rollback of no-data update had no cached value, it may be due to LRU
                            // For now we don't reload the subkey cache because it is only in-memory and will be reloaded upon the next subkey get
                        }
                    }
                }
                UncommittedSubkeyChange::Delete {
                    opt_old_data: _,
                    is_lru: _,
                } => {
                    // Skip until second pass
                }
            }
        }
        for (stk, usc) in uncommitted_subkey_changes.into_iter().rev() {
            match usc {
                UncommittedSubkeyChange::Update {
                    new_data,
                    opt_old_data,
                } => {
                    // Update of an uncached subkey could have old data so re-cache it
                    if let Some(old_data) = opt_old_data {
                        let opt_prev_data = self.cache_subkey(stk.clone(), old_data);

                        // Validate
                        if let Some(prev_data) = opt_prev_data {
                            if prev_data != new_data {
                                veilid_log!(self error "UncommittedSubkeyChange::Update (with data) rollback: {} had mismatched previous value upon removal", &stk);
                            }
                        } else {
                            // If rollback of with-data update had no cached value, it may be due to LRU
                            // For now we don't reload the subkey cache because it is only in-memory and will be reloaded upon the next subkey get
                        }
                    }
                }
                UncommittedSubkeyChange::Delete {
                    opt_old_data,
                    is_lru,
                } => {
                    // Delete of an uncached subkey could have no old data
                    if let Some(old_data) = opt_old_data {
                        // Put the data back in the cache
                        let opt_prev_data = self.cache_subkey(stk.clone(), old_data);

                        // Validate
                        if opt_prev_data.is_some() {
                            veilid_log!(self error "UncommittedSubkeyChange::Delete rollback: {} had unexpected previous value", &stk);
                        }
                        if is_lru {
                            match self.subkey_cache.entry(stk) {
                                hashlink::lru_cache::Entry::Occupied(mut occupied_entry) => {
                                    // Move to LRU position
                                    occupied_entry.to_front();
                                }
                                hashlink::lru_cache::Entry::Vacant(vacant_entry) => {
                                    // Validate
                                    veilid_log!(self error "UncommittedRecordChange::Delete rollback: {} was not present directly after insertion", vacant_entry.into_key());
                                }
                            }
                        }
                    }
                }
                UncommittedSubkeyChange::Create { new_data: _ } => {
                    // Already did these
                }
            }
        }
    }

    /// Loads a record directly from the database, bypassing caches
    /// Automatically cleans up the record if it is desynchronized
    async fn load_record_from_db(
        &self,
        rtk: &RecordTableKey,
        rt_xact: &TableDBTransaction,
        repaired_count: &mut AtomicUsize,
    ) -> VeilidAPIResult<Record<D>> {
        let Some(mut record) = self
            .unlocked_inner
            .record_table
            .load_json::<Record<D>>(0, &rtk.bytes())
            .await?
        else {
            apibail_internal!("missing record: {}", rtk);
        };

        record.post_deserialize();

        if record.needs_repair() {
            self.repair_record(rtk, &mut record, rt_xact).await?;
            repaired_count.fetch_add(1, Ordering::AcqRel);
        }

        Ok(record)
    }

    /// Deletes a record directly from the database via a transaction
    /// Requires that it is not in the record index and has no subkeys cached
    async fn delete_record_from_db_transaction(
        &self,
        rtk: &RecordTableKey,
        record: &Record<D>,
        rt_xact: &TableDBTransaction,
        st_xact: &TableDBTransaction,
    ) -> VeilidAPIResult<()> {
        if self.record_cache.contains_key(rtk) {
            apibail_internal!("should have removed record from cache already: {}", rtk);
        }

        let stored_subkeys = record.stored_subkeys();
        for sk in stored_subkeys.iter() {
            let stk = SubkeyTableKey {
                record_key: rtk.record_key.clone(),
                subkey: sk,
            };
            if self.subkey_cache.contains_key(&stk) {
                apibail_internal!("should have removed subkey from cache already: {}", stk);
            }

            st_xact.delete(0, &stk.bytes()).await?;
        }
        rt_xact.delete(0, &rtk.bytes()).await?;

        Ok(())
    }

    /// Synchronizes record with in-database copies of all subkey data
    async fn repair_record(
        &self,
        rtk: &RecordTableKey,
        record: &mut Record<D>,
        rt_xact: &TableDBTransaction,
    ) -> VeilidAPIResult<()> {
        let mut subkey_info = vec![];
        let mut stk = SubkeyTableKey {
            record_key: rtk.record_key.clone(),
            subkey: 0,
        };
        for subkey in 0..record.subkey_count() {
            stk.subkey = subkey as ValueSubkey;
            if let Ok(Some(recorddata)) = self.load_subkey_from_db(&stk).await {
                subkey_info.push((
                    recorddata.signed_value_data().value_data().seq(),
                    recorddata.data_size() as u16,
                ));
            } else {
                subkey_info.push((ValueSeqNum::NONE, 0u16));
            }
        }
        record.repair(subkey_info);

        rt_xact
            .store_json::<Record<D>>(0, &rtk.bytes(), record)
            .await?;
        Ok(())
    }

    /// Loads a subkey from the database directly, bypassing the cache
    /// Performs no verifications
    async fn load_subkey_from_db(
        &self,
        stk: &SubkeyTableKey,
    ) -> VeilidAPIResult<Option<RecordData>> {
        self.unlocked_inner
            .subkey_table
            .load_json::<RecordData>(0, &stk.bytes())
            .await
    }

    /// Adds subkey data to the cache, performing all of the accounting around the operation
    /// Evicts enough other subkeys from the cache to make room and meet limits
    /// Return the data that was previously in the cache
    fn cache_subkey(&mut self, stk: SubkeyTableKey, data: RecordData) -> Option<RecordData> {
        veilid_log!(self debug target: "stor::record_index", "cache_subkey: {} <-- {} bytes", stk, data.data_size());

        // If the data being set is exactly the same, shortcut this operation
        if let Some(existing_data) = self.subkey_cache.get(&stk) {
            if existing_data == &data {
                return Some(data);
            }
        }

        // If we modify the cache we should clear it on errors because
        // we want to ensure that it is consistent at all costs
        let res = (|| {
            // Calculate the space requirements for the new record data
            let subkey_memsize = stk.get_size() + data.get_size();
            let mut space = self.subkey_cache_space.modify().unwrap_or_log();
            space = self.add_to_subkey_cache_space(space, subkey_memsize)?;

            let mut opt_lru_out = None;
            let opt_prev_data =
                self.subkey_cache
                    .insert_with_callback(stk.clone(), data, |lruk, lruv| {
                        opt_lru_out = Some((lruk, lruv));
                    });

            if let Some(lru_out) = opt_lru_out {
                let lru_memsize = lru_out.0.get_size() + lru_out.1.get_size();
                space = self.sub_from_subkey_cache_space(space, lru_memsize)?;
            }

            if let Some(prev_data) = &opt_prev_data {
                let prev_memsize = stk.get_size() + prev_data.get_size();
                space = self.sub_from_subkey_cache_space(space, prev_memsize)?;
            }

            while !space.check_limit() {
                let Some((dead_stk, dead_data)) = self.subkey_cache.remove_lru() else {
                    apibail_internal!(
                        "purging subkey cache due to inability to make room: space={:?}",
                        space
                    );
                };
                let lru_memsize = dead_stk.get_size() + dead_data.get_size();
                space = self.sub_from_subkey_cache_space(space, lru_memsize)?;
            }

            // Must succeed due to prior check_limit
            space.commit().unwrap_or_log();

            Ok(opt_prev_data)
        })();

        match res {
            Err(e) => {
                veilid_log!(self error "purging subkey cache due to cache error: {}", e);
                let mut space = self.subkey_cache_space.modify().unwrap_or_log();
                space.set(0);
                self.subkey_cache.clear();

                None
            }
            Ok(v) => v,
        }
    }

    /// Removes subkey data from the cache, performing all of the accounting around the operation
    /// Return the data that was previously in the cache
    fn uncache_subkey(&mut self, stk: &SubkeyTableKey) -> Option<RecordData> {
        veilid_log!(self debug target: "stor::record_index", "uncache_subkey: {}", stk);

        // If we modify the cache we should clear it on errors because
        // we want to ensure that it is consistent at all costs
        let res: VeilidAPIResult<Option<RecordData>> = (|| {
            let opt_data = self.subkey_cache.remove(stk);
            if let Some(data) = &opt_data {
                let mut space = self.subkey_cache_space.modify().unwrap_or_log();
                let subkey_memsize = stk.get_size() + data.get_size();
                space = self.sub_from_subkey_cache_space(space, subkey_memsize)?;
                space.commit()?;
            }
            Ok(opt_data)
        })();

        match res {
            Err(e) => {
                veilid_log!(self error "purging subkey cache due to uncache error: {}", e);
                let mut space = self.subkey_cache_space.modify().unwrap_or_log();
                space.set(0);
                self.subkey_cache.clear();
                None
            }
            Ok(v) => v,
        }
    }

    fn add_to_record_cache_space(
        &mut self,
        mut space: LimitedSizeGuard<u64>,
        value: u64,
    ) -> VeilidAPIResult<LimitedSizeGuard<u64>> {
        if let Err(e) = space.add(value) {
            space.rollback();
            veilid_log!(self error "RecordIndex({}): Record space overflow: {}",self.unlocked_inner.name, e);
            return Err(e.into());
        }
        Ok(space)
    }

    fn sub_from_record_cache_space(
        &mut self,
        mut space: LimitedSizeGuard<u64>,
        value: u64,
    ) -> VeilidAPIResult<LimitedSizeGuard<u64>> {
        if let Err(e) = space.sub(value) {
            self.record_cache.clear();
            space.set(0);
            space.commit().unwrap_or_log();
            veilid_log!(self error "RecordIndex({}): Record space underflow: {}",self.unlocked_inner.name, e);
            return Err(e.into());
        }
        Ok(space)
    }

    fn add_to_subkey_cache_space(
        &mut self,
        mut space: LimitedSizeGuard<usize>,
        value: usize,
    ) -> VeilidAPIResult<LimitedSizeGuard<usize>> {
        if let Err(e) = space.add(value) {
            space.rollback();
            veilid_log!(self error "RecordIndex({}): Subkey space overflow:{}", self.unlocked_inner.name, e);
            return Err(e.into());
        }
        Ok(space)
    }

    fn sub_from_subkey_cache_space(
        &mut self,
        mut space: LimitedSizeGuard<usize>,
        value: usize,
    ) -> VeilidAPIResult<LimitedSizeGuard<usize>> {
        if let Err(e) = space.sub(value) {
            self.subkey_cache.clear();
            space.set(0);
            space.commit().unwrap_or_log();
            veilid_log!(self error "RecordIndex({}): Subkey space underflow: {}",self.unlocked_inner.name, e);
            return Err(e.into());
        }
        Ok(space)
    }

    fn make_room_for_record(
        &mut self,
        record_size: u64,
        opt_old_record_size: Option<u64>,
    ) -> VeilidAPIResult<()> {
        let record_size_estimator = self.record_size_estimator();

        // Get starting size and limit
        let Some(storage_size_limit) = self.record_cache_space.limit() else {
            // No limit, just go for it
            return Ok(());
        };
        let mut current_record_cache_size = self.record_cache_space.with_value(|x| x)?;

        // Add the delta we need to make room for
        if let Some(old_record_size) = opt_old_record_size {
            if let Some(new_record_cache_size) =
                current_record_cache_size.checked_sub(old_record_size)
            {
                current_record_cache_size = new_record_cache_size;
            } else {
                let mut space = self.record_cache_space.modify().unwrap_or_log();
                space.set(0);
                space.commit().unwrap_or_log();

                self.record_cache.clear();

                apibail_internal!(
                    "Cache size underflow making room for record: current={} removed={}",
                    current_record_cache_size,
                    old_record_size
                );
            }
        }

        if let Some(new_record_cache_size) = current_record_cache_size.checked_add(record_size) {
            current_record_cache_size = new_record_cache_size;
        } else {
            apibail_internal!(
                "Cache size overflow making room for record: current={} removed={}",
                current_record_cache_size,
                record_size
            );
        }

        // Figure out how many records from the LRU need to go to fit the delta
        let mut dead_count = 0usize;
        let mut lru_iter = self.record_cache.iter();

        while current_record_cache_size > storage_size_limit {
            let Some((dead_k, dead_v)) = lru_iter.next() else {
                apibail_generic!("can not make enough room in record store");
            };

            let lru_record_size = record_size_estimator.estimate(dead_k, dead_v)?;

            if let Some(new_record_cache_size) =
                current_record_cache_size.checked_sub(lru_record_size)
            {
                current_record_cache_size = new_record_cache_size;
            } else {
                let mut space = self.record_cache_space.modify().unwrap_or_log();
                space.set(0);
                space.commit().unwrap_or_log();

                self.record_cache.clear();

                apibail_internal!(
                    "Cache size underflow making room for record in LRU: current={} removed={}",
                    current_record_cache_size,
                    lru_record_size
                );
            }

            dead_count += 1;
        }

        // Purge the required number of records
        for _n in 0..dead_count {
            let (dead_k, dead_v) = self.record_cache.remove_lru().unwrap_or_log();
            self.purge_record_and_subkeys(dead_k, dead_v, true)?;
        }

        Ok(())
    }

    fn purge_record_and_subkeys(
        &mut self,
        rtk: RecordTableKey,
        record: Record<D>,
        is_lru: bool,
    ) -> VeilidAPIResult<()> {
        veilid_log!(self debug target: "stor::record_index", "purge_record_and_subkeys: {}, stored_subkey={}, is_lru={}", rtk, record.stored_subkeys(), is_lru);

        let record_size_estimator = self.record_size_estimator();

        if self.record_cache.contains_key(&rtk) {
            apibail_internal!(
                "RecordIndex({}): Should already have removed record from cache: {}",
                self.unlocked_inner.name,
                &rtk
            );
        }

        // Remove record everywhere else now that it's gone from the cache
        let record_size = record_size_estimator.estimate(&rtk, &record)?;
        let mut space = self.record_cache_space.modify().unwrap_or_log();
        space = self.sub_from_record_cache_space(space, record_size)?;
        space.commit()?;

        let stored_subkeys = record.stored_subkeys();
        for sk in stored_subkeys.iter() {
            let stk = SubkeyTableKey {
                record_key: rtk.record_key.clone(),
                subkey: sk,
            };

            let opt_data = self.uncache_subkey(&stk);
            self.add_uncommitted_subkey_delete(stk, opt_data, is_lru);
        }

        self.add_uncommitted_record_delete(rtk, record, record_size, is_lru);

        Ok(())
    }

    fn add_uncommitted_record_delete(
        &mut self,
        rtk: RecordTableKey,
        old_record: Record<D>,
        old_record_size: u64,
        is_lru: bool,
    ) {
        veilid_log!(self debug target: "stor::record_index", "add_uncommitted_record_delete: {}, {} bytes, is_lru={}", rtk, old_record_size, is_lru);

        let rtk_log = rtk.clone();

        match self.uncommitted_record_changes.entry(rtk) {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(UncommittedRecordChange::Delete {
                    old_record,
                    old_record_size,
                    is_lru,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut o) => {
                let urc = o.get_mut();
                match urc {
                    UncommittedRecordChange::Create {
                        new_record: _,
                        new_record_size: _,
                    } => {
                        // If we created a record and then deleted it, there's nothing to commit here
                        o.remove();
                    }
                    UncommittedRecordChange::Update {
                        new_record: _,
                        new_record_size: _,
                        old_record,
                        old_record_size,
                    } => {
                        // If we updated a record and then deleted it, rolling it back should be to the original value
                        *urc = UncommittedRecordChange::Delete {
                            old_record: old_record.clone(),
                            old_record_size: *old_record_size,
                            is_lru,
                        };
                    }
                    UncommittedRecordChange::Delete {
                        old_record: _,
                        old_record_size: _,
                        is_lru: _,
                    } => {
                        // Should never happen. Can't delete a record twice.
                        veilid_log!(self error "record was deleted twice in uncommitted log: {}", rtk_log);
                    }
                }
            }
        }
    }

    fn add_uncommitted_record_update(
        &mut self,
        rtk: RecordTableKey,
        new_record: Record<D>,
        new_record_size: u64,
        old_record: Record<D>,
        old_record_size: u64,
    ) {
        veilid_log!(self debug target: "stor::record_index", "add_uncommitted_record_update: {}, {} -> {} bytes", rtk, old_record_size, new_record_size);

        let rtk_log = rtk.clone();

        match self.uncommitted_record_changes.entry(rtk) {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(UncommittedRecordChange::Update {
                    new_record,
                    new_record_size,
                    old_record,
                    old_record_size,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut o) => {
                let urc = o.get_mut();
                match urc {
                    UncommittedRecordChange::Create {
                        new_record: _,
                        new_record_size: _,
                    } => {
                        // If we created a record and then updated it, might as well have just created it with the new value
                        *urc = UncommittedRecordChange::Create {
                            new_record,
                            new_record_size,
                        };
                    }
                    UncommittedRecordChange::Update {
                        new_record: _,
                        new_record_size: _,
                        old_record,
                        old_record_size,
                    } => {
                        // If we updated a record and then updated it, rolling it back should be to the original value
                        *urc = UncommittedRecordChange::Update {
                            new_record,
                            new_record_size,
                            old_record: old_record.clone(),
                            old_record_size: *old_record_size,
                        };
                    }
                    UncommittedRecordChange::Delete {
                        old_record: _,
                        old_record_size: _,
                        is_lru: _,
                    } => {
                        // Should never happen. Can't update a deleted record.
                        veilid_log!(self error "record was updated after being deleted in uncommitted log: {}", rtk_log);
                    }
                }
            }
        }
    }

    fn add_uncommitted_record_create(
        &mut self,
        rtk: RecordTableKey,
        new_record: Record<D>,
        new_record_size: u64,
    ) {
        veilid_log!(self debug target: "stor::record_index", "add_uncommitted_record_create: {}, {} bytes", rtk, new_record_size);

        let rtk_log = rtk.clone();

        match self.uncommitted_record_changes.entry(rtk) {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(UncommittedRecordChange::Create {
                    new_record,
                    new_record_size,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut o) => {
                let urc = o.get_mut();
                match urc {
                    UncommittedRecordChange::Create {
                        new_record: _,
                        new_record_size: _,
                    } => {
                        // Should never happen. Can't create an already created record.
                        veilid_log!(self error "record was created twice in uncommitted log: {}", rtk_log);
                    }
                    UncommittedRecordChange::Update {
                        new_record: _,
                        new_record_size: _,
                        old_record: _,
                        old_record_size: _,
                    } => {
                        // Should never happen. Can't create an already created record.
                        veilid_log!(self error "record was created after updated in uncommitted log: {}", rtk_log);
                    }
                    UncommittedRecordChange::Delete {
                        old_record,
                        old_record_size,
                        is_lru: _,
                    } => {
                        // A delete followed by a create is really an update
                        *urc = UncommittedRecordChange::Update {
                            new_record,
                            new_record_size,
                            old_record: old_record.clone(),
                            old_record_size: *old_record_size,
                        };
                    }
                }
            }
        }
    }

    fn add_uncommitted_subkey_delete(
        &mut self,
        stk: SubkeyTableKey,
        opt_old_data: Option<RecordData>,
        is_lru: bool,
    ) {
        veilid_log!(self debug target: "stor::record_index", "add_uncommitted_subkey_delete: {}, {}", stk, opt_old_data.as_ref().map(|x| format!("{} bytes", x.data_size())).unwrap_or_else(|| "no data".to_owned()));

        let stk_log = stk.clone();

        match self.uncommitted_subkey_changes.entry(stk) {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(UncommittedSubkeyChange::Delete {
                    opt_old_data,
                    is_lru,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut o) => {
                let usc = o.get_mut();
                match usc {
                    UncommittedSubkeyChange::Create { new_data: _ } => {
                        // Create followed by delete is nothing
                        o.remove();
                    }
                    UncommittedSubkeyChange::Update {
                        new_data: _,
                        opt_old_data: prior_opt_old_data,
                    } => {
                        // If we updated a subkey and then deleted it, rolling it back should be to the original value
                        *usc = UncommittedSubkeyChange::Delete {
                            opt_old_data: prior_opt_old_data.clone(),
                            is_lru,
                        };
                    }
                    UncommittedSubkeyChange::Delete {
                        opt_old_data: _,
                        is_lru: _,
                    } => {
                        // Should never happen. Can't delete a subkey twice.
                        veilid_log!(self error "subkey was deleted twice in uncommitted log: {}", stk_log);
                    }
                }
            }
        }
    }

    fn add_uncommitted_subkey_update(
        &mut self,
        stk: SubkeyTableKey,
        new_data: RecordData,
        opt_old_data: Option<RecordData>,
    ) {
        veilid_log!(self debug target: "stor::record_index", "add_uncommitted_subkey_update: {}, {} --> {} bytes", stk, opt_old_data.as_ref().map(|x| format!("{} bytes", x.data_size())).unwrap_or_else(|| "no data".to_owned()), new_data.data_size());

        let stk_log = stk.clone();

        match self.uncommitted_subkey_changes.entry(stk) {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(UncommittedSubkeyChange::Update {
                    new_data,
                    opt_old_data,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut o) => {
                let usc = o.get_mut();
                match usc {
                    UncommittedSubkeyChange::Create { new_data: _ } => {
                        // If we created a subkey and then updated it, might as well have just created it with the new value
                        *usc = UncommittedSubkeyChange::Create { new_data };
                    }
                    UncommittedSubkeyChange::Update {
                        new_data: _,
                        opt_old_data: prior_opt_old_data,
                    } => {
                        // If we updated a subkey and then updated it, rolling it back should be to the original value
                        *usc = UncommittedSubkeyChange::Update {
                            new_data,
                            opt_old_data: prior_opt_old_data.clone(),
                        };
                    }
                    UncommittedSubkeyChange::Delete {
                        opt_old_data: _,
                        is_lru: _,
                    } => {
                        // Should never happen. Can't update a deleted subkey.
                        veilid_log!(self error "subkey was updated after being deleted in uncommitted log: {}", stk_log);
                    }
                }
            }
        }
    }

    fn add_uncommitted_subkey_create(&mut self, stk: SubkeyTableKey, new_data: RecordData) {
        veilid_log!(self debug target: "stor::record_index", "add_uncommitted_subkey_create: {}, {} bytes", stk, new_data.data_size());
        let stk_log = stk.clone();

        match self.uncommitted_subkey_changes.entry(stk) {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(UncommittedSubkeyChange::Create { new_data });
            }
            std::collections::btree_map::Entry::Occupied(mut o) => {
                let usc = o.get_mut();
                match usc {
                    UncommittedSubkeyChange::Create { new_data: _ } => {
                        // Should never happen. Can't create an already created subkey.
                        veilid_log!(self error "subkey was created twice in uncommitted log: {}", stk_log);
                    }
                    UncommittedSubkeyChange::Update {
                        new_data: _,
                        opt_old_data: _,
                    } => {
                        // Should never happen. Can't create an already created subkey.
                        veilid_log!(self error "record was created after updated in uncommitted log: {}", stk_log);
                    }
                    UncommittedSubkeyChange::Delete {
                        opt_old_data: prior_opt_old_data,
                        is_lru: _,
                    } => {
                        // A delete followed by a create is really an update
                        *usc = UncommittedSubkeyChange::Update {
                            new_data,
                            opt_old_data: prior_opt_old_data.clone(),
                        };
                    }
                }
            }
        }
    }

    pub fn debug(&self) -> String {
        let mut out = String::new();

        out += "Records:\n";
        for (rik, rec) in &self.record_cache {
            out += &format!(
                "  {} age={:#} len={} subkeys={}\n",
                rik.record_key,
                Timestamp::now_non_decreasing().duration_since(rec.last_touched()),
                rec.record_data_size(),
                rec.stored_subkeys(),
            );
        }
        out += &format!("Subkey Cache Count: {}\n", self.subkey_cache.len());
        let subkey_cache_total_size = ByteCount::new(self.subkey_cache_space.debug_value() as u64);
        out += &format!(
            "Subkey Cache Total Size: {:#} ({} bytes)\n",
            subkey_cache_total_size, subkey_cache_total_size
        );
        let total_storage_space = ByteCount::new(self.record_cache_space.debug_value());
        out += &format!(
            "Total Storage Space: {:#} ({} bytes)\n",
            total_storage_space, total_storage_space
        );
        out += &format!(
            "Uncommitted Record Changes: {}\n",
            self.uncommitted_record_changes.len()
        );
        for k in self.uncommitted_record_changes.keys() {
            out += &format!("  {}\n", k);
        }
        out += &format!(
            "Uncommitted Subkey Changes: {}\n",
            self.uncommitted_subkey_changes.len()
        );
        for k in self.uncommitted_subkey_changes.keys() {
            out += &format!("  {}\n", k);
        }

        out
    }
}
