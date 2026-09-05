use super::*;

impl NetworkManager {
    // Compute transfer statistics for the low level network
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    #[allow(clippy::unused_async)]
    pub async fn rolling_transfers_task_routine(
        &self,
        _stop_token: StopToken,
        last_ts: Timestamp,
        cur_ts: Timestamp,
    ) -> EyreResult<()> {
        // veilid_log!(self trace "--- network manager rolling_transfers task");
        {
            let stats = &mut *self.stats.write();

            // Roll the low level network transfer stats for our address
            stats.self_stats.transfer_stats_accounting.roll_transfers(
                last_ts,
                cur_ts,
                &mut stats.self_stats.transfer_stats,
            );

            // Roll all per-address transfers
            let mut dead_addrs: HashSet<PerAddressStatsKey> = HashSet::new();
            for (addr, pa_stats) in &mut stats.per_address_stats {
                pa_stats.transfer_stats_accounting.roll_transfers(
                    last_ts,
                    cur_ts,
                    &mut pa_stats.transfer_stats,
                );

                // While we're here, lets see if this address has timed out
                if cur_ts.duration_since(pa_stats.last_seen_ts) >= IPADDR_MAX_INACTIVE_DURATION {
                    // it's dead, put it in the dead list
                    dead_addrs.insert(*addr);
                }
            }

            // Remove the dead addresses from our tables
            for da in &dead_addrs {
                stats.per_address_stats.remove(da);
            }
        }

        // Broadcast network stats (cheap shared handle) so other components can react,
        // e.g. the storage manager's operation bandwidth gate
        if let Err(e) = self.event_bus().post(NetworkManagerStatsChangeEvent {
            stats: self.stats.clone(),
        }) {
            veilid_log!(self debug "failed to post network stats change event: {}", e);
        }

        // Send update
        self.send_network_update();

        Ok(())
    }
}
