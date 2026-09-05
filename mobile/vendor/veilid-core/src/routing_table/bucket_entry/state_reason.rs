use super::*;

impl_veilid_log_facility!("rtab");

//////////////////////////////////////////////////////////////////////////////////////////
// Reliable state thresholds

/// Reliable pings are done with increased spacing between pings
///
/// - Start secs is the number of seconds between the first two pings
pub(crate) const RELIABLE_PING_INTERVAL_START: TimestampDuration = TimestampDuration::new_secs(10);
/// - Max secs is the maximum number of seconds between consecutive pings.
///   Bounded so a reliable transport's last_seen can't get more than this stale before the
///   next ping, which keeps stopped-node detection timely.
pub(crate) const RELIABLE_PING_INTERVAL_MAX: TimestampDuration = TimestampDuration::new_secs(120);
/// - Multiplier changes the number of seconds between pings over time
///   making it longer as the node becomes more reliable
pub(crate) const RELIABLE_PING_INTERVAL_MULTIPLIER: f64 = 2.0;

//////////////////////////////////////////////////////////////////////////////////////////
// Unreliable state thresholds

/// Unreliable pings are done for a fixed amount of time while the
/// node is given a chance to come back online before it is made dead
/// If a node misses a single ping, it is marked missing and must
/// return reliable pings for the duration of the span before being
/// marked reliable again
///
/// - Span is the number of seconds of consecutive successful answers before a node is considered reliable
pub(crate) const UNRELIABLE_ANSWER_SPAN: TimestampDuration = TimestampDuration::new_secs(60);
/// - Interval is the number of seconds between each ping
pub(crate) const UNRELIABLE_PING_INTERVAL: TimestampDuration = TimestampDuration::new_secs(5);

//////////////////////////////////////////////////////////////////////////////////////////
// Missing state thresholds

/// - Number of consecutive lost questions on an unordered transport
///   at which time we call something missing
pub(crate) const MISSING_LOST_QUESTIONS_UNORDERED: u32 = 3;
/// - Number of consecutive lost questions on an ordered transport
///   at which time we call something missing
pub(crate) const MISSING_LOST_QUESTIONS_ORDERED: u32 = 1;
/// Default missing lost-answers threshold per SequenceOrdering
/// Will become adaptive later (per-network baseline).
pub(crate) fn missing_lost_questions_count(sequence_ordering: SequenceOrdering) -> u32 {
    match sequence_ordering {
        SequenceOrdering::Unordered => MISSING_LOST_QUESTIONS_UNORDERED,
        SequenceOrdering::Ordered => MISSING_LOST_QUESTIONS_ORDERED,
    }
}

//////////////////////////////////////////////////////////////////////////////////////////
// Dead state thresholds

/// - Span is the number of seconds of consecutive lost questions before a node is considered dead
pub(crate) const DEAD_LOST_QUESTION_SPAN: TimestampDuration = TimestampDuration::new_secs(60);
/// Failed node-level unreachable attempts (no usable transport) before we call it dead
pub(crate) const DEAD_UNREACHABLE_COUNT: u32 = 1;

/// Number of failed-to-send RPCs to a node over an unordered transport before we call it dead
pub(crate) const DEAD_FAILED_TO_SEND_UNORDERED: u32 = 9;
/// Number of failed-to-send RPCs to a node over an ordered transport before we call it dead
pub(crate) const DEAD_FAILED_TO_SEND_ORDERED: u32 = 3;
/// Default failed-to-send count per sequence ordering.
pub(crate) fn dead_failed_to_send_count(sequence_ordering: SequenceOrdering) -> u32 {
    match sequence_ordering {
        SequenceOrdering::Unordered => DEAD_FAILED_TO_SEND_UNORDERED,
        SequenceOrdering::Ordered => DEAD_FAILED_TO_SEND_ORDERED,
    }
}

/// Number of lost questions to a node over an unordered transport before we call it dead if we haven't seen any traffic from it yet
pub(crate) const DEAD_NEVER_SEEN_LOST_QUESTIONS_UNORDERED: u32 = 3;
/// Number of lost questions to a node over an ordered transport before we call it dead if we haven't seen any traffic from it yet
pub(crate) const DEAD_NEVER_SEEN_LOST_QUESTIONS_ORDERED: u32 = 1;
/// Default lost questions count for never-seen nodes per sequence ordering.
pub(crate) fn dead_never_seen_lost_questions_count(sequence_ordering: SequenceOrdering) -> u32 {
    match sequence_ordering {
        SequenceOrdering::Unordered => DEAD_NEVER_SEEN_LOST_QUESTIONS_UNORDERED,
        SequenceOrdering::Ordered => DEAD_NEVER_SEEN_LOST_QUESTIONS_ORDERED,
    }
}

impl BucketEntryInner {
    /// Node-level calculation of state.
    ///
    /// Cold start is Unreliable(Unseen). Pure state-reason computation with no side effects.
    ///
    /// * Punished:
    ///   - [P1] the node id is punished by the address filter
    /// * Dead: (any of the following are true)
    ///   - [D1] the node has per-entry unreachable >= DEAD_UNREACHABLE_COUNT
    ///   - [D2] the node has any per-sequence-ordering failed_to_send >= DEAD_FAILED_TO_SEND
    ///   - [D3] if no transports have been seen (per-transport), and:
    ///     - the node has any per-sequence-ordering recent_lost_questions >= DEAD_NEVER_SEEN_LOST_QUESTIONS
    ///   - [D4] if any transport has been seen (per-transport), and:
    ///     - for any sequence-ordering, all of:
    ///       - first_steady_lost_question_ts is Some
    ///       - time_since(first_steady_lost_question_ts) >= DEAD_LOST_QUESTION_SPAN
    /// * Missing (any of the following are true):
    ///   - [M1] the node has per-entry unreachable > 0 (only possible if DEAD_UNREACHABLE_COUNT > 1)
    ///   - [M2] the node has any per-sequence-ordering failed_to_send >= 0 (only possible if DEAD_FAILED_TO_SEND > 1)
    ///   - [M3] the node has any per-sequence-ordering recent_lost_questions > MISSING_LOST_QUESTIONS
    /// * Initial:
    ///   - [I1] for all sequence-orderings:
    ///     - first_steady_answer_ts is None (cold start)
    /// * Unreliable (any of the following are true):
    ///   - [U1] for any sequence-ordering:
    ///     - time_since(first_steady_answer_ts) < UNRELIABLE_ANSWER_SPAN
    /// * Reliable:
    ///   - [R1] Any other condition is reliable
    pub(super) fn compute_state_reason(&self, cur_ts: Timestamp) -> BucketEntryStateReason {
        // Credit reliability windows for time PublicInternet was offline (couldn't
        // ping). Zero when there's no network manager (early init / tests).
        let opt_nm = self.registry.lookup::<NetworkManager>();
        let offline_overlap_since = |from: Timestamp| {
            opt_nm
                .as_ref()
                .map(|nm| {
                    nm.online_detector().offline_overlap(
                        RoutingDomain::PublicInternet,
                        from,
                        cur_ts,
                    )
                })
                .unwrap_or_else(|| TimestampDuration::new(0))
        };
        Self::compute_state_reason_from_stats(
            cur_ts,
            self.punishment,
            &self.rpc_stats,
            &self.per_sequence_ordering_stats,
            &self.per_transport_stats,
            offline_overlap_since,
        )
    }

    /// Pure node-level state calculation over the stats it depends on. The wrapper
    /// `compute_state_reason` carries the full condition reference; tags below match it.
    pub(crate) fn compute_state_reason_from_stats(
        cur_ts: Timestamp,
        punishment: Option<PunishmentReason>,
        rpc_stats: &RPCStats,
        per_sequence_ordering_stats: &BTreeMap<SequenceOrdering, RPCStats>,
        per_transport_stats: &BTreeMap<TransportType, RPCStats>,
        offline_overlap_since: impl Fn(Timestamp) -> TimestampDuration,
    ) -> BucketEntryStateReason {
        // ---===/ Punished /===---

        // [P1]
        if let Some(p) = punishment {
            return BucketEntryStateReason::Punished(p);
        }

        // ---===/ Dead /===---

        // [D1] the node has per-entry unreachable >= DEAD_UNREACHABLE_COUNT
        // Node is dead if it is unreachable even once. That means no contact method could be chosen
        // or no routing domain could reach it. Giving this node more time will not help. If it comes
        // back with a better peerinfo, it will get another chance.
        if rpc_stats.unreachable >= DEAD_UNREACHABLE_COUNT {
            return BucketEntryStateReason::Dead(BucketEntryStateDeadReason::ExcessiveUnreachable);
        }

        // [D2] Node is dead if it has any per-sequence-ordering failed_to_send >= DEAD_FAILED_TO_SEND
        if per_sequence_ordering_stats
            .iter()
            .any(|(so, stats)| stats.failed_to_send >= dead_failed_to_send_count(*so))
        {
            return BucketEntryStateReason::Dead(BucketEntryStateDeadReason::ExcessiveSendFailures);
        }

        // [D3] if no transports have been seen (per-transport), and:
        //     - the node has any per-sequence-ordering recent_lost_questions >= DEAD_NEVER_SEEN_LOST_QUESTIONS
        if per_transport_stats.is_empty()
            && per_sequence_ordering_stats.iter().any(|(so, stats)| {
                stats.recent_lost_questions >= dead_never_seen_lost_questions_count(*so)
            })
        {
            return BucketEntryStateReason::Dead(
                BucketEntryStateDeadReason::NeverSeenLostQuestions,
            );
        }

        // [D4] if any transport has been seen (per-transport), and:
        //   - for any sequence-ordering, all of:
        //     - cur_ts.since(first_steady_lost_question_ts), minus offline time,
        //       >= DEAD_LOST_QUESTION_SPAN
        if !per_transport_stats.is_empty()
            && per_sequence_ordering_stats.values().any(|stats| {
                stats
                    .first_steady_lost_question_ts
                    .map(|ts| {
                        cur_ts
                            .duration_since(ts)
                            .saturating_sub(offline_overlap_since(ts))
                            >= DEAD_LOST_QUESTION_SPAN
                    })
                    .unwrap_or(false)
            })
        {
            return BucketEntryStateReason::Dead(BucketEntryStateDeadReason::SteadyLostQuestions);
        }

        // ---===/ Missing /===---

        // [M1] the node has per-entry unreachable > 0 (only possible if DEAD_UNREACHABLE_COUNT > 1)
        if rpc_stats.unreachable > 0 {
            return BucketEntryStateReason::Missing(BucketEntryStateMissingReason::Unreachable);
        }

        // [M2] the node has any per-sequence-ordering failed_to_send >= 0 (only possible if DEAD_FAILED_TO_SEND > 1)
        if per_sequence_ordering_stats
            .values()
            .any(|stats| stats.failed_to_send > 0)
        {
            return BucketEntryStateReason::Missing(BucketEntryStateMissingReason::FailedToSend);
        }

        // [M3] the node has any per-sequence-ordering recent_lost_questions > UNRELIABLE_LOST_QUESTIONS
        if per_sequence_ordering_stats
            .iter()
            .any(|(so, stats)| stats.recent_lost_questions >= missing_lost_questions_count(*so))
        {
            return BucketEntryStateReason::Missing(BucketEntryStateMissingReason::LostQuestions);
        }

        // ---===/ Initial /===---

        // [I1] for all sequence-orderings:
        //   - first_steady_answer_ts is None (cold start)
        if per_sequence_ordering_stats
            .values()
            .all(|stats| stats.first_steady_answer_ts.is_none())
        {
            return BucketEntryStateReason::Initial;
        }

        // ---===/ Unreliable /===---

        // [U1] for any sequence-ordering:
        //   - time_since(first_steady_answer_ts), minus offline time, < UNRELIABLE_ANSWER_SPAN
        if per_sequence_ordering_stats.values().any(|stats| {
            if let Some(ts) = stats.first_steady_answer_ts {
                let raw = cur_ts.duration_since(ts);
                // Within span by wall-clock → unreliable regardless; otherwise
                // subtract offline time so we don't grant untested reliability.
                raw < UNRELIABLE_ANSWER_SPAN
                    || raw.saturating_sub(offline_overlap_since(ts)) < UNRELIABLE_ANSWER_SPAN
            } else {
                false
            }
        }) {
            return BucketEntryStateReason::Unreliable;
        }

        // ---===/ Reliable /===---

        // [R1] Any other condition is reliable
        BucketEntryStateReason::Reliable
    }

    /// Compute and record the node-level state for stats accounting and return the full state reason
    pub fn state_reason(&self, cur_ts: Timestamp) -> BucketEntryStateReason {
        // Keep the stats lock over this operation so we don't have a race condition
        let mut state_stats_accounting = self.state_stats_accounting.lock();
        self.compute_and_log_state_reason_inner(cur_ts, &mut state_stats_accounting)
    }

    /// Get the node-level state without the reason for stats accounting
    pub fn state(&self, cur_ts: Timestamp) -> BucketEntryState {
        self.state_reason(cur_ts).into()
    }

    /// Utility function to compute and log a state reason change
    fn compute_and_log_state_reason_inner(
        &self,
        cur_ts: Timestamp,
        state_stats_accounting: &mut StateStatsAccounting,
    ) -> BucketEntryStateReason {
        let reason = self.compute_state_reason(cur_ts);
        if let Some(state_reason_change_span) =
            state_stats_accounting.record_state_reason(cur_ts, reason)
        {
            veilid_log!(self debug target: "rtab::state::node", "{:#}: {:#}", self.best_node_id(), state_reason_change_span);
        }
        reason
    }

    /// If a node is dead or missing, reset it to the 'initial' state to give it another chance.
    /// Called when we get a new peer info for a node so we can try it out.
    pub(super) fn revive(&mut self, cur_ts: Timestamp) {
        // Keep the stats lock over this operation so we don't have a race condition
        let mut state_stats_accounting = self.state_stats_accounting.lock();

        // Check if the node is dead or missing
        let dead_or_missing_reason = self.compute_state_reason(cur_ts);
        if matches!(
            dead_or_missing_reason,
            BucketEntryStateReason::Dead(_) | BucketEntryStateReason::Missing(_)
        ) {
            // Drop node-level stats so we can re-validate the node.
            self.rpc_stats.last_question_ts = None;
            self.rpc_stats.last_seen_ts = None;
            self.rpc_stats.first_steady_answer_ts = None;
            self.rpc_stats.first_steady_lost_question_ts = None;
            self.rpc_stats.failed_to_send = 0;
            self.rpc_stats.unreachable = 0;

            // Drop per-sequence-ordering stats so each sequence ordering re-validates.
            self.per_sequence_ordering_stats.clear();

            // Drop per-transport stats so each transport re-validates.
            self.per_transport_stats.clear();

            // Drop contact method failures so each contact method re-validates.
            self.contact_method_failures.clear();

            // Should be at the initial state now, or something is very wrong
            let alive_reason =
                self.compute_and_log_state_reason_inner(cur_ts, &mut state_stats_accounting);
            if !matches!(alive_reason, BucketEntryStateReason::Initial) {
                veilid_log!(self error "node was ({:?}) but is ({:?}) after reviving: {}", dead_or_missing_reason, alive_reason, self.best_node_id());
            }

            veilid_log!(self debug target: "rtab::state::node", "Node revived: {:#}", self.best_node_id());
        }
    }
}
