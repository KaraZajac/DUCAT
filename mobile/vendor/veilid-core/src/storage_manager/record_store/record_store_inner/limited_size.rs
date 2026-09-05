use super::*;
use num_traits::{PrimInt, Unsigned};

impl_veilid_log_facility!("stor");

#[derive(ThisError, Debug, Clone, Eq, PartialEq)]
pub enum ConcurrencyError {
    #[error("ConcurrentAccess({description})")]
    ConcurrentAccess { description: String },
}
impl From<ConcurrencyError> for VeilidAPIError {
    fn from(value: ConcurrencyError) -> Self {
        Self::internal(value.to_string())
    }
}

#[derive(ThisError, Debug, Clone, Copy, Eq, PartialEq)]
pub enum LimitError<T>
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    #[error("OverLimit({value}>{limit})")]
    OverLimit { value: T, limit: T },
    #[error("BelowZero(-{value})")]
    BelowZero { value: T },
}
impl<T> From<LimitError<T>> for VeilidAPIError
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    fn from(value: LimitError<T>) -> Self {
        Self::internal(value.to_string())
    }
}

#[derive(ThisError, Debug, Clone, Copy, Eq, PartialEq)]
pub enum NumericError<T: PrimInt + Unsigned + fmt::Display + fmt::Debug> {
    #[error("Overflow(current={current},added={added})")]
    Overflow { current: T, added: T },
    #[error("Underflow(current={current},removed={removed})")]
    Underflow { current: T, removed: T },
}
impl<T> From<NumericError<T>> for VeilidAPIError
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    fn from(value: NumericError<T>) -> Self {
        Self::internal(value.to_string())
    }
}

struct LimitedSizeUnlockedInner<T: PrimInt + Unsigned + fmt::Display + fmt::Debug> {
    registry: VeilidComponentRegistry,
    description: String,
    opt_limit: Option<T>,
}

impl<T: PrimInt + Unsigned + fmt::Display + fmt::Debug> fmt::Debug for LimitedSizeUnlockedInner<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LimitedSizeUnlockedInner")
            //.field("registry", &self.registry)
            .field("description", &self.description)
            .field("opt_limit", &self.opt_limit)
            .finish()
    }
}

#[derive(Debug)]
struct LimitedSizeInner<T: PrimInt + Unsigned + fmt::Display + fmt::Debug> {
    value: T,
}

#[derive(Clone)]
pub struct LimitedSize<T: PrimInt + Unsigned + fmt::Display + fmt::Debug> {
    unlocked_inner: Arc<LimitedSizeUnlockedInner<T>>,
    inner: Arc<Mutex<LimitedSizeInner<T>>>,
}

impl<T> VeilidComponentRegistryAccessor for LimitedSize<T>
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    fn registry(&self) -> VeilidComponentRegistry {
        self.unlocked_inner.registry.clone()
    }
}

impl<T> fmt::Debug for LimitedSize<T>
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LimitedSize")
            .field("description", &self.unlocked_inner.description)
            .field("opt_limit", &self.unlocked_inner.opt_limit)
            .field("inner", &self.inner)
            .finish()
    }
}

impl<T> fmt::Display for LimitedSize<T>
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.debug_value();

        if f.alternate() {
            write!(f, "{}", value)
        } else {
            write!(
                f,
                "[{}]:{}{}",
                &self.unlocked_inner.description,
                value,
                if let Some(limit) = self.unlocked_inner.opt_limit {
                    format!("/{}", limit)
                } else {
                    "".to_string()
                },
            )
        }
    }
}

impl<T: PrimInt + Unsigned + fmt::Display + fmt::Debug> LimitedSize<T> {
    pub fn try_new(
        registry: VeilidComponentRegistry,
        description: &str,
        opt_limit: Option<T>,
        value: T,
    ) -> Result<Self, LimitError<T>> {
        if let Some(limit) = opt_limit {
            if value > limit {
                return Err(LimitError::OverLimit { value, limit });
            }
        }

        Ok(Self {
            unlocked_inner: Arc::new(LimitedSizeUnlockedInner {
                registry,
                description: description.to_owned(),
                opt_limit,
            }),
            inner: Arc::new(Mutex::new(LimitedSizeInner { value })),
        })
    }

    pub fn limit(&self) -> Option<T> {
        self.unlocked_inner.opt_limit
    }

    pub fn with_value<R, F: FnOnce(T) -> R>(&self, closure: F) -> Result<R, ConcurrencyError> {
        let Some(inner) = self.inner.try_lock_arc() else {
            return Err(ConcurrencyError::ConcurrentAccess {
                description: format!(
                    "Concurrent attempt to access LimitedSize({})",
                    self.unlocked_inner.description
                ),
            });
        };
        Ok(closure(inner.value))
    }

    pub fn debug_value(&self) -> T {
        unsafe { (*self.inner.data_ptr()).value }
    }

    pub fn modify(&self) -> Result<LimitedSizeGuard<T>, ConcurrencyError> {
        let Some(inner) = self.inner.try_lock_arc() else {
            return Err(ConcurrencyError::ConcurrentAccess {
                description: format!(
                    "Concurrent attempt to modify LimitedSize({})",
                    self.unlocked_inner.description
                ),
            });
        };

        Ok(LimitedSizeGuard::new(self.unlocked_inner.clone(), inner))
    }
}

pub struct LimitedSizeGuard<T>
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    unlocked_inner: Arc<LimitedSizeUnlockedInner<T>>,
    inner: ArcMutexGuard<RawMutex, LimitedSizeInner<T>>,
    uncommitted_value: Option<T>,
    positive: bool,
}

impl<T> fmt::Debug for LimitedSizeGuard<T>
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LimitedSizeGuard")
            .field("unlocked_inner", &self.unlocked_inner)
            .field("inner", &*self.inner)
            .field("uncommitted_value", &self.uncommitted_value)
            .field("positive", &self.positive)
            .finish()
    }
}

impl<T> VeilidComponentRegistryAccessor for LimitedSizeGuard<T>
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    fn registry(&self) -> VeilidComponentRegistry {
        self.unlocked_inner.registry.clone()
    }
}

impl<T> LimitedSizeGuard<T>
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    fn new(
        unlocked_inner: Arc<LimitedSizeUnlockedInner<T>>,
        inner: ArcMutexGuard<RawMutex, LimitedSizeInner<T>>,
    ) -> Self {
        Self {
            uncommitted_value: Some(inner.value),
            unlocked_inner,
            inner,
            positive: true,
        }
    }

    pub fn set(&mut self, new_value: T) {
        self.uncommitted_value = Some(new_value);
        self.positive = true;
    }

    pub fn add(&mut self, v: T) -> Result<T, NumericError<T>> {
        let uncommitted_value = self.uncommitted_value.as_mut().unwrap_or_log();

        if !self.positive {
            let new_value = if v < *uncommitted_value {
                *uncommitted_value - v
            } else {
                self.positive = true;
                v - *uncommitted_value
            };

            *uncommitted_value = new_value;
            return Ok(new_value);
        }

        let max_v = T::max_value() - *uncommitted_value;
        if v > max_v {
            return Err(NumericError::Overflow {
                current: *uncommitted_value,
                added: v,
            });
        }
        let new_value = *uncommitted_value + v;
        *uncommitted_value = new_value;
        Ok(new_value)
    }
    pub fn sub(&mut self, v: T) -> Result<T, NumericError<T>> {
        let uncommitted_value = self.uncommitted_value.as_mut().unwrap_or_log();

        if self.positive {
            let new_value = if v <= *uncommitted_value {
                *uncommitted_value - v
            } else {
                self.positive = false;
                v - *uncommitted_value
            };

            *uncommitted_value = new_value;
            return Ok(new_value);
        }

        let max_v = T::max_value() - *uncommitted_value;
        if v > max_v {
            return Err(NumericError::Overflow {
                current: *uncommitted_value,
                added: v,
            });
        }
        let new_value = *uncommitted_value + v;
        *uncommitted_value = new_value;
        Ok(new_value)
    }

    /// Returns true if commit would succeed
    pub fn check_limit(&self) -> bool {
        if !self.positive {
            return false;
        }
        if let Some(limit) = self.unlocked_inner.opt_limit {
            let uncommitted_value = self.uncommitted_value.as_ref().unwrap_or_log();
            if *uncommitted_value > limit {
                return false;
            }
        }
        true
    }

    /// Ensures that a commit would succeed, like `check_limit` but returns the same error
    /// a commit would.
    pub fn verify_commit(&self) -> Result<T, LimitError<T>> {
        let uncommitted_value = self.uncommitted_value.as_ref().unwrap_or_log();
        if !self.positive {
            return Err(LimitError::BelowZero {
                value: *uncommitted_value,
            });
        }
        if let Some(limit) = self.unlocked_inner.opt_limit {
            if *uncommitted_value > limit {
                return Err(LimitError::OverLimit {
                    value: *uncommitted_value,
                    limit,
                });
            }
        }
        Ok(*uncommitted_value)
    }

    /// Make the final commit happen and return an error if the value is out of range
    pub fn commit(mut self) -> Result<T, LimitError<T>> {
        let uncommitted_value = self.uncommitted_value.take().unwrap_or_log();
        if !self.positive {
            veilid_log!(self debug "Commit under zero failed, rolled back LimitedSize({}): -{} < 0", self.unlocked_inner.description, uncommitted_value);
            return Err(LimitError::BelowZero {
                value: uncommitted_value,
            });
        }

        if let Some(limit) = self.unlocked_inner.opt_limit {
            if uncommitted_value > limit {
                veilid_log!(self debug "Commit over limit failed, rolled back LimitedSize({}): {} > {}", self.unlocked_inner.description, uncommitted_value, limit);
                return Err(LimitError::OverLimit {
                    value: uncommitted_value,
                    limit,
                });
            }
        }
        veilid_log!(self trace "Commit LimitedSize({}): {} => {}", self.unlocked_inner.description, self.inner.value, uncommitted_value);
        self.inner.value = uncommitted_value;
        Ok(self.inner.value)
    }

    /// Drops the uncommitted value and does nothing with it
    pub fn rollback(mut self) {
        if let Some(uv) = self.uncommitted_value.take() {
            veilid_log!(self trace "Rollback LimitedSize({}): {} (drop {}{})", self.unlocked_inner.description, self.inner.value, if self.positive { "" } else { "-" }, uv);
        }
    }
}

impl<T> fmt::Display for LimitedSizeGuard<T>
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // We use an unsafe read of this mutex because we don't want to
        // ever deadlock while printing the value
        write!(
            f,
            "[{}]:{}{}{}",
            &self.unlocked_inner.description,
            &self.inner.value,
            if let Some(limit) = self.unlocked_inner.opt_limit {
                format!("/{}", limit)
            } else {
                "".to_string()
            },
            if let Some(uncommitted_value) = self.uncommitted_value {
                format!("({} uncommitted)", uncommitted_value)
            } else {
                "".to_string()
            }
        )
    }
}
impl<T> Drop for LimitedSizeGuard<T>
where
    T: PrimInt + Unsigned + fmt::Display + fmt::Debug,
{
    fn drop(&mut self) {
        if let Some(uv) = self.uncommitted_value.take() {
            veilid_log!(self trace "Drop of uncommitted LimitedSize({}): {} (drop {})", self.unlocked_inner.description, self.inner.value, uv);
        }
    }
}
