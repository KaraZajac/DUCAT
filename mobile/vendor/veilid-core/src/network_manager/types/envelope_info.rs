use hashlink::LinkedHashMap;

use super::*;

/// Number of envelope timestamps per node id to keep in the log
/// to ensure we don't process an envelope twice due to UDP retransmission or anything like that
pub const ENVELOPE_TIMESTAMP_LOG_LENGTH: usize = 1024;

/// A remote and local timestamp record
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimestampOffset {
    pub local: Timestamp,
    pub remote: Timestamp,
}

impl fmt::Debug for TimestampOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (offset, is_behind) = self.remote_offset();
        f.debug_struct("TimestampRecord")
            .field("local", &self.local)
            .field("remote", &self.remote)
            .field("__offset", &format!("{:?}", offset))
            .field("__is_behind", &is_behind)
            .finish()
    }
}

impl TimestampOffset {
    pub fn new(local: Timestamp, remote: Timestamp) -> Self {
        Self { local, remote }
    }
    /// Adjusts remote timestamp to local offset
    pub fn adjust_remote(&self, remote_timestamp: Timestamp) -> Timestamp {
        if self.remote < self.local {
            let offset = self.local.duration_since(self.remote);
            remote_timestamp.later(offset)
        } else {
            let offset = self.remote.duration_since(self.local);
            remote_timestamp.earlier(offset)
        }
    }

    /// Gets the absolute offset of the remote timestamp
    /// Returns the timestamp offset and a bool that is true if
    /// the remote is behind the local timestamp, false otherwise
    pub fn remote_offset(&self) -> (TimestampDuration, bool) {
        if self.remote < self.local {
            let offset = self.local.duration_since(self.remote);
            (offset, true)
        } else {
            let offset = self.remote.duration_since(self.local);
            (offset, false)
        }
    }
}

/// Per-node envelope timestamp logging
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct EnvelopeInfo {
    /// Most recent calculated average timestamp offset
    pub timestamp_offset: TimestampOffset,
    /// Internal total offset for calculating average
    pub total_offset: i128,
    /// Timestamp log for envelopes (map from claimed timestamp to received timestamp)
    pub envelope_timestamp_log: LinkedHashMap<Timestamp, Timestamp>,
}

impl fmt::Debug for EnvelopeInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.sign_plus() {
            f.debug_struct("EnvelopeInfo")
                .field("timestamp_offset", &self.timestamp_offset)
                .field("envelope_timestamp_log", &self.envelope_timestamp_log)
                .finish()
        } else {
            let len = self.envelope_timestamp_log.len();
            if len > 0 {
                let back = self.envelope_timestamp_log.back().unwrap_or_log();
                let front = self.envelope_timestamp_log.front().unwrap_or_log();
                write!(
                    f,
                    "offset: {:?} log(len={}): {:?}->{:?} ... {:?}->{:?}",
                    self.timestamp_offset, len, front.0, front.1, back.0, back.1
                )
            } else {
                write!(f, "offset: {:?} log(len={})", self.timestamp_offset, len)
            }
        }
    }
}

impl fmt::Display for EnvelopeInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.envelope_timestamp_log.len();
        if len > 0 {
            let back = self.envelope_timestamp_log.back().unwrap_or_log();
            let front = self.envelope_timestamp_log.front().unwrap_or_log();
            write!(
                f,
                "offset: {:?} log(len={}): {}->{} ... {}->{}",
                self.timestamp_offset, len, front.0, front.1, back.0, back.1
            )
        } else {
            write!(f, "offset: {:?} log(len={})", self.timestamp_offset, len)
        }
    }
}

impl EnvelopeInfo {
    pub fn new(local_timestamp: Timestamp, remote_timestamp: Timestamp) -> Self {
        let mut envelope_timestamp_log = LinkedHashMap::new();
        envelope_timestamp_log.insert(remote_timestamp, local_timestamp);

        Self {
            timestamp_offset: TimestampOffset::new(local_timestamp, remote_timestamp),
            total_offset: (remote_timestamp.as_u64() as i128) - (local_timestamp.as_u64() as i128),
            envelope_timestamp_log,
        }
    }

    /// Add an envelope timestamp to the log
    pub fn add_timestamp(
        &mut self,
        local_timestamp: Timestamp,
        remote_timestamp: Timestamp,
        oldest_local_timestamp: Timestamp,
    ) {
        self.envelope_timestamp_log
            .insert(remote_timestamp, local_timestamp);
        self.total_offset +=
            (remote_timestamp.as_u64() as i128) - (local_timestamp.as_u64() as i128);

        // Trim log
        while self.envelope_timestamp_log.len() > ENVELOPE_TIMESTAMP_LOG_LENGTH
            || self
                .envelope_timestamp_log
                .front()
                .map(|tt| *tt.1 < oldest_local_timestamp)
                .unwrap_or(false)
        {
            let (rts, lts) = self.envelope_timestamp_log.pop_front().unwrap_or_log();
            self.total_offset -= (rts.as_u64() as i128) - (lts.as_u64() as i128);
        }

        // Update average offset
        let average_offset = self.total_offset / self.envelope_timestamp_log.len() as i128;
        let offset_cur_ts = if average_offset < 0 {
            local_timestamp.earlier(TimestampDuration::new((-average_offset) as u64))
        } else {
            local_timestamp.later(TimestampDuration::new(average_offset as u64))
        };

        self.timestamp_offset = TimestampOffset::new(local_timestamp, offset_cur_ts);
    }
}
