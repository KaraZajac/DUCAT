pub mod flush;
pub mod kick_buckets;
pub mod ping_validator;
pub mod update_statistics;

use super::*;

impl_veilid_log_facility!("rtab");

impl RoutingTable {
    pub fn setup_tasks(&self) {
        // Set flush tick task
        impl_setup_task_async!(self, Self, flush_task, flush_task_routine);

        // Set rolling transfers tick task
        impl_setup_task!(
            self,
            Self,
            rolling_transfers_task,
            rolling_transfers_task_routine
        );

        // Set update state stats tick task
        impl_setup_task!(
            self,
            Self,
            update_state_stats_task,
            update_state_stats_task_routine
        );

        // Set rolling answers tick task
        impl_setup_task!(
            self,
            Self,
            rolling_answers_task,
            rolling_answers_task_routine
        );

        // Set kick buckets tick task
        impl_setup_task!(self, Self, kick_buckets_task, kick_buckets_task_routine);

        // Note: ping validation processor is spawned in startup(), not as a TickTask
    }

    /// Event bus handler for the ticker
    pub async fn tick_event_handler(&self, _evt: Arc<TickEvent>) {
        if let Err(e) = self.tick().await {
            error!("Error in routing table tick: {}", e);
        }
    }

    /// Ticks about once per second
    /// to run tick tasks which may run at slower tick rates as configured
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", name = "RoutingTable::tick", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn tick(&self) -> EyreResult<()> {
        let Ok(_startup_guard) = self.startup_context.startup_lock.enter() else {
            return Ok(());
        };

        // Do flush every ROUTING_TABLE_FLUSH_INTERVAL_SECS secs
        self.flush_task.tick().await?;

        // Do rolling transfers every ROLLING_TRANSFERS_INTERVAL_SECS secs
        self.rolling_transfers_task.tick().await?;

        // Do state stats update every UPDATE_STATE_STATS_INTERVAL_SECS secs
        self.update_state_stats_task.tick().await?;

        // Do rolling answers every ROLLING_ANSWER_INTERVAL_SECS secs
        self.rolling_answers_task.tick().await?;

        // Kick buckets task
        let kick_bucket_queue_count = self.kick_queue.lock().len();
        if kick_bucket_queue_count > 0 {
            self.kick_buckets_task.tick().await?;
        }

        // Refresh summary of all entries in the routing table and cache them per routing domain
        //   - Don't reset the low water marks
        self.refresh_summaries(RoutingDomainSet::empty());

        // Process the routing domain detail ticks
        for rdc in self.get_routing_domain_controllers(RoutingDomainSet::all()) {
            rdc.tick().await?;
        }

        Ok(())
    }

    pub async fn cancel_tasks(&self) {
        // Cancel all tasks being ticked
        veilid_log!(self debug "stopping flush task");
        if let Err(e) = self.flush_task.stop().await {
            veilid_log!(self warn "flush_task not stopped: {}", e);
        }
        veilid_log!(self debug "stopping rolling transfers task");
        if let Err(e) = self.rolling_transfers_task.stop().await {
            veilid_log!(self warn "rolling_transfers_task not stopped: {}", e);
        }
        veilid_log!(self debug "stopping update state stats task");
        if let Err(e) = self.update_state_stats_task.stop().await {
            veilid_log!(self warn "update_state_stats_task not stopped: {}", e);
        }
        veilid_log!(self debug "stopping rolling answers task");
        if let Err(e) = self.rolling_answers_task.stop().await {
            veilid_log!(self warn "rolling_answers_task not stopped: {}", e);
        }
        veilid_log!(self debug "stopping kick buckets task");
        if let Err(e) = self.kick_buckets_task.stop().await {
            veilid_log!(self warn "kick_buckets_task not stopped: {}", e);
        }
        veilid_log!(self debug "stopping routing domain controller task");
        for rdc in self.get_routing_domain_controllers(RoutingDomainSet::all()) {
            rdc.cancel_tasks().await;
        }
    }
}
