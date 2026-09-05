use super::*;

impl StorageManager {
    // Check if server-side transactions have expired
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub(super) fn check_inbound_transactions_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        _cur_ts: Timestamp,
    ) -> EyreResult<()> {
        if let Ok(remote_record_store) = self.get_remote_record_store() {
            remote_record_store.drop_expired_inbound_transactions();
        }

        Ok(())
    }
}
