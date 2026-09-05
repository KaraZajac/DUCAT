use super::*;

impl StorageManager {
    // Save metadata to disk
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub(super) async fn save_metadata_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        _cur_ts: Timestamp,
    ) -> EyreResult<()> {
        self.save_metadata().await
    }
}
