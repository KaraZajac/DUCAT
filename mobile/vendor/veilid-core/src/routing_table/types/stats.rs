use super::*;

/// Measurement of what states the node has been in over a time span
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateStats {
    /// total amount of time measured
    pub span: TimestampDuration,
    /// amount of time spent in each state
    pub per_state: BTreeMap<BucketEntryState, TimestampDuration>,
    /// state reason stats for this peer
    pub reason: StateReasonStats,
}

impl fmt::Display for StateStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "span: {}", f.to_string(self.span))?;
        for (state, duration) in self.per_state.iter() {
            writeln!(f, "{}: {}", f.to_string(state), f.to_string(duration))?;
        }
        write!(
            f,
            "reason:\n{}",
            indent_all_string(f.to_string(&self.reason))
        )?;
        Ok(())
    }
}

/// Measurement of what state reasons the node has been in over a time span
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateReasonStats {
    /// time spent dead due to being unreachable
    pub excessive_unreachable: TimestampDuration,
    /// time spent dead due to being unable to send
    pub excessive_send_failures: TimestampDuration,
    /// time spent dead because of too many lost questions
    pub never_seen_lost_questions: TimestampDuration,
    /// time spent dead because of no ping response
    pub steady_lost_questions: TimestampDuration,
    /// time spent missing because of being unreachable
    pub unreachable: TimestampDuration,
    /// time spent missing because of failures to send
    pub failed_to_send: TimestampDuration,
    /// time spent missing because of lost questions
    pub lost_questions: TimestampDuration,
    /// time spent in initial state because of not seeing answers steadily
    pub no_answer_steadily: TimestampDuration,
    /// time spent unreliable because we are in the unreliable answer span
    pub unreliable_answer: TimestampDuration,
}

impl fmt::Display for StateReasonStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "(dead) excessive_unreachable:     {}",
            f.to_string(self.excessive_unreachable)
        )?;
        writeln!(
            f,
            "(dead) excessive_send_failures:   {}",
            f.to_string(self.excessive_send_failures)
        )?;
        writeln!(
            f,
            "(dead) never_seen_lost_questions: {}",
            f.to_string(self.never_seen_lost_questions)
        )?;
        writeln!(
            f,
            "(dead) steady_lost_questions:     {}",
            f.to_string(self.steady_lost_questions)
        )?;
        writeln!(
            f, //
            "(miss) unreachable:               {}",
            f.to_string(self.unreachable)
        )?;
        writeln!(
            f,
            "(miss) failed_to_send:            {}",
            f.to_string(self.failed_to_send)
        )?;
        writeln!(
            f,
            "(miss) lost_questions:            {}",
            f.to_string(self.lost_questions)
        )?;
        writeln!(
            f,
            "(init) no_answer_steadily:        {}",
            f.to_string(self.no_answer_steadily)
        )?;
        writeln!(
            f,
            "(urel) unreliable_answer:         {}",
            f.to_string(self.unreliable_answer)
        )?;

        Ok(())
    }
}

/// Measurement of round-trip RPC question/answer performance
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerStats {
    /// total amount of time measured
    pub span: TimestampDuration,
    /// number of questions sent in this span
    pub questions: u32,
    /// number of answers received in this span
    pub answers: u32,
    /// number of lost questions in this span
    pub lost_questions: u32,
    /// maximum number of received answers before a lost question in this span
    pub steady_answers_maximum: u32,
    /// average number of received answers before a lost question in this span
    pub steady_answers_average: u32,
    /// minimum number of received answers before a lost question in this span
    pub steady_answers_minimum: u32,
    /// maximum number of timeouts before a received answer in this span
    pub steady_lost_questions_maximum: u32,
    /// average number of timeouts before a received answer in this span
    pub steady_lost_questions_average: u32,
    /// minimum number of timeouts before a received answer in this span
    pub steady_lost_questions_minimum: u32,
}

impl fmt::Display for AnswerStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "span: {}", self.span)?;
        writeln!(
            f,
            "questions/answers/lost: {} / {} / {}",
            self.questions, self.answers, self.lost_questions
        )?;
        writeln!(
            f,
            "consecutive answers min/avg/max: {} / {} / {}",
            self.steady_answers_minimum, self.steady_answers_average, self.steady_answers_maximum
        )?;
        writeln!(
            f,
            "consecutive lost min/avg/max: {} / {} / {}",
            self.steady_lost_questions_minimum,
            self.steady_lost_questions_average,
            self.steady_lost_questions_maximum
        )?;

        Ok(())
    }
}

impl AnswerStats {
    /// Approximate aggregation for projecting per-transport stats into a
    /// coarser view: sum counts, max/min bounds, average the averages.
    #[expect(dead_code)]
    pub fn merge(&mut self, other: &AnswerStats) {
        if self.questions == 0 && self.answers == 0 && self.lost_questions == 0 {
            *self = other.clone();
            return;
        }
        self.span = self.span.max(other.span);
        self.questions += other.questions;
        self.answers += other.answers;
        self.lost_questions += other.lost_questions;
        self.steady_answers_maximum = self
            .steady_answers_maximum
            .max(other.steady_answers_maximum);
        self.steady_answers_minimum = self
            .steady_answers_minimum
            .min(other.steady_answers_minimum);
        self.steady_answers_average =
            (self.steady_answers_average + other.steady_answers_average) / 2;
        self.steady_lost_questions_maximum = self
            .steady_lost_questions_maximum
            .max(other.steady_lost_questions_maximum);
        self.steady_lost_questions_minimum = self
            .steady_lost_questions_minimum
            .min(other.steady_lost_questions_minimum);
        self.steady_lost_questions_average =
            (self.steady_lost_questions_average + other.steady_lost_questions_average) / 2;
    }
}

/// Statistics for connection oriented protocols
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionStats {
    /// How long protected connections to this peer survived before being closed
    /// by the remote side; longer is better.
    /// None = no remote-closed protected drops yet observed.
    pub protected_drop_span: Option<LatencyStats>,
}

impl fmt::Display for ConnectionStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "protected drop span: {}",
            f.to_string_opt(self.protected_drop_span.as_ref())
        )?;
        Ok(())
    }
}

/// Statistics for RPC operations performed on a node
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RPCStats {
    /// number of rpcs that have been sent in the total entry time range
    pub messages_sent: u32,
    /// number of rpcs that have been received in the total entry time range
    pub messages_rcvd: u32,
    /// number of questions that never received an answer since the last received answer
    pub recent_lost_questions: u32,
    /// when the peer was last questioned (either successfully or not) and we wanted an answer
    pub last_question_ts: Option<Timestamp>,
    /// when the peer was last seen for any reason, including when we first attempted to reach out to it
    pub last_seen_ts: Option<Timestamp>,
    /// the timestamp of the first consecutively seen answer for this node
    pub first_steady_answer_ts: Option<Timestamp>,
    /// the timestamp of the first consecutively lost question for this node
    pub first_steady_lost_question_ts: Option<Timestamp>,
    /// number of messages that have failed to send or connections dropped since we last successfully sent one
    pub failed_to_send: u32,
    /// number of attempts where no transport could be chosen (no routing domain, no peer info, no contact method)
    pub unreachable: u32,
}

impl fmt::Display for RPCStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            " # sent/rcvd/flight: {} / {}",
            self.messages_sent, self.messages_rcvd
        )?;
        writeln!(
            f,
            "      last question: {}",
            f.to_string_opt(self.last_question_ts.as_ref())
        )?;
        writeln!(
            f,
            "          last seen: {}",
            f.to_string_opt(self.last_seen_ts.as_ref())
        )?;
        writeln!(
            f,
            "first steady answer: {}",
            f.to_string_opt(self.first_steady_answer_ts.as_ref())
        )?;
        writeln!(
            f,
            "first steady lost q: {}",
            f.to_string_opt(self.first_steady_lost_question_ts.as_ref())
        )?;
        writeln!(
            f,
            //
            "   # lost questions: {}",
            self.recent_lost_questions
        )?;
        Ok(())
    }
}

impl RPCStats {
    pub fn question_sent(&mut self, ts: Timestamp, expects_answer: bool) {
        self.messages_sent += 1;
        self.failed_to_send = 0;
        self.unreachable = 0;
        if expects_answer {
            self.last_question_ts = Some(ts);
        }
    }

    pub fn question_rcvd(&mut self, ts: Timestamp) {
        self.messages_rcvd += 1;
        self.last_seen_ts = Some(ts);
    }

    pub fn answer_sent(&mut self) {
        self.messages_sent += 1;
        self.failed_to_send = 0;
        self.unreachable = 0;
    }

    pub fn answer_rcvd(&mut self, recv_ts: Timestamp) {
        self.messages_rcvd += 1;
        self.last_seen_ts = Some(recv_ts);
        if self.first_steady_answer_ts.is_none() {
            self.first_steady_answer_ts = Some(recv_ts);
        }
        self.first_steady_lost_question_ts = None;
        self.recent_lost_questions = 0;
    }

    pub fn lost_question(&mut self, lost_ts: Timestamp) {
        self.first_steady_answer_ts = None;
        if self.first_steady_lost_question_ts.is_none() {
            self.first_steady_lost_question_ts = Some(lost_ts);
        }
        self.recent_lost_questions += 1;
    }

    pub fn failed_to_send(&mut self, fail_ts: Timestamp, expects_answer: bool) {
        if expects_answer {
            self.last_question_ts = Some(fail_ts);
        }
        self.failed_to_send += 1;
        self.unreachable = 0;
    }

    pub fn unreachable(&mut self) {
        self.unreachable += 1;
        self.first_steady_answer_ts = None;
    }
}
