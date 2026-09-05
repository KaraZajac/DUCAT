//! A span of time measured in microseconds.

use super::*;

aligned_u64_type!(TimestampDuration);
aligned_u64_type_default_debug_impl!(TimestampDuration);

impl fmt::Display for TimestampDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "{}", human_duration(self.as_u64()))
        } else {
            write!(f, "{}", self.as_u64())
        }
    }
}

impl FromStr for TimestampDuration {
    type Err = <u64 as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(TimestampDuration(u64::from_str(s)?))
    }
}

impl TimestampDuration {
    /// Make a duration from a number of seconds.
    pub const fn new_secs(secs: u32) -> Self {
        TimestampDuration::new(secs as u64 * 1_000_000u64)
    }
    /// Make a duration from a number of milliseconds.
    pub const fn new_ms(ms: u64) -> Self {
        TimestampDuration::new(ms * 1_000u64)
    }

    /// Returns true if the duration is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Elapsed time from `older` to now, saturating to zero if `older` is in the future.
    pub fn since(older: Timestamp) -> Self {
        Self::new(Timestamp::now().as_u64().saturating_sub(older.as_u64()))
    }

    /// Elapsed time from `older` to a non-decreasing now, saturating to zero if `older` is in the future.
    pub fn since_non_decreasing(older: Timestamp) -> Self {
        Self::new(
            Timestamp::now_non_decreasing()
                .as_u64()
                .saturating_sub(older.as_u64()),
        )
    }

    /// Whole seconds as a `u32`, erroring if the value overflows.
    pub fn seconds_u32(&self) -> Result<u32, String> {
        u32::try_from(self.as_u64() / 1_000_000u64)
            .map_err(|e| format!("could not convert to seconds: {}", e))
    }

    /// Whole milliseconds as a `u32`, erroring if the value overflows.
    pub fn millis_u32(&self) -> Result<u32, String> {
        u32::try_from(self.as_u64() / 1_000u64)
            .map_err(|e| format!("could not convert to milliseconds: {}", e))
    }

    /// Seconds as an `f64`, trading off least significant bits for range.
    #[must_use]
    pub fn seconds_f64(&self) -> f64 {
        // Downshift precision until it fits, lose least significant bits
        let mut mul: f64 = 1.0f64 / 1_000_000.0f64;
        let mut usec = self.0;
        while usec > (u32::MAX as u64) {
            usec >>= 1;
            mul *= 2.0f64;
        }
        f64::from(usec as u32) * mul
    }

    /// Add two durations, saturating at the maximum value.
    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self::new(self.0.saturating_add(rhs.0))
    }

    /// Subtract a duration, saturating at zero.
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self::new(self.0.saturating_sub(rhs.0))
    }

    /// Multiply by a scalar, saturating at the maximum value.
    pub const fn saturating_mul(self, rhs: u64) -> Self {
        Self::new(self.0.saturating_mul(rhs))
    }

    /// Multiply by an f64 scaling factor (not const due to float arithmetic)
    pub fn f64_mul(self, rhs: f64) -> Self {
        Self::new((self.0 as f64 * rhs) as u64)
    }

    /// Divide by a scalar.
    pub const fn div(self, rhs: u64) -> Self {
        Self::new(self.0 / rhs)
    }

    /// Divide by a scalar in place.
    pub const fn div_assign(&mut self, rhs: u64) {
        *self = self.div(rhs)
    }

    /// Divide by a scalar, returning `None` on divide by zero.
    pub fn checked_div<T: Into<u64>>(self, rhs: T) -> Option<Self> {
        self.0.checked_div(rhs.into()).map(Self::new)
    }

    /// Multiply by a scalar, returning `None` on overflow.
    pub fn checked_mul<T: Into<u64>>(self, rhs: T) -> Option<Self> {
        self.0.checked_mul(rhs.into()).map(Self::new)
    }

    /// Add a duration in place, saturating at the maximum value.
    pub const fn saturating_add_assign(&mut self, rhs: Self) {
        *self = self.saturating_add(rhs);
    }
    /// Subtract a duration in place, saturating at zero.
    pub const fn saturating_sub_assign(&mut self, rhs: Self) {
        *self = self.saturating_sub(rhs);
    }
    /// Multiply by a scalar in place, saturating at the maximum value.
    pub const fn saturating_mul_assign(&mut self, rhs: u64) {
        *self = self.saturating_mul(rhs);
    }

    /// Wrap a future with a timeout based on this duration.
    /// Saturates to u32::MAX milliseconds (~49.7 days) on overflow.
    pub fn timeout<F, T>(self, f: F) -> impl Future<Output = Result<T, TimeoutError>>
    where
        F: Future<Output = T>,
    {
        veilid_tools::timeout(self.millis_u32().unwrap_or(u32::MAX), f)
    }

    /// Run a periodic callback at this duration's interval.
    /// Saturates to u32::MAX milliseconds (~49.7 days) on overflow.
    pub fn interval<F, FUT>(
        self,
        name: &str,
        immediate: bool,
        callback: F,
    ) -> PinBoxFutureStatic<()>
    where
        F: Fn() -> FUT + Send + Sync + 'static,
        FUT: Future<Output = ()> + Send,
    {
        veilid_tools::interval(
            name,
            self.millis_u32().unwrap_or(u32::MAX),
            immediate,
            callback,
        )
    }
}

/// Future extension trait that mirrors `TimestampDuration::timeout` for ergonomic chaining.
/// Named `timeout_duration` to avoid conflict with `futures_time::FutureExt::timeout`.
pub trait TimestampDurationTimeoutFutureExt: Future + Sized {
    /// Wrap this future with a timeout of the given duration.
    fn timeout_duration(
        self,
        duration: TimestampDuration,
    ) -> impl Future<Output = Result<Self::Output, TimeoutError>> {
        duration.timeout(self)
    }
}

impl<F: Future + Sized> TimestampDurationTimeoutFutureExt for F {}
