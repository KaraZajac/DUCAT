//! Monotonically non-decreasing and monotonically increasing microseconds-since epoch timestamp

use super::*;

aligned_u64_type!(Timestamp);
aligned_u64_type_default_debug_impl!(Timestamp);

static LAST_NON_DECREASING_RAW_TIMESTAMP: Mutex<u64> = Mutex::new(0u64);
static LAST_INCREASING_RAW_TIMESTAMP: Mutex<u64> = Mutex::new(0u64);

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "{}", human_timestamp(self.as_u64()))
        } else {
            write!(f, "{}", self.as_u64())
        }
    }
}

impl FromStr for Timestamp {
    type Err = <u64 as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Timestamp(u64::from_str(s)?))
    }
}

impl Timestamp {
    /// Current wall-clock time. May move backward if the system clock is adjusted.
    pub fn now() -> Self {
        Self::new(get_raw_timestamp())
    }

    /// Returns true if the timestamp is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Current time, clamped to never read earlier than a previous call. Repeated values are allowed.
    pub fn now_non_decreasing() -> Self {
        let mut last_raw_timestamp = LAST_NON_DECREASING_RAW_TIMESTAMP.lock();
        let mut next_raw_timestamp = get_raw_timestamp();
        // Never allow time to go backward
        if next_raw_timestamp <= *last_raw_timestamp {
            next_raw_timestamp = *last_raw_timestamp;
        } else {
            *last_raw_timestamp = next_raw_timestamp;
        }
        Self::new(next_raw_timestamp)
    }

    /// Current time, clamped to always read at least 1us later than a previous call. Never repeats a value.
    pub fn now_increasing() -> Self {
        let mut last_raw_timestamp = LAST_INCREASING_RAW_TIMESTAMP.lock();
        let mut next_raw_timestamp = get_raw_timestamp();
        // Never allow time to go backward
        if next_raw_timestamp <= *last_raw_timestamp {
            // Never issue the same timestamp twice
            // It is okay for this not to be perfectly accurate
            // 1us between calls is completely reasonable
            next_raw_timestamp = *last_raw_timestamp + 1u64;
        }
        *last_raw_timestamp = next_raw_timestamp;
        Self::new(next_raw_timestamp)
    }

    /// Timestamp shifted later by a duration, saturating at the maximum value.
    pub fn later(self, rhs: TimestampDuration) -> Self {
        Self::new(self.0.saturating_add(rhs.as_u64()))
    }

    /// Timestamp shifted earlier by a duration, saturating at zero.
    pub fn earlier(self, rhs: TimestampDuration) -> Self {
        Self::new(self.0.saturating_sub(rhs.as_u64()))
    }

    /// Duration from `older` to this timestamp, saturating to zero if `older` is later.
    pub fn duration_since(self, older: Self) -> TimestampDuration {
        TimestampDuration::new(self.0.saturating_sub(older.0))
    }
}
