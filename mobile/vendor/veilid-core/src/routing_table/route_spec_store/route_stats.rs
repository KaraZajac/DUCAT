use super::*;
use crate::rpc_processor::Destination;
use hashlink::LruCache;

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteStatsDisposition {
    /// Route (or ordering) is still valid
    Valid,
    /// Invalid because it has failed to send too many times
    InvalidSendFailure,
    /// Invalid because it has lost too many answers
    InvalidLostQuestions,
}

impl RouteStatsDisposition {
    /// Whether this disposition means the route/ordering is dead (failed).
    pub fn is_invalid(&self) -> bool {
        matches!(
            self,
            RouteStatsDisposition::InvalidSendFailure | RouteStatsDisposition::InvalidLostQuestions
        )
    }
}

/// Per-`SequenceOrdering` validity/failure stats. These are the **single source
/// of truth** for whether a route works at a given ordering; every aggregate
/// view on [RouteStats] is derived from these, never mutated directly. A
/// success or failure always happens on *some* ordering (the transport used),
/// so the recorder always knows which entry to update.
#[derive(Clone, Debug, Default)]
pub struct RouteOrderingStats {
    /// Consecutive failed-to-send count (loopback test failures count here too)
    pub failed_to_send: usize,
    /// Per-destination lost question tracking: destination pubkey -> lost count
    pub lost_question_destinations: HashMap<PublicKey, usize>,
    /// When this ordering was last known valid (loopback pass / rcvd traffic)
    pub last_known_valid_ts: Option<Timestamp>,
    /// When this ordering was last sent to
    pub last_sent_ts: Option<Timestamp>,
    /// When this ordering last received a question or statement
    pub last_rcvd_question_ts: Option<Timestamp>,
    /// When this ordering last received an answer
    pub last_rcvd_answer_ts: Option<Timestamp>,
    /// Last periodic loopback keepalive enqueue for this ordering
    pub last_loopback_keepalive_ts: Option<Timestamp>,
    /// Answer stats for this ordering
    pub answer: AnswerStats,
    /// Accounting mechanism for this ordering's RPC answers
    answer_stats_accounting: AnswerStatsAccounting,
}

impl RouteOrderingStats {
    /// Disposition of this single ordering (route-level Locked handled by caller).
    pub fn disposition(&self) -> RouteStatsDisposition {
        if self.failed_to_send >= ROUTE_SEND_FAILURES_INVALIDATE_THRESHOLD {
            RouteStatsDisposition::InvalidSendFailure
        } else if self.lost_question_destinations.len()
            >= ROUTE_LOST_DESTINATIONS_INVALIDATE_THRESHOLD
        {
            RouteStatsDisposition::InvalidLostQuestions
        } else {
            RouteStatsDisposition::Valid
        }
    }

    /// Does this ordering need (re)testing?
    pub fn needs_testing(&self, cur_ts: Timestamp) -> bool {
        if !self.lost_question_destinations.is_empty() || self.failed_to_send > 0 {
            return true;
        }
        if let Some(last_tested_ts) = self.last_known_valid_ts {
            cur_ts.duration_since(last_tested_ts)
                > TimestampDuration::new_ms(ROUTE_MIN_IDLE_TIME_MS as u64)
        } else {
            true
        }
    }
}

#[derive(Clone, Debug)]
pub struct RouteStats {
    /// Timestamp of when the route was created
    pub created_ts: Timestamp,
    /// Per-`SequenceOrdering` stats — single source of truth for validity/failure.
    pub per_ordering: BTreeMap<SequenceOrdering, RouteOrderingStats>,
    /// Transfers up and down (route-level for now; reserve the ability to move
    /// per-ordering later via a rollup/update).
    pub transfer: TransferStatsDownUp,
    /// Latency stats (route-level for now; same reservation as transfer).
    pub latency: LatencyStats,
    /// Per-local-route stats keyed by local-route pubkey on our side.
    pub routed_stats: LruCache<PublicKey, PerRouteStats>,
    /// Accounting mechanism for this route's RPC latency (route-level)
    latency_stats_accounting: LatencyStatsAccounting,
    /// Accounting mechanism for the bandwidth across this route (route-level)
    transfer_stats_accounting: TransferStatsAccounting,
}

impl Default for RouteStats {
    fn default() -> Self {
        Self {
            created_ts: Timestamp::default(),
            per_ordering: BTreeMap::new(),
            transfer: TransferStatsDownUp::default(),
            latency: LatencyStats::default(),
            routed_stats: LruCache::new(PER_ROUTE_STATS_LRU_SIZE),
            latency_stats_accounting: LatencyStatsAccounting::default(),
            transfer_stats_accounting: TransferStatsAccounting::default(),
        }
    }
}

impl fmt::Display for RouteStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "created: {}", self.created_ts)?;
        writeln!(
            f,
            "last_known_valid:   {}",
            f.to_string_opt(self.last_known_valid_ts())
        )?;
        writeln!(
            f,
            "last_sent:          {}",
            f.to_string_opt(self.last_sent_ts())
        )?;
        writeln!(
            f,
            "last_rcvd_answer:   {}",
            f.to_string_opt(self.last_rcvd_answer_ts())
        )?;
        for (ordering, os) in self.per_ordering.iter() {
            writeln!(
                f,
                "  [{}] valid={} sent={} rq={} ra={} lost-dests/failed={}/{} answer={}",
                ordering,
                f.to_string_opt(os.last_known_valid_ts),
                f.to_string_opt(os.last_sent_ts),
                f.to_string_opt(os.last_rcvd_question_ts),
                f.to_string_opt(os.last_rcvd_answer_ts),
                os.lost_question_destinations.len(),
                os.failed_to_send,
                f.to_string(&os.answer),
            )?;
        }
        writeln!(f, "transfer:           {}", f.to_string(&self.transfer))?;
        writeln!(f, "latency:            {}", f.to_string(&self.latency))?;
        if !self.routed_stats.is_empty() {
            writeln!(f, "routed_stats:")?;
            for (key, entry) in self.routed_stats.iter() {
                writeln!(f, "  {}:", key)?;
                writeln!(
                    f,
                    "{}",
                    indent_all_string(indent_all_string(f.to_string(entry)))
                )?;
            }
        }

        Ok(())
    }
}

impl RouteStats {
    /// Make new route stats
    pub fn new(created_ts: Timestamp) -> Self {
        Self {
            created_ts,
            ..Default::default()
        }
    }

    /// Make new route stats from a route set spec detail stats
    pub fn new_from_spec_detail_stats(stats: RouteSetSpecDetailStats) -> Self {
        let mut per_ordering = BTreeMap::<SequenceOrdering, RouteOrderingStats>::new();
        for (ordering, answer) in stats.answer_by_ordering {
            per_ordering.insert(
                ordering,
                RouteOrderingStats {
                    answer,
                    ..Default::default()
                },
            );
        }
        Self {
            created_ts: stats.created_ts,
            transfer: stats.transfer,
            latency: stats.latency,
            per_ordering,
            ..Default::default()
        }
    }

    /// Mutable access to a single ordering's stats, created on demand.
    fn ordering_mut(&mut self, ordering: SequenceOrdering) -> &mut RouteOrderingStats {
        self.per_ordering.entry(ordering).or_default()
    }

    /////////////////////////////////////////////////////////////////
    // Derived aggregate views (computed from per_ordering, never stored)

    /// Most recent "known valid" timestamp across all orderings.
    pub fn last_known_valid_ts(&self) -> Option<Timestamp> {
        self.per_ordering
            .values()
            .filter_map(|o| o.last_known_valid_ts)
            .max()
    }

    /// Most recent "sent" timestamp across all orderings.
    pub fn last_sent_ts(&self) -> Option<Timestamp> {
        self.per_ordering
            .values()
            .filter_map(|o| o.last_sent_ts)
            .max()
    }

    /// Most recent "received answer" timestamp across all orderings.
    pub fn last_rcvd_answer_ts(&self) -> Option<Timestamp> {
        self.per_ordering
            .values()
            .filter_map(|o| o.last_rcvd_answer_ts)
            .max()
    }

    /// Last loopback keepalive enqueue for a given ordering.
    pub fn last_loopback_keepalive_ts(&self, ordering: SequenceOrdering) -> Option<Timestamp> {
        self.per_ordering
            .get(&ordering)
            .and_then(|o| o.last_loopback_keepalive_ts)
    }

    /// Disposition for a single ordering, derived purely from viability stats.
    pub fn ordering_disposition(&self, ordering: SequenceOrdering) -> RouteStatsDisposition {
        // No entry => untested, treated as Valid (optimistic; not dead).
        self.per_ordering
            .get(&ordering)
            .map(|o| o.disposition())
            .unwrap_or(RouteStatsDisposition::Valid)
    }

    /// Is the route dead for a given ordering (its ordering disposition is invalid)?
    pub fn is_dead_for_ordering(&self, ordering: SequenceOrdering) -> bool {
        self.ordering_disposition(ordering).is_invalid()
    }

    /// Dead for *every* ordering it provides. An `EnsureOrdered` route
    /// (orderings = {Ordered}) dies when Ordered dies, never waiting on Unordered.
    pub fn is_dead_for(&self, orderings: SequenceOrderingSet) -> bool {
        !orderings.is_empty() && orderings.iter().all(|o| self.is_dead_for_ordering(o))
    }

    /// Disposition aggregated over all orderings (any ordering Valid => Valid).
    pub fn aggregate_disposition(&self) -> RouteStatsDisposition {
        let mut worst = RouteStatsDisposition::Valid;
        for o in self.per_ordering.values() {
            match o.disposition() {
                RouteStatsDisposition::Valid => return RouteStatsDisposition::Valid,
                d => worst = d,
            }
        }
        worst
    }

    /////////////////////////////////////////////////////////////////
    // Per-ordering mutators (the only writers of validity/failure state)

    /// Mark a route as having failed to send at the given ordering
    pub fn record_send_failed(&mut self, ordering: SequenceOrdering) {
        self.ordering_mut(ordering).failed_to_send += 1;
    }

    /// Mark a route as having lost an answer for a destination at the given ordering
    pub fn record_lost_question(&mut self, ordering: SequenceOrdering, destination: &Destination) {
        let cur_ts = Timestamp::now();
        let os = self.ordering_mut(ordering);
        os.answer_stats_accounting.record_lost_question(cur_ts);

        let Ok(dest_key) = destination.destination_key() else {
            return;
        };
        *os.lost_question_destinations.entry(dest_key).or_insert(0) += 1;
    }

    /// Mark a route as having received a question or statement at the given ordering
    pub fn record_question_received(
        &mut self,
        ordering: SequenceOrdering,
        cur_ts: Timestamp,
        bytes: ByteCount,
    ) {
        let os = self.ordering_mut(ordering);
        os.last_rcvd_question_ts = Some(cur_ts);
        os.last_known_valid_ts = Some(cur_ts);
        os.answer_stats_accounting.record_question(cur_ts);
        self.transfer_stats_accounting.add_down(bytes);
    }

    /// Mark a route as having received an answer from a destination at the given ordering
    pub fn record_answer_received(
        &mut self,
        ordering: SequenceOrdering,
        cur_ts: Timestamp,
        bytes: ByteCount,
        _destination: &Destination,
    ) {
        let os = self.ordering_mut(ordering);
        os.last_rcvd_answer_ts = Some(cur_ts);
        os.last_known_valid_ts = Some(cur_ts);
        os.answer_stats_accounting.record_answer(cur_ts);
        // Receiving any answer at this ordering means the ordering is functional.
        os.lost_question_destinations.clear();
        self.transfer_stats_accounting.add_down(bytes);
    }

    /// Mark a route as having been sent to at the given ordering
    pub fn record_sent(&mut self, ordering: SequenceOrdering, cur_ts: Timestamp, bytes: ByteCount) {
        let os = self.ordering_mut(ordering);
        os.last_sent_ts = Some(cur_ts);
        // If we sent successfully, reset this ordering's 'failed_to_send'
        os.failed_to_send = 0;
        self.transfer_stats_accounting.add_up(bytes);
    }

    /// Record a loopback route test result for an ordering. A pass marks the
    /// ordering valid and clears its failures; a fail counts as a send failure
    /// for that ordering, so repeated loopback failures drive the ordering (and
    /// eventually the route) to dead.
    pub fn record_loopback_result(
        &mut self,
        ordering: SequenceOrdering,
        ok: bool,
        cur_ts: Timestamp,
    ) {
        let os = self.ordering_mut(ordering);
        if ok {
            os.last_known_valid_ts = Some(cur_ts);
            os.failed_to_send = 0;
            os.lost_question_destinations.clear();
        } else {
            os.failed_to_send += 1;
        }
    }

    /// Mark when we last enqueued a periodic loopback keepalive for this route at a given ordering
    pub fn record_loopback_keepalive(&mut self, cur_ts: Timestamp, ordering: SequenceOrdering) {
        self.ordering_mut(ordering).last_loopback_keepalive_ts = Some(cur_ts);
    }

    /// Record latency for this route (route-level)
    pub fn record_latency(&mut self, latency: TimestampDuration) {
        self.latency = self.latency_stats_accounting.record_latency(latency);
    }

    pub fn record_routed_up(&mut self, key: &PublicKey, bytes: ByteCount) {
        self.routed_stats
            .entry(key.clone())
            .or_insert_with(PerRouteStats::default)
            .add_up(bytes);
    }
    pub fn record_routed_down(&mut self, key: &PublicKey, bytes: ByteCount) {
        self.routed_stats
            .entry(key.clone())
            .or_insert_with(PerRouteStats::default)
            .add_down(bytes);
    }
    pub fn record_routed_round_trip(
        &mut self,
        key: &PublicKey,
        sample: TimestampDuration,
        bytes: ByteCount,
    ) {
        let entry = self
            .routed_stats
            .entry(key.clone())
            .or_insert_with(PerRouteStats::default);
        entry.record_round_trip(sample);
        entry.add_down(bytes);
    }

    /// Roll transfers for these route stats (route-level)
    pub fn roll_transfers(&mut self, last_ts: Timestamp, cur_ts: Timestamp) {
        self.transfer_stats_accounting
            .roll_transfers(last_ts, cur_ts, &mut self.transfer);
        for (_key, entry) in self.routed_stats.iter_mut() {
            entry.roll_transfers(last_ts, cur_ts);
        }
    }

    /// Roll answers per ordering
    pub fn roll_answers(&mut self, cur_ts: Timestamp) {
        for os in self.per_ordering.values_mut() {
            os.answer = os.answer_stats_accounting.roll_answers(cur_ts);
        }
    }

    /// Snapshot per-ordering answer stats for persistence
    pub fn answer_by_ordering(&self) -> Vec<(SequenceOrdering, AnswerStats)> {
        self.per_ordering
            .iter()
            .map(|(o, os)| (*o, os.answer.clone()))
            .collect()
    }

    /// Get the latency stats
    pub fn latency_stats(&self) -> &LatencyStats {
        &self.latency
    }

    /// Get the transfer stats
    pub fn transfer_stats(&self) -> &TransferStatsDownUp {
        &self.transfer
    }

    /// Reset stats when network restarts
    pub fn reset(&mut self) {
        self.per_ordering.clear();
        self.routed_stats.clear();
    }

    /// Check if a route needs testing for any of the orderings it provides.
    pub fn needs_testing(&self, orderings: SequenceOrderingSet, cur_ts: Timestamp) -> bool {
        // Need testing if any provided ordering has never been tested or is stale/failing.
        orderings.iter().any(|o| {
            self.per_ordering
                .get(&o)
                .map(|os| os.needs_testing(cur_ts))
                .unwrap_or(true)
        })
    }
}

impl RouteSpecStore {
    /// Modify the route statistics for allocated routes
    /// Changes made to the route stats may invalidate the route in the compiled route cache
    /// or lead to the route being tested and eventually removal.
    pub fn update_allocated_route_stats<F>(&self, cur_ts: Timestamp, key: &PublicKey, f: F)
    where
        F: FnOnce(&mut RouteStats) -> VeilidAPIResult<()>,
    {
        // Check for stub route (ignore changes to stubs)
        if self.routing_table().public_keys().contains(key) {
            return;
        }

        let cache = self.cache.read();
        if let Err(e) = cache.update_allocated_route_stats(cur_ts, key, f) {
            veilid_log!(self error "Error updating route stats for allocated route {}: {}", key, e);
        }
    }

    /// After a route-test failure, mark the route for release if it's now dead for all its
    /// orderings. Sticky, so a later send resetting failed_to_send can't un-mark it.
    pub fn mark_allocated_route_for_release_if_dead(&self, key: &PublicKey) {
        let opt_release_rid = {
            let cache = self.cache.read();
            let Some(rid) = cache.get_allocated_route_id_by_key(key) else {
                return;
            };
            let Some(arce) = cache.get_allocated_route_by_id(&rid) else {
                return;
            };
            if arce.with_stats(|s| s.is_dead_for(arce.orderings())) && !arce.is_marked_for_release()
            {
                veilid_log!(self debug "Marking dead route for release: {}", key);
                arce.mark_for_release();
                // Nothing holds it right now -> release immediately
                (!arce.is_locked()).then_some(rid)
            } else {
                None
            }
        };
        if let Some(rid) = opt_release_rid {
            self.release_allocated_route(rid);
        }
    }

    /// Modify the route statistics for remote routes
    /// Changes made to the route stats may invalidate the route in the compiled route cache
    pub fn update_remote_route_stats<F>(&self, cur_ts: Timestamp, key: &PublicKey, f: F)
    where
        F: FnOnce(&mut RouteStats) -> VeilidAPIResult<()>,
    {
        // Check for stub route (ignore changes to stubs)
        if self.routing_table().public_keys().contains(key) {
            return;
        }

        let cache = self.cache.read();
        if let Err(e) = cache.update_remote_route_stats(cur_ts, key, f) {
            veilid_log!(self error "Error updating route stats for remote route {}: {}", key, e);
        }
    }

    /// Process transfer statistics to get averages
    pub fn roll_transfers(&self, last_ts: Timestamp, cur_ts: Timestamp) {
        // Careful with locking order here, we need to lock the content before the cache
        let content = self.content.read();
        let cache = self.cache.read();

        // Roll transfers for allocated route cache and remote private routes
        cache.roll_transfers(last_ts, cur_ts);

        // Update transfers from cache into content to save them
        content.update_transfers(&cache);

        // Also update latency here because we don't use this in realtime from the content like we do from the cache
        content.update_latency(&cache);
    }

    /// Process answer statistics
    pub fn roll_answers(&self, cur_ts: Timestamp) {
        // Careful with locking order here, we need to lock the content before the cache
        let content = self.content.read();
        let cache = self.cache.read();

        // Roll transfers for the cache
        cache.roll_answers(cur_ts);

        // Update answers from cache into content to save them
        content.update_answers(&cache);
    }
}
