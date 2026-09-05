use super::*;

impl StorageManager {
    /// Wait for any pending offline subkey writes on a record to drain.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn flush_record(
        &self,
        record_key: RecordKey,
        timeout: Option<Duration>,
    ) -> VeilidAPIResult<bool> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let Some(shutdown_token) = self.startup_lock.stop_token() else {
            apibail_not_initialized!();
        };

        let opaque_record_key = record_key.opaque();

        let token = {
            // inside a block so we don't hold the lock while waiting on the token
            let mut inner = self.inner.lock();
            // nothing to do if there's no pending writes
            if !inner.offline_subkey_writes.contains_key(&opaque_record_key) {
                return Ok(true);
            }

            // create and save the token
            let stop_source = StopSource::new();
            let token = stop_source.token();
            inner
                .flush_record_waiters
                .entry(opaque_record_key)
                .or_default()
                .push(stop_source);
            token
        };

        // wait for either the flush to complete or for shutdown/timeout
        let result = async {
            if let Some(duration) = timeout {
                crate::timeout(
                    duration.as_millis() as u32,
                    token.timeout_at(shutdown_token),
                )
                .await
            } else {
                Ok(token.timeout_at(shutdown_token).await)
            }
        }
        .await;

        match result {
            Err(_) => Ok(false),                      // timeout
            Ok(Err(_)) => apibail_not_initialized!(), // shutdown
            Ok(Ok(_)) => Ok(true),                    // success
        }
    }
}
