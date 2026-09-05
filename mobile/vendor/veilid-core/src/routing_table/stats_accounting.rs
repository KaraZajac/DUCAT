use super::*;

// State entry is per state reason change

/// Size is number of entries for the rolling state reason spans
const ROLLING_STATE_REASON_SPAN_SIZE: usize = 32;
/// Interval is number of seconds in each state reason span entry
pub const UPDATE_STATE_STATS_INTERVAL_SECS: u32 = 1;

// Answer entries are in counts per interval

/// Size is number of rolling answers entries
const ROLLING_ANSWERS_SIZE: usize = 10;
/// Interval is number of seconds in each rolling answer entry
pub const ROLLING_ANSWER_INTERVAL_SECS: u32 = 60;

/// Span of time for a node when it had a particular state reason
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateReasonSpan {
    /// The state reason for the span
    state_reason: BucketEntryStateReason,
    /// The timestamp when the span started
    enter_ts: Timestamp,
}

/// A change in the state reason for a node returned when the state reason changes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateReasonSpanChange {
    /// The previous state reason span if there was one
    old_span: Option<StateReasonSpan>,
    /// The new stat reason span
    new_span: StateReasonSpan,
}

impl fmt::Display for StateReasonSpanChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(old_span) = self.old_span {
            write!(
                f,
                "from {} to {} in {}",
                f.to_string(old_span.state_reason),
                f.to_string(self.new_span.state_reason),
                f.to_string(self.new_span.enter_ts.duration_since(old_span.enter_ts)),
            )
        } else {
            write!(
                f,
                "created {} at {}",
                f.to_string(self.new_span.state_reason),
                f.to_string(self.new_span.enter_ts),
            )
        }
    }
}

/// Accounting for the state and reason statistics for a node
#[derive(Debug, Clone, Default)]
pub struct StateStatsAccounting {
    rolling_state_reason_spans: VecDeque<StateReasonSpan>,
    last_stats: Option<StateStats>,
}

impl StateStatsAccounting {
    pub fn new() -> Self {
        Self::default()
    }

    fn make_stats(&self, cur_ts: Timestamp) -> StateStats {
        let mut ss = StateStats::default();
        let srs = &mut ss.reason;

        let mut last_ts = cur_ts;
        for rss in self.rolling_state_reason_spans.iter().rev() {
            let span_dur = last_ts.duration_since(rss.enter_ts);

            let state = BucketEntryState::from(rss.state_reason);
            ss.per_state
                .entry(state)
                .and_modify(|d| d.saturating_add_assign(span_dur))
                .or_insert(span_dur);
            match rss.state_reason {
                BucketEntryStateReason::Punished(_) => {
                    // Ignore punished nodes for now
                }
                BucketEntryStateReason::Dead(bucket_entry_dead_reason) => {
                    match bucket_entry_dead_reason {
                        BucketEntryStateDeadReason::ExcessiveUnreachable => {
                            srs.excessive_unreachable.saturating_add_assign(span_dur)
                        }
                        BucketEntryStateDeadReason::ExcessiveSendFailures => {
                            srs.excessive_send_failures.saturating_add_assign(span_dur)
                        }
                        BucketEntryStateDeadReason::NeverSeenLostQuestions => srs
                            .never_seen_lost_questions
                            .saturating_add_assign(span_dur),
                        BucketEntryStateDeadReason::SteadyLostQuestions => {
                            srs.steady_lost_questions.saturating_add_assign(span_dur)
                        }
                    }
                }
                BucketEntryStateReason::Missing(bucket_entry_missing_reason) => {
                    match bucket_entry_missing_reason {
                        BucketEntryStateMissingReason::Unreachable => {
                            srs.unreachable.saturating_add_assign(span_dur)
                        }
                        BucketEntryStateMissingReason::FailedToSend => {
                            srs.failed_to_send.saturating_add_assign(span_dur)
                        }
                        BucketEntryStateMissingReason::LostQuestions => {
                            srs.lost_questions.saturating_add_assign(span_dur)
                        }
                    }
                }
                BucketEntryStateReason::Initial => {
                    srs.no_answer_steadily.saturating_add_assign(span_dur)
                }
                BucketEntryStateReason::Unreliable => {
                    srs.unreliable_answer.saturating_add_assign(span_dur)
                }
                BucketEntryStateReason::Reliable => {
                    // Reliable nodes don't have a reason other than lack of unreliability
                }
            }

            last_ts = rss.enter_ts;
        }
        ss.span = cur_ts.duration_since(last_ts);
        ss
    }

    pub fn take_stats(&mut self) -> Option<StateStats> {
        self.last_stats.take()
    }

    /// Record the state reason and returns a span change structure if the state reason changed.
    pub fn record_state_reason(
        &mut self,
        cur_ts: Timestamp,
        state_reason: BucketEntryStateReason,
    ) -> Option<StateReasonSpanChange> {
        let mut old_span = None;

        let new_span = if let Some(cur_span) = self.rolling_state_reason_spans.back() {
            if state_reason != cur_span.state_reason {
                old_span = Some(*cur_span);

                while self.rolling_state_reason_spans.len() >= ROLLING_STATE_REASON_SPAN_SIZE {
                    self.rolling_state_reason_spans.pop_front();
                }

                true
            } else {
                false
            }
        } else {
            true
        };
        if new_span {
            let new_span = StateReasonSpan {
                state_reason,
                enter_ts: cur_ts,
            };

            self.last_stats = Some(self.make_stats(cur_ts));
            self.rolling_state_reason_spans.push_back(new_span);

            Some(StateReasonSpanChange { old_span, new_span })
        } else {
            None
        }
    }
}

/// Statistics about RPC answers for a node over a span of time
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnswerSpan {
    enter_ts: Timestamp,
    questions: u32,
    answers: u32,
    lost_questions: u32,
    current_steady_answers: u32,
    current_steady_lost_questions: u32,
    steady_answers_maximum: u32,
    steady_answers_total: u32,
    steady_answers_count: u32,
    steady_answers_minimum: u32,
    steady_lost_questions_maximum: u32,
    steady_lost_questions_total: u32,
    steady_lost_questions_count: u32,
    steady_lost_questions_minimum: u32,
}

impl AnswerSpan {
    pub fn new(cur_ts: Timestamp) -> Self {
        AnswerSpan {
            enter_ts: cur_ts,
            questions: 0,
            answers: 0,
            lost_questions: 0,
            current_steady_answers: 0,
            current_steady_lost_questions: 0,
            steady_answers_maximum: 0,
            steady_answers_total: 0,
            steady_answers_count: 0,
            steady_answers_minimum: 0,
            steady_lost_questions_maximum: 0,
            steady_lost_questions_total: 0,
            steady_lost_questions_count: 0,
            steady_lost_questions_minimum: 0,
        }
    }
}

/// Accounting for the answer statistics for a node
#[derive(Debug, Clone, Default)]
pub struct AnswerStatsAccounting {
    rolling_answer_spans: VecDeque<AnswerSpan>,
}

impl AnswerStatsAccounting {
    fn current_span(&mut self, cur_ts: Timestamp) -> &mut AnswerSpan {
        if self.rolling_answer_spans.is_empty() {
            self.rolling_answer_spans.push_back(AnswerSpan::new(cur_ts));
        }
        self.rolling_answer_spans.front_mut().unwrap_or_log()
    }

    fn make_stats(&self, cur_ts: Timestamp) -> AnswerStats {
        let mut questions = 0u32;
        let mut answers = 0u32;
        let mut lost_questions = 0u32;
        let mut steady_answers_maximum = 0u32;
        let mut steady_answers_average = 0u32;
        let mut steady_answers_minimum = u32::MAX;
        let mut steady_lost_questions_maximum = 0u32;
        let mut steady_lost_questions_average = 0u32;
        let mut steady_lost_questions_minimum = u32::MAX;

        let mut last_ts = cur_ts;
        for ras in self.rolling_answer_spans.iter().rev() {
            questions += ras.questions;
            answers += ras.answers;
            lost_questions += ras.lost_questions;

            steady_answers_maximum.max_assign(ras.steady_answers_maximum);
            steady_answers_minimum.min_assign(ras.steady_answers_minimum);
            steady_answers_average += ras
                .steady_answers_total
                .checked_div(ras.steady_answers_count)
                .unwrap_or(0);

            steady_lost_questions_maximum.max_assign(ras.steady_lost_questions_maximum);
            steady_lost_questions_minimum.min_assign(ras.steady_lost_questions_minimum);
            steady_lost_questions_average += ras
                .steady_lost_questions_total
                .checked_div(ras.steady_lost_questions_count)
                .unwrap_or(0);

            last_ts = ras.enter_ts;
        }

        let len = self.rolling_answer_spans.len() as u32;
        steady_answers_average = steady_answers_average.checked_div(len).unwrap_or(0);
        steady_lost_questions_average = steady_lost_questions_average.checked_div(len).unwrap_or(0);

        let span = cur_ts.duration_since(last_ts);

        AnswerStats {
            span,
            questions,
            answers,
            lost_questions,
            steady_answers_maximum,
            steady_answers_average,
            steady_answers_minimum,
            steady_lost_questions_maximum,
            steady_lost_questions_average,
            steady_lost_questions_minimum,
        }
    }

    pub fn roll_answers(&mut self, cur_ts: Timestamp) -> AnswerStats {
        let stats = self.make_stats(cur_ts);

        while self.rolling_answer_spans.len() >= ROLLING_ANSWERS_SIZE {
            self.rolling_answer_spans.pop_front();
        }
        self.rolling_answer_spans.push_back(AnswerSpan::new(cur_ts));

        stats
    }

    pub fn record_question(&mut self, cur_ts: Timestamp) {
        let cas = self.current_span(cur_ts);
        cas.questions += 1;
    }
    pub fn record_answer(&mut self, cur_ts: Timestamp) {
        let cas = self.current_span(cur_ts);
        cas.answers += 1;
        if cas.current_steady_lost_questions > 0 {
            cas.steady_lost_questions_maximum
                .max_assign(cas.current_steady_lost_questions);
            cas.steady_lost_questions_minimum
                .min_assign(cas.current_steady_lost_questions);
            cas.steady_lost_questions_total += cas.current_steady_lost_questions;
            cas.steady_lost_questions_count += 1;
            cas.current_steady_lost_questions = 0;
        }
        cas.current_steady_answers = 1;
    }
    pub fn record_lost_question(&mut self, cur_ts: Timestamp) {
        let cas = self.current_span(cur_ts);
        cas.lost_questions += 1;
        if cas.current_steady_answers > 0 {
            cas.steady_answers_maximum
                .max_assign(cas.current_steady_answers);
            cas.steady_answers_minimum
                .min_assign(cas.current_steady_answers);
            cas.steady_answers_total += cas.current_steady_answers;
            cas.steady_answers_count += 1;
            cas.current_steady_answers = 0;
        }
        cas.current_steady_lost_questions = 1;
    }
}
