use super::*;

use crate::routing_table::tasks::ping_validator::PingValidationGroup;

impl_veilid_log_facility!("rtab");

/// Keepalive pings are done occasionally to ensure holepunched public dialinfo
/// remains valid, as well as to make sure we remain in any relay node's routing table
const RELAY_KEEPALIVE_PING_INTERVAL: TimestampDuration = TimestampDuration::new_secs(10);

/// Keepalive pings are done for active watch nodes to make sure they are still there
const ACTIVE_WATCH_KEEPALIVE_PING_INTERVAL: TimestampDuration = TimestampDuration::new_secs(10);

impl PublicInternetRoutingDomainController {
    // Task routine for PublicInternet status pings
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err))]
    pub fn ping_validator_public_internet_task_routine(
        &self,
        _stop_token: StopToken,
        _last_ts: Timestamp,
        cur_ts: Timestamp,
    ) -> EyreResult<()> {
        self.routing_table()
            .add_reliability_ping_validations(cur_ts, RoutingDomain::PublicInternet)?;

        self.add_relay_keepalive_ping_validations(cur_ts)?;

        self.add_active_watches_keepalive_ping_validations(cur_ts)?;

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

    /// Enqueue relay keepalive pings for all relays that need them and record the keepalive timestamp
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err))]
    fn add_relay_keepalive_ping_validations(&self, cur_ts: Timestamp) -> EyreResult<()> {
        let routing_table = self.routing_table();

        // Iterate the PublicInternet relays
        let relays = self.read_dyn().relays();
        let mut state_updates = Vec::new();
        let mut validations = Vec::new();
        let name = format!("RelayKeepalive({})", RoutingDomain::PublicInternet);
        for relay in relays {
            let Some(mut state) = self.read_dyn().relay_state(relay.relay_id.clone()) else {
                bail!("Relay state not found for relay {}", relay.relay_id);
            };

            let relay_needs_keepalive =
                cur_ts.duration_since(state.last_keepalive) >= RELAY_KEEPALIVE_PING_INTERVAL;

            if !relay_needs_keepalive {
                continue;
            }

            // Enqueue the pings
            for relay_ping in relay.pings.clone() {
                let dest = Destination::dial_info(
                    relay_ping.dial_info.clone(),
                    relay_ping.node_ref.clone(),
                );
                validations.push(dest);
            }

            // Say we're doing this keepalive now
            state.last_keepalive = cur_ts;
            state_updates.push((relay.relay_id.clone(), state));
        }

        routing_table.enqueue_ping_validations(
            name,
            PingValidationGroup::Keepalive,
            0,
            validations,
        );

        // Update the relay keepalive timestamp on the routing domain
        if !state_updates.is_empty() {
            let mut editor = self.edit();
            for (relay_id, state) in state_updates {
                editor.set_relay_state(relay_id, state);
            }
            editor.commit();
        }

        Ok(())
    }

    // Ping the active watch nodes to ensure they are still there
    #[cfg_attr(feature = "instrument", instrument(level = "trace", skip(self), err))]
    fn add_active_watches_keepalive_ping_validations(&self, cur_ts: Timestamp) -> EyreResult<()> {
        let watches_need_keepalive = {
            let mut opt_active_watch_keepalive_ts = self.opt_active_watch_keepalive_ts.lock();

            let need = opt_active_watch_keepalive_ts
                .map(|kts| cur_ts.duration_since(kts) >= ACTIVE_WATCH_KEEPALIVE_PING_INTERVAL)
                .unwrap_or(true);
            if need {
                *opt_active_watch_keepalive_ts = Some(cur_ts);
            }
            need
        };

        if !watches_need_keepalive {
            return Ok(());
        }

        // Get all the active watches from the storage manager
        let validations = self.storage_manager().get_outbound_watch_nodes();
        let name = format!("WatchKeepalive({})", RoutingDomain::PublicInternet);

        self.routing_table().enqueue_ping_validations(
            name,
            PingValidationGroup::Keepalive,
            10,
            validations,
        );

        Ok(())
    }
}
