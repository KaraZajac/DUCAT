use super::*;
pub use keyvaluedb_web::*;

#[derive(Clone)]
#[must_use]
pub(in crate::table_store) struct TableStoreDriver {
    registry: VeilidComponentRegistry,
}

impl_veilid_component_accessors!(TableStoreDriver);

impl TableStoreDriver {
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        Self { registry }
    }

    fn get_namespaced_table_name(&self, table: &str) -> String {
        let config = self.registry().config();
        let namespace = config.namespace.clone();
        if namespace.is_empty() {
            table.to_owned()
        } else {
            format!("{}_{}", namespace, table)
        }
    }

    pub async fn open(
        &self,
        table_name: &str,
        column_count: u32,
        _concurrency: usize,
    ) -> VeilidAPIResult<Database> {
        let namespaced_table_name = self.get_namespaced_table_name(table_name);
        let db = Database::open(&namespaced_table_name, column_count, false)
            .await
            .map_err(|e| VeilidAPIError::generic(format!("failed to open table store: {}", e)))?;

        veilid_log!(self trace
            "opened table store '{}' with {} columns",
            namespaced_table_name,
            column_count
        );
        Ok(db)
    }

    /// Delete a TableDB table by name
    pub async fn delete(&self, table_name: &str) -> VeilidAPIResult<bool> {
        if is_browser() {
            let namespaced_table_name = self.get_namespaced_table_name(table_name);
            let out = Database::delete(&namespaced_table_name).await.is_ok();
            if out {
                veilid_log!(self trace "TableStore::delete {} deleted", namespaced_table_name);
            } else {
                veilid_log!(self debug "TableStore::delete {} not deleted", namespaced_table_name);
            }
            Ok(out)
        } else {
            unimplemented!();
        }
    }

    /// Delete every table in the currently configured namespace.
    pub async fn delete_all_in_namespace(&self) -> VeilidAPIResult<usize> {
        let namespace = self.config().namespace.clone();
        let opt_prefix = if namespace.is_empty() {
            None
        } else {
            Some(format!("{}_", namespace))
        };

        let entries = Database::list(opt_prefix.as_deref())
            .await
            .map_err(VeilidAPIError::from)?;

        let mut deleted = 0usize;
        for (name, _ver) in entries {
            match Database::delete(&name).await {
                Ok(()) => {
                    deleted += 1;
                    veilid_log!(self debug "delete_all_in_namespace: removed '{}'", name);
                }
                Err(e) => {
                    veilid_log!(self warn "delete_all_in_namespace: failed to remove '{}': {}", name, e);
                }
            }
        }
        Ok(deleted)
    }
}
