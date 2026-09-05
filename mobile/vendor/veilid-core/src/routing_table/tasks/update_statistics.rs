use super::*;

impl_veilid_log_facility!("rtab");

impl RoutingTable {
    // Compute transfer statistics to determine how 'fast' a node is
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn rolling_transfers_task_routine(
        &self,
        _stop_token: StopToken,
        last_ts: Timestamp,
        cur_ts: Timestamp,
    ) -> EyreResult<()> {
        {
            let self_transfer_stats_accounting = &mut *self.self_transfer_stats_accounting.lock();

            // Roll our own node's transfers
            self_transfer_stats_accounting.0.roll_transfers(
                last_ts,
                cur_ts,
                &mut self_transfer_stats_accounting.1,
            );
        }

        {
            // Roll all bucket entry transfers
            let all_entries: Vec<Arc<BucketEntry>> = self.inner.read().all_entries.iter().collect();
            for entry in all_entries {
                entry.with_mut(|e| e.roll_transfers(last_ts, cur_ts));
            }
        }

        // Roll all route transfers
        self.route_spec_store().roll_transfers(last_ts, cur_ts);

        Ok(())
    }

    // Update state statistics in PeerStats
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn update_state_stats_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        _cur_ts: Timestamp,
    ) -> EyreResult<()> {
        // Roll all bucket entry transfers
        let all_entries: Vec<Arc<BucketEntry>> = self.inner.read().all_entries.iter().collect();
        for entry in all_entries {
            entry.with_mut(|e| e.update_state_stats());
        }

        Ok(())
    }

    // Update rolling answers in PeerStats
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    pub fn rolling_answers_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        cur_ts: Timestamp,
    ) -> EyreResult<()> {
        // Roll all bucket entry answers stats
        let all_entries: Vec<Arc<BucketEntry>> = self.inner.read().all_entries.iter().collect();
        for entry in all_entries {
            entry.with_mut(|e| e.roll_answer_stats(cur_ts));
        }

        // Roll all route answers
        let rss = self.route_spec_store();
        rss.roll_answers(cur_ts);

        Ok(())
    }
}
