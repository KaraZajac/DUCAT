use super::*;

mod table_db;
mod tasks;

pub use table_db::*;

#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod tests_table_store;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod wasm;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use wasm::*;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod native;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use native::*;

use keyvaluedb::*;
use weak_table::WeakValueHashMap;

impl_veilid_log_facility!("tstore");

const ALL_TABLE_NAMES: &[u8] = b"all_table_names";
const FLUSH_TABLES_INTERVAL_SECS: u32 = 60;
const CLEANUP_TABLES_INTERVAL_SECS: u32 = 600;

/// Description of column
#[apply(api_data_struct!)]
#[api(eq, default, ts)]
pub struct ColumnInfo {
    pub key_count: AlignedU64,
}

/// IO Stats for table
#[apply(api_data_struct!)]
#[api(eq, default, ts)]
pub struct IOStatsInfo {
    /// Number of transaction.
    pub transactions: AlignedU64,
    /// Number of read operations.
    pub reads: AlignedU64,
    /// Number of reads resulted in a read from cache.
    pub cache_reads: AlignedU64,
    /// Number of write operations.
    pub writes: AlignedU64,
    /// Number of bytes read
    pub bytes_read: ByteCount,
    /// Number of bytes read from cache
    pub cache_read_bytes: ByteCount,
    /// Number of bytes write
    pub bytes_written: ByteCount,
    /// Number of delete operations.
    pub deletes: AlignedU64,
    /// Number of prefix (batch) delete operations.
    pub prefix_deletes: AlignedU64,
    /// Write size buckets. Keys are write sizes, slightly rounded upwards.
    /// Values are the number of times a value of a particular size was written.
    pub write_size_buckets: BTreeMap<usize, u64>,
    /// Similar to `write_size_buckets` but tracks total sizes of transactions
    /// and the average duration for a transaction of that size. The duration is
    /// in **microseconds**.
    pub tx_write_size_buckets: BTreeMap<usize, (u64, TimestampDuration)>,
    /// Start of the statistic period.
    pub started: Timestamp,
    /// Total duration of the statistic period.
    pub span: TimestampDuration,
}

/// Description of table
#[apply(api_data_struct!)]
#[api(eq, default, ts)]
pub struct TableInfo {
    /// Internal table name
    pub table_name: String,
    /// IO statistics since previous query
    pub io_stats_since_previous: IOStatsInfo,
    /// IO statistics since database open
    pub io_stats_overall: IOStatsInfo,
    /// Total number of columns in the table
    pub column_count: u32,
    /// Column descriptions
    pub columns: Vec<ColumnInfo>,
}

#[must_use]
struct TableStoreInner {
    opened: WeakValueHashMap<String, Weak<TableDBUnlockedInner>>,
    encryption_key: Option<SharedSecret>,
    all_table_names: HashMap<String, String>,
    all_tables_db: Option<Database>,
    /// Tick subscription
    tick_subscription: Option<EventBusSubscription>,
}

impl fmt::Debug for TableStoreInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TableStoreInner")
            .field("opened", &self.opened)
            .field("encryption_key", &self.encryption_key)
            .field("all_table_names", &self.all_table_names)
            .finish()
    }
}

/// Veilid Table Storage.
/// Database for storing key value pairs persistently and securely across runs.
#[must_use]
pub struct TableStore {
    registry: VeilidComponentRegistry,
    startup_lock: StartupLock,
    inner: Mutex<TableStoreInner>, // Sync mutex here because TableDB drops can happen at any time
    table_store_driver: TableStoreDriver,
    async_lock: Arc<AsyncMutex<()>>,
    flush_tables_task: TickTask<EyreReport>,
    cleanup_tables_task: TickTask<EyreReport>,
}

impl fmt::Debug for TableStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TableStore")
            .field("registry", &self.registry)
            .field("startup_lock", &self.startup_lock)
            .field("inner", &self.inner)
            .field("async_lock", &self.async_lock)
            .finish()
    }
}

impl_veilid_component!(TableStore);

impl TableStore {
    fn new_inner() -> TableStoreInner {
        TableStoreInner {
            opened: WeakValueHashMap::new(),
            encryption_key: None,
            all_table_names: HashMap::new(),
            all_tables_db: None,
            tick_subscription: None,
        }
    }
    pub(crate) fn new(registry: VeilidComponentRegistry) -> Self {
        let inner = Self::new_inner();
        let table_store_driver = TableStoreDriver::new(registry.clone());

        let this = Self {
            registry,
            startup_lock: StartupLock::new(),
            inner: Mutex::new(inner),
            table_store_driver,
            async_lock: Arc::new(AsyncMutex::new(())),
            flush_tables_task: TickTask::new("flush_tables_task", FLUSH_TABLES_INTERVAL_SECS),
            cleanup_tables_task: TickTask::new("cleanup_tables_task", CLEANUP_TABLES_INTERVAL_SECS),
        };

        this.setup_tasks();

        this
    }

    // Flush internal control state
    async fn flush(&self) {
        let (all_table_names_value, all_tables_db) = {
            let inner = self.inner.lock();
            let all_table_names_value = serialize_json_bytes(&inner.all_table_names);
            (
                all_table_names_value,
                inner.all_tables_db.clone().unwrap_or_log(),
            )
        };
        let mut dbt = DBTransaction::new();
        dbt.put(0, ALL_TABLE_NAMES, &all_table_names_value);
        if let Err(e) = all_tables_db.write(dbt).await {
            veilid_log!(self error "failed to write all tables db: {}", e);
        }
    }

    // Cleanup/vacuum database
    async fn cleanup(&self) {
        // Vacuum the open databases while we're at it
        let all_open_db: Vec<_> = {
            let inner = self.inner.lock();
            inner.opened.values().collect()
        };
        for db in all_open_db {
            let tdb = TableDB::new_from_unlocked_inner(db, 0);
            if let Err(e) = tdb.cleanup().await {
                veilid_log!(self error "Error cleaning up database '{}': {}", tdb.table_name(), e);
            }
        }
    }

    // Internal naming support
    // Adds rename capability and ensures names of tables are totally unique and valid

    fn namespaced_name(&self, table: &str) -> VeilidAPIResult<String> {
        if !table
            .chars()
            .all(|c| char::is_alphanumeric(c) || c == '_' || c == '-')
        {
            apibail_invalid_argument!("table name is invalid", "table", table);
        }
        let namespace = self.config().namespace.clone();
        Ok(if namespace.is_empty() {
            table.to_string()
        } else {
            format!("_ns_{}_{}", namespace, table)
        })
    }

    fn name_get_or_create(&self, table: &str) -> VeilidAPIResult<String> {
        let name = self.namespaced_name(table)?;

        let mut inner = self.inner.lock();
        // Do we have this name yet?
        if let Some(real_name) = inner.all_table_names.get(&name) {
            return Ok(real_name.clone());
        }

        // If not, make a new low level name mapping
        let mut real_name_bytes = [0u8; 32];
        random_bytes(&mut real_name_bytes);
        let real_name = data_encoding::BASE64URL_NOPAD.encode(&real_name_bytes);

        if inner
            .all_table_names
            .insert(name.to_owned(), real_name.clone())
            .is_some()
        {
            veilid_log!(self error "should not have already had this table name: {}", name);
        };

        Ok(real_name)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    fn name_delete(&self, table: &str) -> VeilidAPIResult<Option<String>> {
        let name = self.namespaced_name(table)?;
        let mut inner = self.inner.lock();
        let real_name = inner.all_table_names.remove(&name);
        Ok(real_name)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    fn name_get(&self, table: &str) -> VeilidAPIResult<Option<String>> {
        let name = self.namespaced_name(table)?;
        let inner = self.inner.lock();
        let real_name = inner.all_table_names.get(&name).cloned();
        Ok(real_name)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    fn name_rename(&self, old_table: &str, new_table: &str) -> VeilidAPIResult<()> {
        let old_name = self.namespaced_name(old_table)?;
        let new_name = self.namespaced_name(new_table)?;

        let mut inner = self.inner.lock();
        // Ensure new name doesn't exist
        if inner.all_table_names.contains_key(&new_name) {
            return Err(VeilidAPIError::generic("new table already exists"));
        }
        // Do we have this name yet?
        let Some(real_name) = inner.all_table_names.remove(&old_name) else {
            return Err(VeilidAPIError::generic("table does not exist"));
        };
        // Insert with new name
        inner.all_table_names.insert(new_name.to_owned(), real_name);

        Ok(())
    }

    /// List all known tables
    ///
    /// Reads cached names locally without blocking on disk; returns empty before init or after shutdown.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub fn list_all(&self) -> Vec<(String, String)> {
        let Ok(_startup_guard) = self.startup_lock.enter() else {
            return vec![];
        };

        let inner = self.inner.lock();
        inner
            .all_table_names
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<(String, String)>>()
    }

    /// Delete all known tables
    ///
    /// No-op before init or after shutdown. Blocks on deleting each table on disk and flushing.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn delete_all(&self) {
        let Ok(_startup_guard) = self.startup_lock.enter() else {
            return;
        };

        // Get all tables
        let table_names = {
            let mut inner = self.inner.lock();
            let real_names = inner
                .all_table_names
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<(String, String)>>();
            inner.all_table_names.clear();
            real_names
        };

        // Delete all tables
        for (table_name, table_real_name) in table_names {
            veilid_log!(self debug "deleting table: {} ({})", table_real_name, table_name);
            if let Err(e) = self.table_store_driver.delete(&table_real_name).await {
                veilid_log!(self error "error deleting table: {}", e);
            }
        }
        self.flush().await;
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub(crate) async fn maybe_unprotect_device_encryption_key(
        &self,
        dek_bytes: &[u8],
        device_encryption_key_password: &str,
    ) -> EyreResult<SharedSecret> {
        // Ensure the key is at least as long as necessary
        // to check for crypto kind
        if dek_bytes.len() < 4 {
            bail!("device encryption key is not valid");
        }

        // Get cryptosystem
        let kind = CryptoKind::try_from(&dek_bytes[0..4]).unwrap_or_log();
        let crypto = self.crypto();
        let Some(vcrypto) = crypto.get_async(kind) else {
            bail!("unsupported cryptosystem '{kind}'");
        };

        if !device_encryption_key_password.is_empty() {
            if dek_bytes.len()
                != (4
                    + vcrypto.shared_secret_length()
                    + vcrypto.aead_overhead()
                    + vcrypto.nonce_length())
            {
                bail!("password protected device encryption key is not valid");
            }
            let protected_key =
                &dek_bytes[4..(4 + vcrypto.shared_secret_length() + vcrypto.aead_overhead())];
            let nonce = Nonce::new(
                &dek_bytes[(4 + vcrypto.shared_secret_length() + vcrypto.aead_overhead())..],
            );
            let shared_secret = vcrypto
                .derive_shared_secret(
                    Bytes::copy_from_slice(device_encryption_key_password.as_bytes()),
                    nonce.bytes(),
                )
                .await
                .wrap_err("failed to derive shared secret")?;

            let unprotected_key = vcrypto
                .decrypt_aead(
                    Bytes::copy_from_slice(protected_key),
                    &nonce,
                    &shared_secret,
                    None,
                )
                .await
                .wrap_err("failed to decrypt device encryption key")?;

            return Ok(SharedSecret::new(
                kind,
                BareSharedSecret::new(unprotected_key.as_ref()),
            ));
        }

        if dek_bytes.len() != (4 + vcrypto.shared_secret_length()) {
            bail!("unprotected device encryption key is not valid");
        }

        Ok(SharedSecret::new(
            kind,
            BareSharedSecret::new(&dek_bytes[4..]),
        ))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub(crate) async fn maybe_protect_device_encryption_key(
        &self,
        dek: SharedSecret,
        device_encryption_key_password: &str,
    ) -> EyreResult<Vec<u8>> {
        // Check if we are to protect the key
        if device_encryption_key_password.is_empty() {
            veilid_log!(self debug "no dek password");
            // Return the unprotected key bytes
            return Ok(Vec::from(dek));
        }

        // Get cryptosystem
        let crypto = self.crypto();
        let Some(vcrypto) = crypto.get_async(dek.kind()) else {
            bail!("unsupported cryptosystem '{}'", dek.kind());
        };

        let nonce = vcrypto.random_nonce().await;
        let shared_secret = vcrypto
            .derive_shared_secret(
                Bytes::copy_from_slice(device_encryption_key_password.as_bytes()),
                Bytes::copy_from_slice(&nonce),
            )
            .await
            .wrap_err("failed to derive shared secret")?;
        let protected_key = vcrypto
            .encrypt_aead(
                Bytes::copy_from_slice(dek.ref_value()),
                &nonce,
                &shared_secret,
                None,
            )
            .await
            .wrap_err("failed to decrypt device encryption key")?;
        let mut out = Vec::with_capacity(
            4 + vcrypto.shared_secret_length() + vcrypto.aead_overhead() + vcrypto.nonce_length(),
        );
        out.extend_from_slice(dek.kind().bytes());
        out.extend_from_slice(&protected_key);
        out.extend_from_slice(&nonce);
        debug_assert_eq!(
            out.len(),
            4 + vcrypto.shared_secret_length() + vcrypto.aead_overhead() + vcrypto.nonce_length()
        );
        Ok(out)
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    async fn load_device_encryption_key(&self) -> EyreResult<Option<SharedSecret>> {
        let dek_bytes: Option<Vec<u8>> = self
            .protected_store()
            .load_user_secret("device_encryption_key")?;
        let Some(dek_bytes) = dek_bytes else {
            veilid_log!(self debug "no device encryption key");
            return Ok(None);
        };

        // Get device encryption key protection password if we have it
        let device_encryption_key_password = self
            .config()
            .protected_store
            .device_encryption_key_password
            .clone();

        Ok(Some(
            self.maybe_unprotect_device_encryption_key(&dek_bytes, &device_encryption_key_password)
                .await?,
        ))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    async fn save_device_encryption_key(
        &self,
        device_encryption_key: Option<SharedSecret>,
    ) -> EyreResult<()> {
        let Some(device_encryption_key) = device_encryption_key else {
            // Remove the device encryption key
            let existed = self
                .protected_store()
                .remove_user_secret("device_encryption_key")?;
            veilid_log!(self debug "removed device encryption key. existed: {}", existed);
            return Ok(());
        };

        // Get new device encryption key protection password if we are changing it
        let new_device_encryption_key_password = self
            .config()
            .protected_store
            .new_device_encryption_key_password
            .clone();
        let device_encryption_key_password =
            if let Some(new_device_encryption_key_password) = new_device_encryption_key_password {
                // Change password
                veilid_log!(self debug "changing dek password");
                new_device_encryption_key_password
            } else {
                // Get device encryption key protection password if we have it
                veilid_log!(self debug "saving with existing dek password");
                self.config()
                    .protected_store
                    .device_encryption_key_password
                    .clone()
            };

        let dek_bytes = self
            .maybe_protect_device_encryption_key(
                device_encryption_key,
                &device_encryption_key_password,
            )
            .await?;

        // Save the new device encryption key
        let existed = self
            .protected_store()
            .save_user_secret("device_encryption_key", &dek_bytes)?;
        veilid_log!(self debug "saving device encryption key. existed: {}", existed);
        Ok(())
    }

    fn log_facilities_impl(&self) -> VeilidComponentLogFacilities {
        VeilidComponentLogFacilities::new().with_facility(
            VeilidComponentLogFacility::try_new_with_tags("tstore", ["#common"]).unwrap(),
        )
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    async fn init_async(&self) -> EyreResult<()> {
        let startup_guard = self.startup_lock.startup()?;
        {
            let _async_guard = self.async_lock.lock().await;

            // Get device encryption key from protected store
            let mut device_encryption_key = self.load_device_encryption_key().await?;
            let mut device_encryption_key_changed = false;
            if let Some(device_encryption_key) = &device_encryption_key {
                // If encryption in current use is not the best encryption, then run table migration
                let best_kind = best_crypto_kind();
                if device_encryption_key.kind() != best_kind {
                    // XXX: Run migration. See issue #209
                    veilid_log!(self error "Need to write migration support");
                }
            } else {
                // If we don't have an encryption key yet, then make one with the best cryptography and save it
                let crypto = self.crypto();
                let vcrypto = crypto.best_async();
                let shared_secret = vcrypto.random_shared_secret().await;

                device_encryption_key = Some(shared_secret);
                device_encryption_key_changed = true;
            }

            // Check for password change
            let changing_password = self
                .config()
                .protected_store
                .new_device_encryption_key_password
                .is_some();

            // Save encryption key if it has changed or if the protecting password wants to change
            if device_encryption_key_changed || changing_password {
                self.save_device_encryption_key(device_encryption_key.clone())
                    .await?;
            }

            // Deserialize all table names
            let all_tables_db = match self
                .table_store_driver
                .open("__veilid_all_tables", 1, 1)
                .await
            {
                Ok(db) => db,
                Err(e) => {
                    veilid_log!(self error "failed to create all tables table: {}", e);
                    return Err(e.into());
                }
            };
            match all_tables_db.get(0, ALL_TABLE_NAMES).await {
                Ok(Some(v)) => match deserialize_json_bytes::<HashMap<String, String>>(&v) {
                    Ok(all_table_names) => {
                        let mut inner = self.inner.lock();
                        inner.all_table_names = all_table_names;
                    }
                    Err(e) => {
                        veilid_log!(self error "could not deserialize __veilid_all_tables: {}", e);
                    }
                },
                Ok(None) => {
                    // No table names yet, that's okay
                    veilid_log!(self trace "__veilid_all_tables is empty");
                }
                Err(e) => {
                    veilid_log!(self error "could not get __veilid_all_tables: {}", e);
                }
            };

            {
                let mut inner = self.inner.lock();
                inner.encryption_key = device_encryption_key;
                inner.all_tables_db = Some(all_tables_db);
            }
        }
        startup_guard.success();

        // Delete all tables if config has 'delete' enabled
        // Must happen after startup guard is marked as successful
        let do_delete = self.config().table_store.delete;
        if do_delete {
            veilid_log!(self debug "TableStore config 'delete' enabled: deleting all tables");
            self.delete_all().await;
        }

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    #[allow(clippy::unused_async)]
    async fn post_init_async(&self) -> EyreResult<()> {
        // Register event handlers
        let tick_subscription = impl_subscribe_event_bus_async!(self, Self, tick_event_handler);

        let mut inner = self.inner.lock();

        // Schedule tick
        inner.tick_subscription = Some(tick_subscription);

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    #[allow(clippy::unused_async)]
    async fn pre_terminate_async(&self) {
        // Unsubscribe from ticker
        let mut inner = self.inner.lock();
        if let Some(sub) = inner.tick_subscription.take() {
            self.event_bus().unsubscribe(sub);
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    async fn terminate_async(&self) {
        let Ok(_startup_guard) = self.startup_lock.shutdown().await else {
            veilid_log!(self error "table store is already shut down");
            return;
        };

        // Cancel tasks
        self.cancel_tasks().await;

        self.flush().await;

        let mut inner = self.inner.lock();
        inner.opened.shrink_to_fit();
        if !inner.opened.is_empty() {
            veilid_log!(self warn
                "all open databases should have been closed: {:?}",
                inner.opened
            );
            inner.opened.clear();
        }
        inner.all_tables_db = None;
        inner.all_table_names.clear();
        inner.encryption_key = None;
    }

    /// Get or create a TableDB database table. If the column count is greater than an
    /// existing TableDB's column count, the database will be upgraded to add the missing columns.
    ///
    /// Returns a `TableDB` handle the caller must drop before the table can be deleted; while any clone is held the table counts as opened. Opening the same name again returns a handle to the same shared database. Blocks on opening the on-disk database.
    ///
    /// Same errors as `open_pooled`.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn open(&self, name: &str, column_count: u32) -> VeilidAPIResult<TableDB> {
        self.open_pooled(name, column_count, 1).await
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    /// Get or create a TableDB table, opening a pool of `concurrency` database connections for it.
    /// Like `open`, upgrades the table if `column_count` exceeds the existing column count.
    ///
    /// Returns a `TableDB` handle the caller must drop before the table can be deleted; while any clone is held the table counts as opened. Opening the same name again returns a handle to the same shared database. Blocks on opening the on-disk database.
    ///
    /// Errors with:
    /// - `VeilidAPIError::InvalidArgument` if `column_count` is zero or `name` contains characters other than alphanumeric, `_`, or `-`.
    /// - `VeilidAPIError::NotInitialized` before init or after shutdown, or when the existing data fails to decrypt (wrong device encryption key) and `wipe_on_invalid_device_encryption_key` is disabled.
    /// - `VeilidAPIError::Generic` if the table is already open with a smaller column count than requested (close it first), or if the backing-store open fails (`::Internal` for filesystem-permission failures).
    pub async fn open_pooled(
        &self,
        name: &str,
        column_count: u32,
        concurrency: usize,
    ) -> VeilidAPIResult<TableDB> {
        if column_count == 0 {
            apibail_invalid_argument!(
                "column count must be greater than zero",
                "column_count",
                column_count
            );
        }

        let Ok(_startup_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let _async_guard = self.async_lock.lock().await;

        // If we aren't initialized yet, bail
        {
            let inner = self.inner.lock();
            if inner.all_tables_db.is_none() {
                apibail_not_initialized!();
            }
        }

        let mut table_name = self.name_get_or_create(name)?;

        // See if this table is already opened, if so the column count must be the same
        {
            let inner = self.inner.lock();
            if let Some(table_db_unlocked_inner) = inner.opened.get(&table_name) {
                let tdb = TableDB::new_from_unlocked_inner(table_db_unlocked_inner, column_count);

                // Ensure column count isnt bigger
                let existing_col_count = tdb.get_column_count()?;
                if column_count > existing_col_count {
                    return Err(VeilidAPIError::generic(format!(
                        "database must be closed before increasing column count {} -> {}",
                        existing_col_count, column_count,
                    )));
                }

                return Ok(tdb);
            }
        }

        // Open table db using platform-specific driver.
        let mut db = match self
            .table_store_driver
            .open(&table_name, column_count, concurrency)
            .await
        {
            Ok(db) => db,
            Err(e) => {
                self.name_delete(name).expect_or_log("removing name failed");
                self.flush().await;
                return Err(e);
            }
        };

        // Flush table names to disk
        self.flush().await;

        // If more columns are available, open the low level db with the max column count but restrict the tabledb object to the number requested
        let existing_col_count = db.num_columns().map_err(VeilidAPIError::from)?;
        if existing_col_count > column_count {
            drop(db);
            db = match self
                .table_store_driver
                .open(&table_name, existing_col_count, concurrency)
                .await
            {
                Ok(db) => db,
                Err(e) => {
                    self.name_delete(name).expect_or_log("removing name failed");
                    self.flush().await;
                    return Err(e);
                }
            };
        }

        // Wrap low-level Database in TableDB object
        let encryption_key = self.inner.lock().encryption_key.clone();
        let table_db = TableDB::new(
            table_name.clone(),
            self.registry(),
            db,
            encryption_key.clone(),
            encryption_key.clone(),
            column_count,
        );

        // Validate keys are readable if they exist, otherwise wipe this database
        // because there is an invalid encryption key and no way to read the data
        let mut first_failure: Option<(u32, String)> = None;
        for col in 0..existing_col_count {
            match table_db.get_keys(col).await {
                Ok(_keys) => {
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(self debug "table {}({}) col {}: {} keys read ok", name, table_name, col, _keys.len());
                }
                Err(e) => {
                    veilid_log!(self warn "table {}({}) col {}: get_keys failed: {}", name, table_name, col, e);
                    if first_failure.is_none() {
                        first_failure = Some((col, e.to_string()));
                    }
                }
            }
        }

        let table_db = if let Some((failed_col, failure_msg)) = first_failure {
            let namespace = self.config().namespace.clone();

            // If the wipe policy is disabled, return NotInitialized so the caller
            // can decide what to do (e.g., prompt for the right key).
            if !self
                .config()
                .table_store
                .wipe_on_invalid_device_encryption_key
            {
                veilid_log!(self error
                    "table {}({}) has invalid encryption key (col {} failed: {}); wipe_on_invalid_device_encryption_key=false, refusing to wipe namespace '{}'",
                    name, table_name, failed_col, failure_msg, namespace
                );
                return Err(VeilidAPIError::not_initialized());
            }

            veilid_log!(self warn
                "table {}({}) has invalid encryption key (col {} failed: {}); wiping all tables in namespace '{}' and starting fresh",
                name, table_name, failed_col, failure_msg, namespace
            );

            // Drop our open handle to the bad table before deleting files.
            drop(table_db);

            // Drop the open all_tables_db handle and clear cached state so
            // we can delete and re-create everything in the namespace.
            {
                let mut inner = self.inner.lock();
                inner.all_tables_db = None;
                inner.all_table_names.clear();
                inner.opened.clear();
            }

            let deleted = self.table_store_driver.delete_all_in_namespace().await?;
            veilid_log!(self warn "wiped {} table file(s) from namespace '{}'", deleted, namespace);

            // Reopen __veilid_all_tables fresh.
            let all_tables_db = self
                .table_store_driver
                .open("__veilid_all_tables", 1, 1)
                .await?;
            {
                let mut inner = self.inner.lock();
                inner.all_tables_db = Some(all_tables_db);
            }

            // Generate a new random real_name for the requested friendly name
            // and open an empty database for it.
            table_name = self.name_get_or_create(name)?;
            self.flush().await;

            let db = self
                .table_store_driver
                .open(&table_name, column_count, concurrency)
                .await?;

            TableDB::new(
                table_name.clone(),
                self.registry(),
                db,
                encryption_key.clone(),
                encryption_key.clone(),
                column_count,
            )
        } else {
            table_db
        };

        // Keep track of opened DBs
        let mut inner = self.inner.lock();
        inner
            .opened
            .insert(table_name.clone(), table_db.unlocked_inner());

        Ok(table_db)
    }

    /// Delete a TableDB table by name
    ///
    /// Errors if the table is still opened; the caller must drop all `TableDB` handles for it first. Blocks on the on-disk delete and flush.
    ///
    /// Returns `Ok(false)` if no table by that name exists. Errors with:
    /// - `VeilidAPIError::NotInitialized` before init or after shutdown.
    /// - `VeilidAPIError::InvalidArgument` if `name` contains characters other than alphanumeric, `_`, or `-`.
    /// - `VeilidAPIError::Generic` if the table is still opened (drop all handles first) or the backing-store delete fails.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn delete(&self, name: &str) -> VeilidAPIResult<bool> {
        let Ok(_startup_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let _async_guard = self.async_lock.lock().await;
        // If we aren't initialized yet, bail
        {
            let inner = self.inner.lock();
            if inner.all_tables_db.is_none() {
                apibail_not_initialized!();
            }
        }

        let Some(table_name) = self.name_get(name)? else {
            // Did not exist in name table
            return Ok(false);
        };

        // See if this table is opened
        {
            let inner = self.inner.lock();
            if inner.opened.contains_key(&table_name) {
                apibail_generic!("Not deleting table that is still opened");
            }
        }

        // Delete table db using platform-specific driver
        let deleted = self.table_store_driver.delete(&table_name).await?;
        if !deleted {
            // Table missing? Just remove name
            veilid_log!(self warn
                "table existed in name table but not in storage: {} : {}",
                name, table_name
            );
        }
        if let Err(e) = self.name_delete(name) {
            veilid_log!(self error "failed to delete name: {}", e);
            return Err(e);
        }
        self.flush().await;
        Ok(true)
    }
    /// Get column and key-count information for a table, or `None` if it does not exist.
    ///
    /// Opens the table to read it; blocks on the on-disk open and key-count reads.
    ///
    /// Errors with `VeilidAPIError::NotInitialized` before init or after shutdown, the same errors as `open` (the table is opened to read it), or `VeilidAPIError::Generic` if a column-count or key-count read fails.
    pub async fn info(&self, name: &str) -> VeilidAPIResult<Option<TableInfo>> {
        let Ok(_startup_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        // Open with at least one column, then reopen to match all available columns.
        let mut tdb = self.open(name, 1).await?;
        let column_count = tdb.get_column_count()?;
        if column_count > 1 {
            tdb = self.open(name, column_count).await?;
        }

        let internal_name = tdb.table_name();
        let io_stats_since_previous = tdb.io_stats(IoStatsKind::SincePrevious);
        let io_stats_overall = tdb.io_stats(IoStatsKind::Overall);
        let mut columns = Vec::<ColumnInfo>::with_capacity(column_count as usize);
        for col in 0..column_count {
            let key_count = tdb.get_key_count(col).await?;
            columns.push(ColumnInfo {
                key_count: AlignedU64::new(key_count),
            })
        }
        Ok(Some(TableInfo {
            table_name: internal_name,
            io_stats_since_previous: IOStatsInfo {
                transactions: AlignedU64::new(io_stats_since_previous.transactions),
                reads: AlignedU64::new(io_stats_since_previous.reads),
                cache_reads: AlignedU64::new(io_stats_since_previous.cache_reads),
                writes: AlignedU64::new(io_stats_since_previous.writes),
                bytes_read: ByteCount::new(io_stats_since_previous.bytes_read),
                cache_read_bytes: ByteCount::new(io_stats_since_previous.cache_read_bytes),
                bytes_written: ByteCount::new(io_stats_since_previous.bytes_written),
                deletes: AlignedU64::new(io_stats_since_previous.deletes),
                prefix_deletes: AlignedU64::new(io_stats_since_previous.prefix_deletes),
                write_size_buckets: io_stats_since_previous
                    .write_size_buckets
                    .into_iter()
                    .collect(),
                tx_write_size_buckets: io_stats_since_previous
                    .tx_write_size_buckets
                    .into_iter()
                    .map(|(k, (count, avg_duration))| {
                        (k, (count, TimestampDuration::new(avg_duration as u64)))
                    })
                    .collect(),
                started: Timestamp::new(io_stats_since_previous.started),
                span: TimestampDuration::new(io_stats_since_previous.span.as_micros() as u64),
            },
            io_stats_overall: IOStatsInfo {
                transactions: AlignedU64::new(io_stats_overall.transactions),
                reads: AlignedU64::new(io_stats_overall.reads),
                cache_reads: AlignedU64::new(io_stats_overall.cache_reads),
                writes: AlignedU64::new(io_stats_overall.writes),
                bytes_read: ByteCount::new(io_stats_overall.bytes_read),
                cache_read_bytes: ByteCount::new(io_stats_overall.cache_read_bytes),
                bytes_written: ByteCount::new(io_stats_overall.bytes_written),
                deletes: AlignedU64::new(io_stats_overall.deletes),
                prefix_deletes: AlignedU64::new(io_stats_overall.prefix_deletes),
                write_size_buckets: io_stats_overall.write_size_buckets.into_iter().collect(),
                tx_write_size_buckets: io_stats_overall
                    .tx_write_size_buckets
                    .into_iter()
                    .map(|(k, (count, avg_duration))| {
                        (k, (count, TimestampDuration::new(avg_duration as u64)))
                    })
                    .collect(),
                started: Timestamp::new(io_stats_overall.started),
                span: TimestampDuration::new(io_stats_overall.span.as_micros() as u64),
            },
            column_count,
            columns,
        }))
    }

    /// Rename a TableDB table
    ///
    /// Blocks on flushing the renamed name table to disk.
    ///
    /// Errors with:
    /// - `VeilidAPIError::NotInitialized` before init or after shutdown.
    /// - `VeilidAPIError::InvalidArgument` if `old_name` or `new_name` contains characters other than alphanumeric, `_`, or `-`.
    /// - `VeilidAPIError::Generic` if `new_name` already exists or `old_name` does not exist.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn rename(&self, old_name: &str, new_name: &str) -> VeilidAPIResult<()> {
        let Ok(_startup_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        let _async_guard = self.async_lock.lock().await;
        // If we aren't initialized yet, bail
        {
            let inner = self.inner.lock();
            if inner.all_tables_db.is_none() {
                apibail_not_initialized!();
            }
        }
        veilid_log!(self debug "TableStore::rename {} -> {}", old_name, new_name);
        self.name_rename(old_name, new_name)?;
        self.flush().await;
        Ok(())
    }

    async fn tick_event_handler(&self, evt: Arc<TickEvent>) {
        let lag = evt.last_tick_ts.map(|x| evt.cur_tick_ts.duration_since(x));
        if let Err(e) = self.tick(lag).await {
            error!("Error in table store tick: {}", e);
        }
    }
}
