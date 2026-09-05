pub mod cleanup_tables;
pub mod flush_tables;

use super::*;

impl TableStore {
    pub(super) fn setup_tasks(&self) {
        // Set flush tables tick task
        veilid_log!(self debug "starting flush tables task");
        impl_setup_task_async!(self, Self, flush_tables_task, flush_tables_task_routine);

        // Set cleanup tables tick task
        veilid_log!(self debug "starting cleanup tables task");
        impl_setup_task_async!(self, Self, cleanup_tables_task, cleanup_tables_task_routine);
    }

    #[cfg_attr(feature = "instrument", instrument(parent = None, level = "trace", target = "tstore", name = "TableStore::tick", skip_all, err))]
    /// Run the periodic flush and cleanup table maintenance tasks.
    ///
    /// No-op before init or after shutdown. Each subtask only runs once its interval has elapsed (60s flush, 600s cleanup), so calling more often is cheap; an actual flush or cleanup run blocks on disk.
    pub async fn tick(&self, _lag: Option<TimestampDuration>) -> EyreResult<()> {
        let Ok(_startup_guard) = self.startup_lock.enter() else {
            return Ok(());
        };

        // Run the flush tables task
        self.flush_tables_task.tick().await?;

        // Run the cleanup tables task
        self.cleanup_tables_task.tick().await?;

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub(super) async fn cancel_tasks(&self) {
        veilid_log!(self debug "stopping flush tables task");
        if let Err(e) = self.flush_tables_task.stop().await {
            veilid_log!(self warn "flush_tables_task not stopped: {}", e);
        }
        veilid_log!(self debug "stopping cleanup tables task");
        if let Err(e) = self.cleanup_tables_task.stop().await {
            veilid_log!(self warn "cleanup_tables_task not stopped: {}", e);
        }
    }
}
