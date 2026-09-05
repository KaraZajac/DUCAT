use futures_util::future::{select, Either};

use super::*;

#[allow(dead_code)]
#[derive(Debug)]
pub struct DurationRecorder<'a> {
    name: &'a str,
    start_ts: Timestamp,
    finished: bool,
    #[cfg(feature = "instrument")]
    span: Option<Span>,
}

#[allow(dead_code)]
impl<'a> DurationRecorder<'a> {
    /// Starts a new duration recorder with a name and a callback to be called before the duration is recorded
    pub fn new<F: FnOnce(&str, Timestamp)>(name: &'a str, before: F) -> Self {
        let start_ts = Timestamp::now_non_decreasing();
        #[cfg(feature = "instrument")]
        let span = Some(debug_span!("duration_marker", name, start_ts = %start_ts));
        before(name, start_ts);
        Self {
            name,
            start_ts,
            finished: false,
            #[cfg(feature = "instrument")]
            span,
        }
    }

    /// Ends the duration recorder and records the duration
    /// Calls the after callback with the start timestamp, passing through a result from the after callback
    pub fn end<A, R>(mut self, after: A) -> R
    where
        A: FnOnce(&str, Timestamp, TimestampDuration) -> R,
    {
        let duration = TimestampDuration::since_non_decreasing(self.start_ts);
        #[cfg(feature = "instrument")]
        if let Some(span) = self.span.take() {
            span.record("duration", duration.to_string());
        }
        self.finished = true;
        after(self.name, self.start_ts, duration)
    }

    /// Records the duration of a closure and calls the after callback with the start timestamp, passing through a result from the after callback
    pub fn record<C, A, R>(mut self, closure: C, after: A) -> R
    where
        C: FnOnce() -> R,
        A: FnOnce(&str, Timestamp, TimestampDuration, R) -> R,
    {
        let out = closure();
        let duration = TimestampDuration::since_non_decreasing(self.start_ts);
        #[cfg(feature = "instrument")]
        if let Some(span) = self.span.take() {
            span.record("duration", duration.to_string());
        }
        self.finished = true;
        after(self.name, self.start_ts, duration, out)
    }

    /// Records the duration of a future and calls the after callback with the start timestamp, the duration, and the result of the future
    pub async fn record_fut<
        F: Future<Output = R>,
        A: FnOnce(&str, Timestamp, TimestampDuration, R) -> R,
        R,
    >(
        mut self,
        fut: F,
        after: A,
    ) -> R {
        let out = Box::pin(fut).await;
        let duration = TimestampDuration::since_non_decreasing(self.start_ts);
        #[cfg(feature = "instrument")]
        if let Some(span) = self.span.take() {
            span.record("duration", duration.to_string());
        }
        self.finished = true;
        after(self.name, self.start_ts, duration, out)
    }
}

impl<'a> Drop for DurationRecorder<'a> {
    fn drop(&mut self) {
        if !self.finished {
            // prefer this after switching to task-local for log key
            // veilid_log!(error "DurationRecorder dropped with no recording: {:?}", self);
            tracing::error!("DurationRecorder dropped with no recording: {:?}", self);
        }
        #[cfg(feature = "instrument")]
        if let Some(span) = self.span.take() {
            let duration = TimestampDuration::since_non_decreasing(self.start_ts);
            span.record("duration", duration.to_string());
        }
    }
}

#[allow(dead_code)]
pub async fn record_duration_fut<F, R>(fut: F) -> R
where
    F: Future<Output = R>,
{
    #[cfg(feature = "instrument")]
    let start = Timestamp::now_non_decreasing();
    let out = Box::pin(fut).await;
    #[cfg(feature = "instrument")]
    let duration = TimestampDuration::since_non_decreasing(start);
    #[cfg(feature = "instrument")]
    tracing::Span::current().record("duration", duration.to_string());
    out
}

#[allow(dead_code)]
pub fn record_duration<C, R>(closure: C) -> R
where
    C: FnOnce() -> R,
{
    #[cfg(feature = "instrument")]
    let start = Timestamp::now_non_decreasing();
    let out = closure();
    #[cfg(feature = "instrument")]
    let duration = TimestampDuration::since_non_decreasing(start);
    #[cfg(feature = "instrument")]
    tracing::Span::current().record("duration", duration.to_string());
    out
}

#[allow(dead_code)]
pub fn debug_duration<C, R, D>(closure: C, limit: TimestampDuration, callback: D) -> R
where
    C: FnOnce() -> R,
    D: FnOnce(String),
{
    let start = Timestamp::now_non_decreasing();
    let out = closure();
    let duration = TimestampDuration::since_non_decreasing(start);
    if duration > limit {
        let msg = format!("Excessive duration: {:#}", duration);
        callback(msg);
    }

    out
}

#[allow(dead_code)]
pub trait MeasureFuture<T, C>
where
    C: FnOnce(TimestampDuration),
{
    fn measure(self, callback: C) -> impl Future<Output = T>;
    fn measure_limit(self, limit: TimestampDuration, callback: C) -> impl Future<Output = T>;
}

#[allow(dead_code)]
pub trait MeasureDebugFuture<T, D>
where
    D: FnOnce(String),
{
    fn measure_debug(self, limit: TimestampDuration, callback: D) -> impl Future<Output = T>;
}

impl<T, C, M> MeasureFuture<T, C> for M
where
    C: FnOnce(TimestampDuration),
    M: Future<Output = T>,
{
    async fn measure(self, callback: C) -> T {
        let start = Timestamp::now_non_decreasing();
        let out = Box::pin(self).await;
        let duration = TimestampDuration::since_non_decreasing(start);
        callback(duration);
        out
    }

    async fn measure_limit(self, limit: TimestampDuration, callback: C) -> T {
        let start = Timestamp::now_non_decreasing();
        let out = Box::pin(self).await;
        let duration = TimestampDuration::since_non_decreasing(start);
        if duration > limit {
            callback(duration);
        }
        out
    }
}

#[allow(dead_code)]
impl<T, D, M> MeasureDebugFuture<T, D> for M
where
    D: Fn(String),
    M: Future<Output = T>,
{
    async fn measure_debug(self, limit: TimestampDuration, callback: D) -> T {
        let start = Timestamp::now_non_decreasing();

        let res = select(
            Box::pin(self),
            Box::pin(sleep(limit.millis_u32().unwrap_or_log())),
        )
        .await;
        let out = match res {
            Either::Left((out, sleep_fut)) => {
                drop(sleep_fut);
                out
            }
            Either::Right((_, fut)) => {
                let msg = format!("Duration limit exceeded: {:#}", limit);
                callback(msg);
                fut.await
            }
        };
        let duration = TimestampDuration::since_non_decreasing(start);
        if duration > limit {
            let msg = format!("Excessive duration: {:#}", duration);
            callback(msg);
        }
        out
    }
}
