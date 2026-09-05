use super::*;

#[derive(Debug)]
pub(super) struct OperationWaitHandle<T, C>
where
    T: Unpin,
    C: Unpin + Clone,
{
    waiter: OperationWaiter<T, C>,
    op_id: OperationId,
    result_receiver: flume::Receiver<(Span, T)>,
}

impl<T, C> OperationWaitHandle<T, C>
where
    T: Unpin,
    C: Unpin + Clone,
{
    pub fn id(&self) -> OperationId {
        self.op_id
    }
}

impl<T, C> Drop for OperationWaitHandle<T, C>
where
    T: Unpin,
    C: Unpin + Clone,
{
    fn drop(&mut self) {
        self.waiter.cancel_op_waiter(self.op_id);
    }
}

#[derive(Debug)]
struct OperationWaitingOp<T, C>
where
    T: Unpin,
    C: Unpin + Clone,
{
    context: C,
    timestamp: Timestamp,
    result_sender: flume::Sender<(Span, T)>,
}

#[derive(Debug)]
struct OperationWaiterInner<T, C>
where
    T: Unpin,
    C: Unpin + Clone,
{
    waiting_op_table: HashMap<OperationId, OperationWaitingOp<T, C>>,
}

#[derive(Debug)]
pub(super) struct OperationWaiter<T, C>
where
    T: Unpin,
    C: Unpin + Clone,
{
    registry: VeilidComponentRegistry,
    inner: Arc<Mutex<OperationWaiterInner<T, C>>>,
}

impl<T, C> VeilidComponentRegistryAccessor for OperationWaiter<T, C>
where
    T: Unpin,
    C: Unpin + Clone,
{
    fn registry(&self) -> VeilidComponentRegistry {
        self.registry.clone()
    }
}

impl<T, C> Clone for OperationWaiter<T, C>
where
    T: Unpin,
    C: Unpin + Clone,
{
    fn clone(&self) -> Self {
        Self {
            registry: self.registry.clone(),
            inner: self.inner.clone(),
        }
    }
}

impl<T, C> OperationWaiter<T, C>
where
    T: Unpin,
    C: Unpin + Clone,
{
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            registry,
            inner: Arc::new(Mutex::new(OperationWaiterInner {
                waiting_op_table: HashMap::new(),
            })),
        }
    }

    /// Set up wait for operation to complete
    pub fn add_op_waiter(&self, op_id: OperationId, context: C) -> OperationWaitHandle<T, C> {
        let (result_sender, result_receiver) = flume::bounded(1);
        let waiting_op = OperationWaitingOp {
            context,
            timestamp: Timestamp::now_non_decreasing(),
            result_sender,
        };

        {
            let mut inner = self.inner.lock();
            if inner.waiting_op_table.insert(op_id, waiting_op).is_some() {
                error!(
                    "add_op_waiter collision should not happen for op_id {}",
                    op_id
                );
            }
        }

        OperationWaitHandle {
            waiter: self.clone(),
            op_id,
            result_receiver,
        }
    }

    /// Get all waiting operation ids
    pub fn get_operation_ids(&self) -> Vec<OperationId> {
        let inner = self.inner.lock();
        let mut opids: Vec<(OperationId, Timestamp)> = inner
            .waiting_op_table
            .iter()
            .map(|x| (*x.0, x.1.timestamp))
            .collect();
        opids.sort_unstable_by_key(|a| a.1);
        opids.into_iter().map(|x| x.0).collect()
    }

    /// Get operation context
    pub fn get_op_context(&self, op_id: OperationId) -> Result<C, RPCError> {
        let inner = self.inner.lock();
        let res = {
            let Some(waiting_op) = inner.waiting_op_table.get(&op_id) else {
                return Err(RPCError::ignore(format!(
                    "Missing operation id getting op context: id={}",
                    op_id
                )));
            };
            Ok(waiting_op.context.clone())
        };
        drop(inner);
        res
    }

    /// Remove wait for op
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    fn cancel_op_waiter(&self, op_id: OperationId) {
        let mut inner = self.inner.lock();
        inner.waiting_op_table.remove(&op_id);
    }

    /// Complete the waiting op
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub fn complete_op_waiter(&self, op_id: OperationId, message: T) -> Result<(), RPCError> {
        let waiting_op = {
            let mut inner = self.inner.lock();
            inner
                .waiting_op_table
                .remove(&op_id)
                .ok_or_else(RPCError::else_ignore(format!(
                    "Unmatched operation id: {}",
                    op_id
                )))?
        };
        waiting_op
            .result_sender
            .send((Span::current(), message))
            .map_err(RPCError::ignore)
    }

    /// Wait for operation to complete
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn wait_for_op(
        &self,
        handle: OperationWaitHandle<T, C>,
        timeout_us: TimestampDuration,
    ) -> Result<TimeoutOr<(T, TimestampDuration)>, RPCError> {
        let result_fut = handle.result_receiver.recv_async().in_current_span();

        // wait for eventualvalue
        let start_ts = Timestamp::now();
        let res = result_fut
            .timeout_duration(timeout_us)
            .await
            .into_timeout_or();

        match res {
            TimeoutOr::Timeout => Ok(TimeoutOr::Timeout),
            TimeoutOr::Value(Ok((_span_id, ret))) => {
                let end_ts = Timestamp::now();

                //xxx: causes crash (Missing otel data span extensions)
                // Span::current().follows_from(span_id);

                Ok(TimeoutOr::Value((ret, end_ts.duration_since(start_ts))))
            }
            TimeoutOr::Value(Err(e)) => {
                //
                Err(RPCError::ignore(e))
            }
        }
    }
}
