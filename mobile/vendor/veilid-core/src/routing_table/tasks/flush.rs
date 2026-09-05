use super::*;

impl_veilid_log_facility!("rtab");

impl RoutingTable {
    // Save routing table to disk
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub async fn flush_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        _cur_ts: Timestamp,
    ) -> EyreResult<()> {
        // Simple task, just writes everything to the tablestore
        self.flush().await;

        Ok(())
    }
}
