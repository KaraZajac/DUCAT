use super::*;

impl TableStore {
    // Flush records stores to disk and remove dead records
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub(super) async fn flush_tables_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        _cur_ts: Timestamp,
    ) -> EyreResult<()> {
        self.flush().await;
        Ok(())
    }
}
