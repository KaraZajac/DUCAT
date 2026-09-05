use super::*;

impl StorageManager {
    // Flush records stores to disk and remove dead records
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub(super) async fn flush_record_stores_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        _cur_ts: Timestamp,
    ) -> EyreResult<()> {
        if let Ok(local_record_store) = self.get_local_record_store() {
            if let Err(e) = local_record_store.flush().await {
                veilid_log!(self error "Error flushing local record store during flush task: {}", e);
            }
        }
        if let Ok(remote_record_store) = self.get_remote_record_store() {
            if let Err(e) = remote_record_store.flush().await {
                veilid_log!(self error "Error flushing remote record store during flush task: {}", e);
            }
        }
        Ok(())
    }
}
