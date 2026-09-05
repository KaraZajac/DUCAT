use super::*;

/// Per-train mutable state, shared by every queued and in-flight keepalive for that train.
#[derive(Debug, Default)]
pub(super) struct OutboundTransactionKeepaliveState {
    /// Most recent successful keepalive RTT.
    pub opt_last_rtt: Option<TimestampDuration>,
    /// Most recent server-reported expiration; drives the next send time.
    pub opt_last_expiration: Option<Timestamp>,
    /// Server told us the inbound transaction is gone. Future fires should bail.
    pub dead: bool,
}

/// Ndode transaction keepalive train
#[derive(Debug, Clone)]
pub(super) struct OutboundTransactionKeepalive {
    params: Arc<OutboundTransactionKeepaliveParams>,
    state: Arc<Mutex<OutboundTransactionKeepaliveState>>,
}

impl OutboundTransactionKeepalive {
    pub(super) fn new(
        params: Arc<OutboundTransactionKeepaliveParams>,
        state: Arc<Mutex<OutboundTransactionKeepaliveState>>,
    ) -> Self {
        Self { params, state }
    }

    pub(super) fn lnxid(&self) -> LocalNodeTransactionId {
        self.params.lnxid
    }

    pub(super) fn opaque_record_key(&self) -> &OpaqueRecordKey {
        &self.params.opaque_record_key
    }

    pub(super) fn rnxid(&self) -> &RemoteNodeTransactionId {
        &self.params.rnxid
    }

    pub(super) fn dest(&self) -> &Destination {
        &self.params.dest
    }

    pub(super) fn descriptor(&self) -> &Arc<SignedValueDescriptor> {
        &self.params.descriptor
    }

    pub(super) fn state(&self) -> &Arc<Mutex<OutboundTransactionKeepaliveState>> {
        &self.state
    }
}

/// Shared deadline state protected by sync mutex
#[derive(Debug)]
pub(super) struct OutboundTransactionKeepaliveDeadlines {
    /// Registry for logging
    registry: VeilidComponentRegistry,
    /// Scheduled deadlines sorted by timestamp. Each deadline maps to one or more OutboundTransactionKeepalive items
    deadlines: BTreeMap<Timestamp, Vec<OutboundTransactionKeepalive>>,
    /// Server-side transaction TTL. Used to derive the next send time.
    ttl: TimestampDuration,
}

impl VeilidComponentRegistryAccessor for OutboundTransactionKeepaliveDeadlines {
    fn registry(&self) -> VeilidComponentRegistry {
        self.registry.clone()
    }
}

impl OutboundTransactionKeepaliveDeadlines {
    pub fn new(registry: VeilidComponentRegistry, ttl: TimestampDuration) -> Self {
        Self {
            registry,
            deadlines: BTreeMap::new(),
            ttl,
        }
    }

    /// Timestamp of the earliest scheduled keepalive, if any.
    pub fn next_deadline(&self) -> Option<Timestamp> {
        self.deadlines.keys().next().copied()
    }

    /// Schedule the next deadline for this keepalive. Silently drops if
    /// the handle is cancelled; computes the next send time from the keepalive's
    /// state and the configured TTL.
    pub fn schedule_deadline(
        &mut self,
        keepalive: OutboundTransactionKeepalive,
        send_ts: Timestamp,
    ) {
        let next_ts = {
            let st = keepalive.state.lock();
            if st.dead {
                return;
            }
            Self::compute_next_send_ts(send_ts, st.opt_last_rtt, st.opt_last_expiration, self.ttl)
        };
        self.deadlines.entry(next_ts).or_default().push(keepalive);
    }

    /// Cancel all scheduled deadlines for a transaction and mark it dead so new deadlines aren't scheduled.
    pub fn cancel_deadline(&mut self, lnxid: LocalNodeTransactionId) {
        self.deadlines.retain(|_ts, entries| {
            entries.retain(|keepalive| {
                if keepalive.lnxid() == lnxid {
                    // Mark as dead so any clones are not rescheduled
                    keepalive.state.lock().dead = true;

                    // Remove from the deadline schedule
                    false
                } else {
                    // Keep alive
                    true
                }
            });

            // Remove if there are no entries left at this timestamp
            !entries.is_empty()
        });
    }

    /// Remove and return all entries whose deadline has passed.
    pub fn take_due_entries(&mut self) -> Vec<OutboundTransactionKeepalive> {
        let cur_ts = Timestamp::now();
        // split_off returns [cur_ts+1ms..), leaving [..=cur_ts] in self.deadlines
        let future = self
            .deadlines
            .split_off(&cur_ts.later(TimestampDuration::new_ms(1)));
        let due = std::mem::replace(&mut self.deadlines, future);
        due.into_values().flatten().collect()
    }

    /// Total number of scheduled deadline entries across all timestamps.
    pub fn len(&self) -> usize {
        self.deadlines.values().map(|v| v.len()).sum()
    }

    /// Pick the next send time. Aim for `expiration - safety_margin` when known,
    /// else use the ceiling. Clamp to [send_ts + floor, send_ts + ceiling].
    fn compute_next_send_ts(
        send_ts: Timestamp,
        opt_last_rtt: Option<TimestampDuration>,
        opt_last_expiration: Option<Timestamp>,
        transaction_ttl: TimestampDuration,
    ) -> Timestamp {
        let floor = transaction_ttl.div(KEEPALIVE_INTERVAL_FLOOR_TTL_FRACTION);
        let ceiling = transaction_ttl.div(KEEPALIVE_INTERVAL_CEILING_TTL_FRACTION);
        let min_next = send_ts.later(floor);
        let max_next = send_ts.later(ceiling);

        let Some(expiration) = opt_last_expiration else {
            return max_next;
        };
        let rtt = opt_last_rtt.unwrap_or(floor);
        let safety = rtt.saturating_mul(KEEPALIVE_SAFETY_RTT_FACTOR);
        let target = expiration.earlier(safety);

        if target < min_next {
            min_next
        } else if target > max_next {
            max_next
        } else {
            target
        }
    }
}
