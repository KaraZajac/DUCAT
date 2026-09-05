use super::*;

#[derive(Default, Debug)]
pub struct InboundWatchIdAllocator {
    all_watch_ids: HashMap<InboundWatchId, RecordTableKey>,
}

impl InboundWatchIdAllocator {
    pub fn lookup(&mut self, raw_id: u64) -> VeilidAPIResult<Option<InboundWatchId>> {
        let id = InboundWatchId::new(raw_id)?;
        Ok(self.all_watch_ids.contains_key(&id).then_some(id))
    }

    pub fn allocate(&mut self, rtk: RecordTableKey) -> VeilidAPIResult<InboundWatchId> {
        // Generate a record-unique watch id > 0
        let mut id = 0;
        while id == 0 {
            id = get_random_u64();
        }
        // Make sure it doesn't match any other id or zero (unlikely, but lets be certain)
        let mut id = InboundWatchId::new(id)?;
        let starting_id = id;
        while self.all_watch_ids.contains_key(&id) {
            let next_id = u64::from(id).overflowing_add(1);
            id = InboundWatchId::new(next_id.0 + if next_id.1 { 1 } else { 0 })?;
            if id == starting_id {
                apibail_internal!("unable to allocate watch id");
            }
        }

        if self.all_watch_ids.insert(id, rtk).is_some() {
            apibail_internal!("allocated already existing inbound watch id");
        }

        Ok(id)
    }

    pub fn get_key(&self, id: InboundWatchId) -> Option<RecordTableKey> {
        self.all_watch_ids.get(&id).cloned()
    }

    pub fn free(&mut self, id: InboundWatchId) -> VeilidAPIResult<()> {
        if self.all_watch_ids.remove(&id).is_none() {
            apibail_internal!("freeing non-existent inbound watch id");
        }
        Ok(())
    }
}
