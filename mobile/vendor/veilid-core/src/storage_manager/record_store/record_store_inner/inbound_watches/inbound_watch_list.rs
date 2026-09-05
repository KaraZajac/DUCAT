use super::*;

#[derive(Debug, Default, Clone)]
/// A record being watched for changes
pub struct InboundWatchList {
    /// The list of active watches
    watches: Vec<InboundWatch>,
}

impl InboundWatchList {
    pub(super) fn new_watch(&mut self, id: InboundWatchId, params: InboundWatchParameters) {
        let inbound_watch = InboundWatch::new(id, params);

        self.watches.push(inbound_watch);
    }

    #[expect(dead_code)]
    pub fn get(&self, transaction_id: InboundWatchId) -> Option<&InboundWatch> {
        self.watches.iter().find(|x| x.id() == transaction_id)
    }

    pub fn get_mut(&mut self, transaction_id: InboundWatchId) -> Option<&mut InboundWatch> {
        self.watches.iter_mut().find(|x| x.id() == transaction_id)
    }

    pub fn watches(&self) -> impl Iterator<Item = &InboundWatch> {
        self.watches.iter()
    }

    pub fn watches_mut(&mut self) -> impl Iterator<Item = &mut InboundWatch> {
        self.watches.iter_mut()
    }

    pub(super) fn drop_watch(
        &mut self,
        watch_id: InboundWatchId,
        allocator: &mut InboundWatchIdAllocator,
    ) -> VeilidAPIResult<bool> {
        self.watches.retain(|t| t.id() != watch_id);
        allocator.free(watch_id)?;
        Ok(!self.watches.is_empty())
    }

    pub(super) fn drop_expired_watches<D: Fn(InboundWatchId), L: Fn(VeilidAPIError)>(
        &mut self,
        now: Timestamp,
        allocator: &mut InboundWatchIdAllocator,
        debug_logger: &D,
        error_logger: &L,
    ) -> bool {
        self.watches.retain(|w| {
            let alive = w.is_alive(now);
            if !alive {
                let id = w.id();
                if let Err(e) = allocator.free(id) {
                    error_logger(e);
                }
                debug_logger(id);
            }
            alive
        });
        !self.watches.is_empty()
    }
}
