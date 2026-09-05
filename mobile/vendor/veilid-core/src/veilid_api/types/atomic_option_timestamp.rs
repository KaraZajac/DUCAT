use std::sync::atomic::AtomicU64;

use super::*;

/// Atomic wrapper for optional timestamps used in structs so we can avoid locks
/// for LRU and similar operations
#[derive(Debug, Default, Serialize, Deserialize, GetSize)]
#[serde(from = "Option<Timestamp>", into = "Option<Timestamp>")]
#[must_use]
pub struct AtomicOptionTimestamp(AtomicU64);

impl Clone for AtomicOptionTimestamp {
    fn clone(&self) -> Self {
        Self(AtomicU64::new(self.0.load(Ordering::Acquire)))
    }
}

impl fmt::Display for AtomicOptionTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.get()
                .map(|t| t.to_string())
                .unwrap_or("None".to_string())
        )
    }
}

impl AtomicOptionTimestamp {
    /// Make an atomic optional timestamp set to the given value.
    pub fn new(opt_ts: Option<Timestamp>) -> Self {
        Self(AtomicU64::new(opt_ts.map(|t| t.as_u64()).unwrap_or(0)))
    }

    /// Make an atomic optional timestamp set to none.
    pub fn none() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Make an atomic optional timestamp set to the current time.
    pub fn now() -> Self {
        Self::new(Some(Timestamp::now()))
    }

    /// Make an atomic optional timestamp set to the current non-decreasing time.
    pub fn now_non_decreasing() -> Self {
        Self::new(Some(Timestamp::now_non_decreasing()))
    }

    /// Make an atomic optional timestamp set to the current increasing time.
    pub fn now_increasing() -> Self {
        Self::new(Some(Timestamp::now_increasing()))
    }

    /// Load the optional timestamp.
    pub fn get(&self) -> Option<Timestamp> {
        let ts = self.0.load(Ordering::Acquire);
        if ts == 0 {
            None
        } else {
            Some(Timestamp::new(ts))
        }
    }

    /// Store the optional timestamp.
    pub fn set(&self, opt_t: Option<Timestamp>) {
        self.0
            .store(opt_t.map(|t| t.as_u64()).unwrap_or(0), Ordering::Release);
    }

    /// Store the optional timestamp and return the previous value.
    pub fn swap(&self, opt_t: Option<Timestamp>) -> Option<Timestamp> {
        let old_ts = self
            .0
            .swap(opt_t.map(|t| t.as_u64()).unwrap_or(0), Ordering::AcqRel);
        if old_ts == 0 {
            None
        } else {
            Some(Timestamp::new(old_ts))
        }
    }

    /// Fetch and update the optional timestamp atomically
    ///
    /// Parameters:
    /// - f: A function that takes the current optional timestamp and returns an optional new optional timestamp
    ///
    /// Returns:
    /// - The new optional timestamp if the update was successful, otherwise the current optional timestamp
    // fetch_update deprecated in newer std; move to try_update when MSRV >= 1.96
    #[allow(deprecated)]
    pub fn fetch_update(
        &self,
        mut f: impl FnMut(Option<Timestamp>) -> Option<Option<Timestamp>>,
    ) -> Result<Option<Timestamp>, Option<Timestamp>> {
        self.0
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                f(if v == 0 {
                    None
                } else {
                    Some(Timestamp::new(v))
                })
                .map(|opt_v| opt_v.map(|v| v.as_u64()).unwrap_or(0))
            })
            .map(|opt_v| {
                if opt_v == 0 {
                    None
                } else {
                    Some(Timestamp::new(opt_v))
                }
            })
            .map_err(|opt_v| {
                if opt_v == 0 {
                    None
                } else {
                    Some(Timestamp::new(opt_v))
                }
            })
    }

    /// Check if a timestamp has expired atomically
    ///
    /// Parameters:
    /// - cur_ts: The current timestamp
    /// - expiration: The expiration duration
    ///
    /// Returns:
    /// - true if the timestamp has expired, false if it has not
    pub fn expiration_state(
        &self,
        cur_ts: Timestamp,
        expiration: TimestampDuration,
    ) -> Option<ExpirationState> {
        let existing_ts = self.get()?;
        if cur_ts.duration_since(existing_ts) >= expiration {
            Some(ExpirationState::Dead)
        } else {
            Some(ExpirationState::Live)
        }
    }

    /// Check if a timestamp is expired and update it to the current time if it is not, atomically.
    /// Useful for LRU caching.
    ///
    /// Parameters:
    /// - cur_ts: The current timestamp
    /// - expiration: The expiration duration
    ///
    /// Returns:
    /// - true if the timestamp was updated, false if it was expired
    pub fn update_if_not_expired(
        &self,
        cur_ts: Timestamp,
        expiration: TimestampDuration,
    ) -> ExpirationState {
        let updated = self
            .fetch_update(move |opt_existing_ts| {
                if let Some(existing_ts) = opt_existing_ts {
                    if cur_ts.duration_since(existing_ts) >= expiration {
                        None
                    } else {
                        Some(Some(cur_ts))
                    }
                } else {
                    Some(Some(cur_ts))
                }
            })
            .is_ok();
        if updated {
            ExpirationState::Live
        } else {
            ExpirationState::Dead
        }
    }
}

impl From<Timestamp> for AtomicOptionTimestamp {
    fn from(t: Timestamp) -> Self {
        Self::new(Some(t))
    }
}

impl From<Option<Timestamp>> for AtomicOptionTimestamp {
    fn from(opt_t: Option<Timestamp>) -> Self {
        Self::new(opt_t)
    }
}

impl From<AtomicOptionTimestamp> for Option<Timestamp> {
    fn from(at: AtomicOptionTimestamp) -> Self {
        at.get()
    }
}

impl From<&AtomicOptionTimestamp> for Option<Timestamp> {
    fn from(at: &AtomicOptionTimestamp) -> Self {
        at.get()
    }
}
