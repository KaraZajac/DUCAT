use super::*;

impl NativeNetwork {
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) async fn network_interfaces_task_routine(
        &self,
        stop_token: StopToken,
        _l: Timestamp,
        _t: Timestamp,
    ) -> EyreResult<()> {
        // Network lock ensures only one task operating on the low level network state
        // can happen at the same time. Try lock is here to give preference to other longer
        // running processes like update_dial_info_task.
        let _guard = match self
            .network_task_lock
            .write()
            .timeout_at(stop_token.clone())
            .await
        {
            Ok(v) => v,
            Err(_) => {
                // If we can't get the lock right now, then
                return Ok(());
            }
        };

        // See if any routing domains are configured to detect address changes
        let routing_domain_detect_address_changes =
            self.routing_domains_detecting_address_changes();
        if routing_domain_detect_address_changes.is_empty() {
            return Ok(());
        }

        // Update the network state
        self.update_network_state(stop_token).await?;

        Ok(())
    }

    // See if our interface addresses have changed, if so redo public dial info if necessary
    async fn update_network_state(&self, _stop_token: StopToken) -> EyreResult<()> {
        let Some(last_network_state) = self.last_network_state() else {
            veilid_log!(self error "No previous network state available during update tick");
            bail!("No previous network state available during update tick");
        };

        let new_network_state = match self.refresh_network_state().await {
            Ok(Some(v)) => v,
            Ok(None) => {
                // Nothing has changed
                return Ok(());
            }
            Err(e) => {
                veilid_log!(self debug "Skipping network state update: {}", e);
                return Ok(());
            }
        };

        // Update the public internet domain
        let routing_domain_detect_address_changes =
            self.routing_domains_detecting_address_changes();

        if routing_domain_detect_address_changes.contains(&RoutingDomain::PublicInternet) {
            self.update_public_internet_routing_domain(
                Some(last_network_state.clone()),
                new_network_state.clone(),
            )?;
        }

        // Update the local network domain
        if routing_domain_detect_address_changes.contains(&RoutingDomain::LocalNetwork) {
            self.update_local_network_routing_domain(
                Some(last_network_state.clone()),
                new_network_state.clone(),
            )?;
        }

        Ok(())
    }
}
