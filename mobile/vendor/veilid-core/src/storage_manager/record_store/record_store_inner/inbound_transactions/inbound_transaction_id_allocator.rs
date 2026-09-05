use super::*;

#[derive(Default, Debug)]
pub struct InboundTransactionIdAllocator {
    all_transaction_ids: HashMap<InboundTransactionId, RecordTableKey>,
}

impl InboundTransactionIdAllocator {
    pub fn lookup(&mut self, raw_id: u64) -> VeilidAPIResult<Option<InboundTransactionId>> {
        let id = InboundTransactionId::new(raw_id)?;
        Ok(self.all_transaction_ids.contains_key(&id).then_some(id))
    }

    pub fn allocate(&mut self, rtk: RecordTableKey) -> VeilidAPIResult<InboundTransactionId> {
        // Generate a record-unique transaction id > 0
        let mut id = 0;
        while id == 0 {
            id = get_random_u64();
        }
        // Make sure it doesn't match any other id or zero (unlikely, but lets be certain)
        let mut id = InboundTransactionId::new(id)?;
        let starting_id = id;
        while self.all_transaction_ids.contains_key(&id) {
            let next_id = u64::from(id).overflowing_add(1);
            id = InboundTransactionId::new(next_id.0 + if next_id.1 { 1 } else { 0 })?;
            if id == starting_id {
                apibail_internal!("unable to allocate transaction id");
            }
        }

        if self.all_transaction_ids.insert(id, rtk).is_some() {
            apibail_internal!("allocated already existing inbound transaction id");
        }

        Ok(id)
    }

    pub fn get_key(&self, id: InboundTransactionId) -> Option<RecordTableKey> {
        self.all_transaction_ids.get(&id).cloned()
    }

    pub fn free(&mut self, id: InboundTransactionId) -> VeilidAPIResult<()> {
        if self.all_transaction_ids.remove(&id).is_none() {
            apibail_internal!("freeing non-existent inbound transaction id");
        }
        Ok(())
    }
}
