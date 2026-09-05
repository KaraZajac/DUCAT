use super::*;

impl TableStore {
    // Cleanup/vacuum records stores on disk
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub(super) async fn cleanup_tables_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        _cur_ts: Timestamp,
    ) -> EyreResult<()> {
        self.cleanup().await;
        Ok(())
    }
}
