use super::*;

impl_veilid_log_facility!("stor");

#[derive(Debug, Default, Clone)]
/// Transactions active on a record
pub struct InboundTransactionList {
    /// The list of active transactions
    transactions: Vec<InboundTransaction>,
    /// If a transaction ends, with changed subkeys, it locks the record with its transaction id.
    /// During the lock, no other transactions on this record may begin, set, end with changes, or commit.
    lock: Option<InboundTransactionId>,
}

impl InboundTransactionList {
    pub(super) fn new_transaction(
        &mut self,
        id: InboundTransactionId,
        expiration: Timestamp,
        signing_member_id: MemberId,
        descriptor: Arc<SignedValueDescriptor>,
        opt_snapshot: Option<Arc<RecordSnapshot>>,
    ) {
        let inbound_transaction = InboundTransaction::new(
            id,
            expiration,
            signing_member_id,
            descriptor.clone(),
            opt_snapshot,
        );

        self.transactions.push(inbound_transaction);
    }

    pub fn get(&self, transaction_id: InboundTransactionId) -> Option<&InboundTransaction> {
        self.transactions.iter().find(|x| x.id() == transaction_id)
    }

    pub fn get_mut(
        &mut self,
        transaction_id: InboundTransactionId,
    ) -> Option<&mut InboundTransaction> {
        self.transactions
            .iter_mut()
            .find(|x| x.id() == transaction_id)
    }

    pub fn transactions(&self) -> impl Iterator<Item = &InboundTransaction> {
        self.transactions.iter()
    }

    pub(super) fn drop_transaction(
        &mut self,
        transaction_id: InboundTransactionId,
        allocator: &mut InboundTransactionIdAllocator,
    ) -> VeilidAPIResult<bool> {
        self.transactions.retain(|t| t.id() != transaction_id);
        if self.lock == Some(transaction_id) {
            self.lock = None;
        }
        allocator.free(transaction_id)?;

        Ok(!self.transactions.is_empty())
    }

    pub(super) fn drop_expired_transactions<D: Fn(InboundTransactionId), L: Fn(VeilidAPIError)>(
        &mut self,
        now: Timestamp,
        allocator: &mut InboundTransactionIdAllocator,
        debug_logger: &D,
        error_logger: &L,
    ) -> bool {
        self.transactions.retain(|t| {
            let alive = t.is_alive(now);
            if !alive {
                let id = t.id();
                if self.lock == Some(id) {
                    self.lock = None;
                }
                if let Err(e) = allocator.free(id) {
                    error_logger(e);
                }
                debug_logger(id);
            }
            alive
        });
        !self.transactions.is_empty()
    }

    pub fn is_locked_by(&self, transaction_id: InboundTransactionId) -> bool {
        self.lock == Some(transaction_id)
    }

    pub fn lock(&mut self, transaction_id: InboundTransactionId) -> VeilidAPIResult<()> {
        if let Some(existing_xid) = self.lock {
            apibail_internal!("request to lock inbound transaction list by xid {} when it was already locked by {}", transaction_id, existing_xid);
        }

        self.lock = Some(transaction_id);

        Ok(())
    }
}
