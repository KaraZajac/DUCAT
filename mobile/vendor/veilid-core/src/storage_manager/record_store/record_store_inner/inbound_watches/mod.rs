mod inbound_watch;
mod inbound_watch_id;
mod inbound_watch_id_allocator;
mod inbound_watch_list;

use super::*;

use inbound_watch::*;
pub use inbound_watch_id::*;
pub use inbound_watch_id_allocator::*;
pub use inbound_watch_list::*;

impl_veilid_log_facility!("stor");

#[derive(Debug, Default)]
pub struct InboundWatches {
    /// The set of records being watched for changes
    record_watches: HashMap<RecordTableKey, InboundWatchList>,

    /// The list of watched records that have changed values since last notification
    changed_records: HashSet<RecordTableKey>,

    /// The set of all allocated watch ids
    watch_id_allocator: InboundWatchIdAllocator,
}

impl InboundWatches {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn allocate(
        &mut self,
        opaque_record_key: &OpaqueRecordKey,
        params: InboundWatchParameters,
    ) -> VeilidAPIResult<InboundWatchId> {
        let rtk = RecordTableKey {
            record_key: opaque_record_key.clone(),
        };

        // Generate a record-unique watch id > 0
        let id = self.watch_id_allocator.allocate(rtk.clone())?;

        // Create a new watch
        let inbound_watch_list = self.record_watches.entry(rtk).or_default();
        inbound_watch_list.new_watch(id, params);

        Ok(id)
    }

    pub fn lookup_id(&mut self, raw_id: u64) -> VeilidAPIResult<Option<InboundWatchId>> {
        self.watch_id_allocator.lookup(raw_id)
    }

    pub fn check_id(&self, id: InboundWatchId, rtk: &RecordTableKey) -> bool {
        self.watch_id_allocator.get_key(id).as_ref() == Some(rtk)
    }

    pub fn get(&self, rtk: &RecordTableKey) -> Option<&InboundWatchList> {
        self.record_watches.get(rtk)
    }

    pub fn get_mut(&mut self, rtk: &RecordTableKey) -> Option<&mut InboundWatchList> {
        self.record_watches.get_mut(rtk)
    }

    pub fn insert_changed_record(&mut self, rtk: RecordTableKey) {
        self.changed_records.insert(rtk);
    }

    pub fn take_changed_records(&mut self) -> Vec<RecordTableKey> {
        let mut out = self.changed_records.drain().collect::<Vec<_>>();
        out.sort_unstable();
        out
    }

    pub fn try_remove_record(&mut self, rtk: &RecordTableKey) -> VeilidAPIResult<bool> {
        let Some(inbound_watch_list) = self.record_watches.remove(rtk) else {
            return Ok(false);
        };
        let dead_ids = inbound_watch_list
            .watches()
            .map(|x| x.id())
            .collect::<Vec<_>>();
        for dead_id in dead_ids {
            self.watch_id_allocator.free(dead_id)?;
        }
        self.changed_records.remove(rtk);

        Ok(true)
    }

    pub fn remove_watch(&mut self, id: InboundWatchId) -> VeilidAPIResult<()> {
        let Some(rtk) = self.watch_id_allocator.get_key(id) else {
            apibail_internal!("watch id does not exist");
        };
        let Some(inbound_watch_list) = self.record_watches.get_mut(&rtk) else {
            apibail_internal!("record does not exist for watch id");
        };
        let alive = inbound_watch_list.drop_watch(id, &mut self.watch_id_allocator)?;
        if !alive {
            self.record_watches.remove(&rtk);
            self.changed_records.remove(&rtk);
        }
        Ok(())
    }

    #[expect(dead_code)]
    pub fn try_remove_watch(&mut self, id: InboundWatchId) -> VeilidAPIResult<bool> {
        let Some(rtk) = self.watch_id_allocator.get_key(id) else {
            return Ok(false);
        };
        let Some(inbound_watch_list) = self.record_watches.get_mut(&rtk) else {
            apibail_internal!("record does not exist for watch id");
        };
        let alive = inbound_watch_list.drop_watch(id, &mut self.watch_id_allocator)?;
        if !alive {
            self.record_watches.remove(&rtk);
            self.changed_records.remove(&rtk);
        }
        Ok(true)
    }

    pub fn remove_expired_watches<D: Fn(InboundWatchId), L: Fn(VeilidAPIError)>(
        &mut self,
        now: Timestamp,
        debug_logger: &D,
        error_logger: &L,
    ) {
        self.record_watches.retain(|_, inbound_watch_list| {
            inbound_watch_list.drop_expired_watches(
                now,
                &mut self.watch_id_allocator,
                debug_logger,
                error_logger,
            )
        });
    }

    pub fn debug(&self) -> String {
        let mut out = format!(
            "Records with inbound watches: {}\n",
            self.record_watches.len()
        );
        let mut watches_keys = self.record_watches.keys().collect::<Vec<_>>();
        watches_keys.sort_unstable();
        for rtk in watches_keys {
            let inbound_watch_list = self.record_watches.get(rtk).unwrap_or_log();
            out += &format!("  {}: {:?}\n", rtk, inbound_watch_list);
        }

        out
    }
}
