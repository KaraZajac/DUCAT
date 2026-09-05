mod inbound_transaction;
mod inbound_transaction_id;
mod inbound_transaction_id_allocator;
mod inbound_transaction_list;

use super::*;

pub use inbound_transaction::*;
pub use inbound_transaction_id::*;
pub use inbound_transaction_id_allocator::*;
pub use inbound_transaction_list::*;

impl_veilid_log_facility!("stor");

#[derive(Debug, Default)]
pub struct InboundTransactions {
    /// The set of records for which transactions are active. The records may not exist in the store until committed.
    record_transactions: HashMap<RecordTableKey, InboundTransactionList>,

    /// The set of all allocated transaction ids
    transaction_id_allocator: InboundTransactionIdAllocator,
}

impl InboundTransactions {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn allocate(
        &mut self,
        opaque_record_key: &OpaqueRecordKey,
        expiration: Timestamp,
        signing_member_id: MemberId,
        descriptor: Arc<SignedValueDescriptor>,
        opt_snapshot: Option<Arc<RecordSnapshot>>,
    ) -> VeilidAPIResult<InboundTransactionId> {
        let rtk = RecordTableKey {
            record_key: opaque_record_key.clone(),
        };

        // Generate a record-unique transaction id > 0
        let id = self.transaction_id_allocator.allocate(rtk.clone())?;

        // Create a new transaction
        let inbound_transaction_list = self.record_transactions.entry(rtk).or_default();
        inbound_transaction_list.new_transaction(
            id,
            expiration,
            signing_member_id,
            descriptor.clone(),
            opt_snapshot,
        );

        Ok(id)
    }

    pub fn lookup_id(&mut self, raw_id: u64) -> VeilidAPIResult<Option<InboundTransactionId>> {
        self.transaction_id_allocator.lookup(raw_id)
    }

    pub fn check_id(&self, id: InboundTransactionId, rtk: &RecordTableKey) -> bool {
        self.transaction_id_allocator.get_key(id).as_ref() == Some(rtk)
    }

    pub fn get(&self, rtk: &RecordTableKey) -> Option<&InboundTransactionList> {
        self.record_transactions.get(rtk)
    }

    pub fn get_mut(&mut self, rtk: &RecordTableKey) -> Option<&mut InboundTransactionList> {
        self.record_transactions.get_mut(rtk)
    }

    pub fn try_remove_record(&mut self, rtk: &RecordTableKey) -> VeilidAPIResult<bool> {
        let Some(inbound_transaction_list) = self.record_transactions.remove(rtk) else {
            return Ok(false);
        };
        let dead_ids = inbound_transaction_list
            .transactions()
            .map(|x| x.id())
            .collect::<Vec<_>>();
        for dead_id in dead_ids {
            self.transaction_id_allocator.free(dead_id)?;
        }
        Ok(true)
    }

    pub fn try_remove_transaction(&mut self, id: InboundTransactionId) -> VeilidAPIResult<bool> {
        let Some(rtk) = self.transaction_id_allocator.get_key(id) else {
            return Ok(false);
        };
        let Some(inbound_transaction_list) = self.record_transactions.get_mut(&rtk) else {
            apibail_internal!("record does not exist for transaction id");
        };
        let alive =
            inbound_transaction_list.drop_transaction(id, &mut self.transaction_id_allocator)?;
        if !alive {
            self.record_transactions.remove(&rtk);
        }
        Ok(true)
    }

    pub fn remove_transaction(&mut self, id: InboundTransactionId) -> VeilidAPIResult<()> {
        let Some(rtk) = self.transaction_id_allocator.get_key(id) else {
            apibail_internal!("transaction id does not exist");
        };
        let Some(inbound_transaction_list) = self.record_transactions.get_mut(&rtk) else {
            apibail_internal!("record does not exist for transaction id");
        };
        let alive =
            inbound_transaction_list.drop_transaction(id, &mut self.transaction_id_allocator)?;
        if !alive {
            self.record_transactions.remove(&rtk);
        }
        Ok(())
    }

    pub fn remove_expired_transactions<D: Fn(InboundTransactionId), L: Fn(VeilidAPIError)>(
        &mut self,
        now: Timestamp,
        debug_logger: &D,
        error_logger: &L,
    ) {
        self.record_transactions
            .retain(|_, inbound_transaction_list| {
                inbound_transaction_list.drop_expired_transactions(
                    now,
                    &mut self.transaction_id_allocator,
                    debug_logger,
                    error_logger,
                )
            });
    }

    pub fn debug(&self) -> String {
        let mut out = format!(
            "Records with inbound transactions: {}\n",
            self.record_transactions.len()
        );
        let mut record_transactions_keys = self.record_transactions.keys().collect::<Vec<_>>();
        record_transactions_keys.sort_unstable();
        for atk in record_transactions_keys {
            let atx = self.record_transactions.get(atk).unwrap_or_log();
            out += &format!("  {}: {:?}\n", atk, atx);
        }
        out
    }
}
