use super::*;
use futures_util::StreamExt;
use stop_token::future::FutureExt;

impl_veilid_log_facility!("stor");

impl StorageManager {
    // Send value change notifications across the network
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub(super) async fn send_value_changes_task_routine(
        &self,
        stop_token: StopToken,
        _last_ts: Timestamp,
        _cur_ts: Timestamp,
    ) -> EyreResult<()> {
        let Ok(remote_record_store) = self.get_remote_record_store() else {
            return Ok(());
        };

        let value_changes = remote_record_store.take_value_changes().await;

        // Send all value changes in parallel
        let mut unord = FuturesUnordered::new();

        // Add a future for each value change
        for vc in value_changes {
            unord.push(
                async move {
                    if let Err(e) = self.send_value_change(vc).await {
                        veilid_log!(self debug "Failed to send value change: {}", e);
                    }
                }
                .in_current_span(),
            );
        }

        while !unord.is_empty() {
            match unord
                .next()
                .in_current_span()
                .timeout_at(stop_token.clone())
                .in_current_span()
                .await
            {
                Ok(Some(_)) => {
                    // Some ValueChanged completed
                }
                Ok(None) => {
                    // We're empty
                }
                Err(_) => {
                    // Timeout means we drop the rest because we were asked to stop
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    // Send single value change out to the network
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip(self), err)
    )]
    async fn send_value_change(&self, vc: ValueChangedInfo) -> VeilidAPIResult<()> {
        if !self.dht_is_online() {
            apibail_try_again!("network is not available");
        };

        let rpc_processor = self.rpc_processor();

        let dest = rpc_processor
            .resolve_target_to_destination(
                vc.target.clone(),
                SafetySelection::Unsafe(Sequencing::PreferOrdered),
            )
            .await
            .map_err(VeilidAPIError::from)?;

        network_result_value_or_log!(self rpc_processor
            .rpc_call_value_changed(dest, vc.record_key.clone(), vc.subkeys.clone(), vc.count, vc.watch_id.into(), vc.value.map(|v| (*v).clone()) )
            .await
            .map_err(VeilidAPIError::from)? => [format!(": dest={:?} vc={:?}", dest, vc)] {});

        Ok(())
    }
}
