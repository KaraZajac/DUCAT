use crate::*;

cfg_if! {
    if #[cfg(all(target_arch = "wasm32", target_os = "unknown"))] {
        use keyvaluedb_web::*;
        use keyvaluedb::*;
    } else {
        use keyvaluedb_sqlite::*;
        use keyvaluedb::*;
    }
}

impl_veilid_log_facility!("tstore");

#[must_use]
#[derive(Debug)]
struct CryptInfo {
    secret: SharedSecret,
}
impl CryptInfo {
    pub fn new(secret: SharedSecret) -> Self {
        Self { secret }
    }
}

/// Shared state behind a `TableDB`: the open database, commit serialization lock, and encryption keys.
#[must_use]
pub(super) struct TableDBUnlockedInner {
    registry: VeilidComponentRegistry,
    table: String,
    database: Database,
    // Lock to serialize commits so they don't cause SQLITE_BUSY or similar errors
    commit_lock: AsyncMutex<()>,
    // Encryption and decryption key will be the same unless configured for an in-place migration
    encrypt_info: Option<CryptInfo>,
    decrypt_info: Option<CryptInfo>,
}

impl fmt::Debug for TableDBUnlockedInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TableDBUnlockedInner(table={})", self.table)
    }
}

/// A handle to an opened encrypted key-value table. Cheap to clone; clones share the same underlying database.
#[derive(Debug, Clone)]
#[must_use]
pub struct TableDB {
    opened_column_count: u32,
    unlocked_inner: Arc<TableDBUnlockedInner>,
}

impl VeilidComponentRegistryAccessor for TableDB {
    fn registry(&self) -> VeilidComponentRegistry {
        self.unlocked_inner.registry.clone()
    }
}

impl TableDB {
    pub(super) fn new(
        table: String,
        registry: VeilidComponentRegistry,
        database: Database,
        encryption_key: Option<SharedSecret>,
        decryption_key: Option<SharedSecret>,
        opened_column_count: u32,
    ) -> Self {
        let encrypt_info = encryption_key.map(CryptInfo::new);
        let decrypt_info = decryption_key.map(CryptInfo::new);

        let total_columns = database.num_columns().unwrap_or_log();

        Self {
            opened_column_count: if opened_column_count == 0 {
                total_columns
            } else {
                opened_column_count
            },
            unlocked_inner: Arc::new(TableDBUnlockedInner {
                registry,
                table,
                database,
                commit_lock: AsyncMutex::new(()),
                encrypt_info,
                decrypt_info,
            }),
        }
    }

    pub(super) fn new_from_unlocked_inner(
        unlocked_inner: Arc<TableDBUnlockedInner>,
        opened_column_count: u32,
    ) -> Self {
        let db = &unlocked_inner.database;
        let total_columns = db.num_columns().unwrap_or_log();
        Self {
            opened_column_count: if opened_column_count == 0 {
                total_columns
            } else {
                opened_column_count
            },
            unlocked_inner,
        }
    }

    pub(super) fn unlocked_inner(&self) -> Arc<TableDBUnlockedInner> {
        self.unlocked_inner.clone()
    }

    /// Get the internal name of the table
    #[must_use]
    pub fn table_name(&self) -> String {
        self.unlocked_inner.table.clone()
    }

    /// Get the io stats for the table
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    #[must_use]
    pub fn io_stats(&self, kind: IoStatsKind) -> IoStats {
        self.unlocked_inner.database.io_stats(kind)
    }

    /// Cleanup the database
    ///
    /// Blocks on on-disk database maintenance (vacuum).
    ///
    /// Errors with `VeilidAPIError::Internal` if the backing-store vacuum fails.
    pub async fn cleanup(&self) -> VeilidAPIResult<()> {
        self.unlocked_inner
            .database
            .cleanup()
            .measure_debug(
                TimestampDuration::new_secs(1),
                veilid_log_dbg!(self, "TableDB::cleanup {}", self.table_name()),
            )
            .await
            .map_err(VeilidAPIError::internal)
    }

    /// Get the total number of columns in the TableDB.
    /// Not the number of columns that were opened, rather the total number that could be opened.
    ///
    /// Errors with `VeilidAPIError::Generic` if the backing store fails to report its column count.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub fn get_column_count(&self) -> VeilidAPIResult<u32> {
        let db = &self.unlocked_inner.database;
        db.num_columns().map_err(VeilidAPIError::from)
    }

    /// Estimate the storage size for a table entry
    /// Overestimates size on disk because records are compressed in the tabledb
    /// Rough guess for sqlite based on their file format. Other databases may vary.
    ///
    /// Infallible on all supported targets (the `usize`→`u64` conversion only widens).
    pub fn estimate_storage_size(
        &self,
        _col: u32,
        key: &[u8],
        value: &[u8],
    ) -> VeilidAPIResult<u64> {
        let size =
            // Count of fields byte
            1 +
            // Type of field byte
            1 +
            // Length of key times two because it uses hex encoding sometimes
            key.len() * 2 +
            // Length of key length
            4 +
            // Length of value
            value.len() +
            // Length of value length
            4 +
            // Extra padding for max length and whatever else
            // XXX: at some point we should measure this on disk to figure out a better estimate :P
            4;
        size.try_into().map_err(VeilidAPIError::internal)
    }

    /// Estimate the storage size for a table entry if it is json encoded
    ///
    /// Errors with `VeilidAPIError::Internal` if `value` fails to JSON-serialize.
    pub fn estimate_storage_size_json<T>(
        &self,
        col: u32,
        key: &[u8],
        value: &T,
    ) -> VeilidAPIResult<u64>
    where
        T: serde::Serialize,
    {
        let value_json = serde_json::to_vec(value).map_err(VeilidAPIError::internal)?;
        self.estimate_storage_size(col, key, &value_json)
    }

    /// Encrypt buffer using encrypt key and prepend nonce to output.
    /// Keyed nonces are unique because keys must be unique.
    /// Normally they must be sequential or random, but the critical.
    /// requirement is that they are different for each encryption
    /// but if the contents are guaranteed to be unique, then a nonce
    /// can be generated from the hash of the contents and the encryption key itself.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub(in crate::table_store) async fn maybe_encrypt(
        &self,
        data: &[u8],
        keyed_nonce: bool,
    ) -> Bytes {
        let Some(ei) = &self.unlocked_inner.encrypt_info else {
            return Bytes::copy_from_slice(data);
        };

        let crypto = self.crypto();
        let vcrypto = crypto.get_async(ei.secret.kind()).unwrap_or_log();
        let mut out = BytesMut::zeroed(vcrypto.nonce_length() + data.len());

        if keyed_nonce {
            // Key content nonce
            let mut noncedata = BytesMut::with_capacity(data.len() + ei.secret.ref_value().len());
            noncedata.extend_from_slice(data);
            noncedata.extend_from_slice(ei.secret.ref_value());
            let noncehash = vcrypto.generate_hash(noncedata.freeze()).await.value();
            // Key content nonce is first 'nonce_length' bytes of generated hash
            out.as_mut()[0..vcrypto.nonce_length()]
                .copy_from_slice(&noncehash.as_ref()[0..vcrypto.nonce_length()]);
        } else {
            // Random nonce
            random_bytes(&mut out[0..vcrypto.nonce_length()]);
        }
        let nonce = Nonce::new(&out[0..vcrypto.nonce_length()]);

        let out = vcrypto
            .crypt_b2b_no_auth(
                Bytes::copy_from_slice(data),
                out,
                vcrypto.nonce_length(),
                &nonce,
                &ei.secret,
            )
            .await
            .unwrap_or_log();

        out.freeze()
    }

    /// Decrypt buffer using decrypt key with nonce prepended to input
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub(in crate::table_store) async fn maybe_decrypt(
        &self,
        data: &[u8],
    ) -> std::io::Result<Bytes> {
        let Some(di) = &self.unlocked_inner.decrypt_info else {
            return Ok(Bytes::copy_from_slice(data));
        };

        let crypto = self.crypto();
        let vcrypto = crypto.get_async(di.secret.kind()).unwrap_or_log();
        if data.len() < vcrypto.nonce_length() {
            veilid_log!(self error "maybe_decrypt: data too short for nonce: {} < {}", data.len(), vcrypto.nonce_length());
            return Err(std::io::Error::other("data too short for nonce"));
        }
        if data.len() == vcrypto.nonce_length() {
            return Ok(Bytes::new());
        }

        let out = BytesMut::zeroed(data.len() - vcrypto.nonce_length());
        let mut data = Bytes::copy_from_slice(data);
        let data_start = data.split_to(vcrypto.nonce_length());

        let out = vcrypto
            .crypt_b2b_no_auth(data, out, 0, &Nonce::new(data_start.as_ref()), &di.secret)
            .await
            .unwrap_or_log();

        Ok(out.freeze())
    }

    /// Get the list of keys in a column of the TableDB
    ///
    /// Blocks on on-disk reads.
    ///
    /// Errors with `VeilidAPIError::Generic` if `col` is at or above the opened column count, if the backing-store read fails, or if a stored key fails to decompress (wrong device encryption key or corrupt data).
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn get_keys(&self, col: u32) -> VeilidAPIResult<Vec<Vec<u8>>> {
        if col >= self.opened_column_count {
            apibail_generic!(
                "Column exceeds opened column count {} >= {}",
                col,
                self.opened_column_count
            );
        }
        let db = self.unlocked_inner.database.clone();
        let out = Vec::new();
        let (mut out, _) = db
            .iter_keys(col, None, out, |out, ekey| {
                //let key = self.maybe_decrypt(k).await?;
                out.push(ekey.clone());
                Ok(Option::<()>::None)
            })
            .await
            .map_err(VeilidAPIError::from)?;

        #[cfg(feature = "verbose-tracing")]
        veilid_log!(self debug "TableDB::get_keys({}) col={}: read {} raw keys", self.unlocked_inner.table, col, out.len());

        let max_value_size = self.config().table_store.max_value_size_mb as usize * 1024 * 1024;
        for (idx, k) in out.iter_mut().enumerate() {
            let raw_len = k.len();
            let decrypted = self.maybe_decrypt(k).await.map_err(|e| {
                let msg = format!("idx={} maybe_decrypt failed (raw_len={}): {}", idx, raw_len, e);
                veilid_log!(self warn "TableDB::get_keys({}) col={} {}", self.unlocked_inner.table, col, msg);
                VeilidAPIError::generic(msg)
            })?;
            let decompressed = decompress_size_prepended(&decrypted, max_value_size).map_err(|e| {
                let preview: Vec<u8> = decrypted.as_ref()[..decrypted.len().min(16)].to_vec();
                let msg = format!("idx={} decompress failed (raw_len={} decrypted_len={} first_bytes={:02x?}): {}", idx, raw_len, decrypted.len(), preview, e);
                veilid_log!(self warn "TableDB::get_keys({}) col={} {}", self.unlocked_inner.table, col, msg);
                std::io::Error::other(msg)
            })?;
            *k = decompressed;
        }
        Ok(out)
    }

    /// Get the number of keys in a column of the TableDB
    ///
    /// Blocks on on-disk reads.
    ///
    /// Errors with `VeilidAPIError::Generic` if `col` is at or above the opened column count or the backing-store read fails.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn get_key_count(&self, col: u32) -> VeilidAPIResult<u64> {
        if col >= self.opened_column_count {
            apibail_generic!(
                "Column exceeds opened column count {} >= {}",
                col,
                self.opened_column_count
            );
        }
        let db = self.unlocked_inner.database.clone();
        let key_count = db.num_keys(col).await.map_err(VeilidAPIError::from)?;
        Ok(key_count)
    }

    /// Start a TableDB write transaction. The transaction object must be committed or rolled back before dropping.
    ///
    /// Returns a handle whose `commit` or `rollback` the caller must call; dropping it uncompleted logs an error and silently discards the writes.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    #[must_use]
    pub fn transact(&self) -> TableDBTransaction {
        let dbt = self.unlocked_inner.database.transaction();
        TableDBTransaction::new(self.clone(), dbt)
    }

    /// Store a key with a value in a column in the TableDB. Performs a single transaction immediately.
    ///
    /// Blocks on the on-disk write.
    ///
    /// Errors with `VeilidAPIError::Generic` if `col` is at or above the opened column count or the backing-store write fails.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn store(&self, col: u32, key: &[u8], value: &[u8]) -> VeilidAPIResult<()> {
        if col >= self.opened_column_count {
            apibail_generic!(
                "Column exceeds opened column count {} >= {}",
                col,
                self.opened_column_count
            );
        }
        let db = self.unlocked_inner.database.clone();
        let mut dbt = db.transaction();
        dbt.put(
            col,
            self.maybe_encrypt(&compress_prepend_size(key), true).await,
            self.maybe_encrypt(&compress_prepend_size(value), false)
                .await,
        );
        db.write(dbt)
            .await
            .map_err(|e| VeilidAPIError::generic(format!("failed to store: {}", e)))
    }

    /// Store a key in json format with a value in a column in the TableDB. Performs a single transaction immediately.
    ///
    /// Blocks on the on-disk write.
    ///
    /// Errors with `VeilidAPIError::Internal` if `value` fails to JSON-serialize, otherwise the same errors as `store`.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn store_json<T>(&self, col: u32, key: &[u8], value: &T) -> VeilidAPIResult<()>
    where
        T: serde::Serialize,
    {
        let value = serde_json::to_vec(value).map_err(VeilidAPIError::internal)?;
        self.store(col, key, &value).await
    }

    /// Read a key from a column in the TableDB immediately.
    ///
    /// Blocks on the on-disk read.
    ///
    /// Returns `Ok(None)` if the key is absent. Errors with `VeilidAPIError::Generic` if `col` is at or above the opened column count, if the backing-store read fails, or if the stored value fails to decompress (wrong device encryption key or corrupt data).
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn load(&self, col: u32, key: &[u8]) -> VeilidAPIResult<Option<Vec<u8>>> {
        if col >= self.opened_column_count {
            apibail_generic!(
                "Column exceeds opened column count {} >= {}",
                col,
                self.opened_column_count
            );
        }
        let db = self.unlocked_inner.database.clone();
        let key = self.maybe_encrypt(&compress_prepend_size(key), true).await;
        let max_value_size = self.config().table_store.max_value_size_mb as usize * 1024 * 1024;
        match db.get(col, &key).await.map_err(VeilidAPIError::from)? {
            Some(v) => Ok(Some(
                decompress_size_prepended(
                    &self.maybe_decrypt(&v).await.map_err(VeilidAPIError::from)?,
                    max_value_size,
                )
                .map_err(|e| std::io::Error::other(e.to_string()))?,
            )),
            None => Ok(None),
        }
    }

    /// Read an serde-json key from a column in the TableDB immediately
    ///
    /// Blocks on the on-disk read.
    ///
    /// Errors with `VeilidAPIError::Internal` if the stored value fails to JSON-deserialize into `T`, otherwise the same errors as `load`.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn load_json<T>(&self, col: u32, key: &[u8]) -> VeilidAPIResult<Option<T>>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        let out = match self.load(col, key).await? {
            Some(v) => Some(serde_json::from_slice(&v).map_err(VeilidAPIError::internal)?),
            None => None,
        };
        Ok(out)
    }

    /// Delete key with from a column in the TableDB
    ///
    /// Blocks on the on-disk write.
    ///
    /// Returns `Ok(None)` if the key was absent. Errors with `VeilidAPIError::Generic` if `col` is at or above the opened column count, if the backing-store delete fails, or if the prior value fails to decompress (wrong device encryption key or corrupt data).
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn delete(&self, col: u32, key: &[u8]) -> VeilidAPIResult<Option<Vec<u8>>> {
        if col >= self.opened_column_count {
            apibail_generic!(
                "Column exceeds opened column count {} >= {}",
                col,
                self.opened_column_count
            );
        }
        let key = self.maybe_encrypt(&compress_prepend_size(key), true).await;

        let db = self.unlocked_inner.database.clone();

        let max_value_size = self.config().table_store.max_value_size_mb as usize * 1024 * 1024;
        match db.delete(col, &key).await.map_err(VeilidAPIError::from)? {
            Some(v) => Ok(Some(
                decompress_size_prepended(
                    &self.maybe_decrypt(&v).await.map_err(VeilidAPIError::from)?,
                    max_value_size,
                )
                .map_err(|e| std::io::Error::other(e.to_string()))?,
            )),
            None => Ok(None),
        }
    }

    /// Delete serde-json key with from a column in the TableDB
    ///
    /// Blocks on the on-disk write.
    ///
    /// Errors with `VeilidAPIError::Internal` if the prior value fails to JSON-deserialize into `T`, otherwise the same errors as `delete`.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn delete_json<T>(&self, col: u32, key: &[u8]) -> VeilidAPIResult<Option<T>>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        let old_value = match self.delete(col, key).await? {
            Some(v) => Some(serde_json::from_slice(&v).map_err(VeilidAPIError::internal)?),
            None => None,
        };
        Ok(old_value)
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

struct TableDBTransactionInner {
    registry: VeilidComponentRegistry,
    dbt: Option<DBTransaction>,
}

impl fmt::Debug for TableDBTransactionInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TableDBTransactionInner({})",
            match &self.dbt {
                Some(dbt) => format!("len={}", dbt.ops.len()),
                None => "".to_owned(),
            }
        )
    }
}

impl Drop for TableDBTransactionInner {
    fn drop(&mut self) {
        if self.dbt.is_some() {
            let registry = &self.registry;
            veilid_log!(registry error "Dropped transaction without commit or rollback");
        }
    }
}

/// A TableDB transaction
/// Atomically commits a group of writes or deletes to the TableDB
#[derive(Debug, Clone)]
pub struct TableDBTransaction {
    db: TableDB,
    inner: Arc<Mutex<TableDBTransactionInner>>,
}

impl VeilidComponentRegistryAccessor for TableDBTransaction {
    fn registry(&self) -> VeilidComponentRegistry {
        self.db.registry()
    }
}

impl TableDBTransaction {
    fn new(db: TableDB, dbt: DBTransaction) -> Self {
        let registry = db.registry();
        Self {
            db,
            inner: Arc::new(Mutex::new(TableDBTransactionInner {
                registry,
                dbt: Some(dbt),
            })),
        }
    }

    /// Commit the transaction. Performs all actions atomically.
    ///
    /// Consumes the transaction handle; committing an already-completed clone errors with "transaction already completed". An empty transaction commits as a no-op. Blocks on the serialized commit lock and the on-disk write.
    ///
    /// Errors with `VeilidAPIError::Generic` if this transaction (or a clone) was already committed or rolled back, or if the atomic backing-store write fails (the buffered writes are then lost).
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn commit(self) -> VeilidAPIResult<()> {
        let dbt = {
            let mut inner = self.inner.lock();
            inner
                .dbt
                .take()
                .ok_or_else(|| VeilidAPIError::generic("transaction already completed"))?
        };

        if dbt.ops.is_empty() {
            // Empty transactions are effectively rollbacks, so just return
            return Ok(());
        }

        let db = self.db.unlocked_inner.database.clone();
        let _commit_lock = self
            .db
            .unlocked_inner
            .commit_lock
            .lock()
            .measure_debug(
                TimestampDuration::new_ms(200),
                veilid_log_dbg!(
                    self,
                    "TableDBTransaction({})::commit lock",
                    self.db.table_name()
                ),
            )
            .await;
        db.write(dbt).await.map_err(|e| {
            veilid_log!(self error "commit failed, transaction lost: {:?}", e);
            VeilidAPIError::generic(format!("commit failed, transaction lost: {}", e))
        })
    }

    /// Rollback the transaction. Does nothing to the TableDB.
    ///
    /// Consumes the transaction handle and discards the buffered writes locally without blocking.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub fn rollback(self) {
        let mut inner = self.inner.lock();
        inner.dbt = None;
    }

    /// Store a key with a value in a column in the TableDB
    ///
    /// Buffers the write into the transaction without touching disk; errors if the transaction is already committed or rolled back.
    ///
    /// Errors with `VeilidAPIError::Generic` if `col` is at or above the opened column count, or if this transaction (or a clone) was already committed or rolled back.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn store(&self, col: u32, key: &[u8], value: &[u8]) -> VeilidAPIResult<()> {
        if col >= self.db.opened_column_count {
            apibail_generic!(
                "Column exceeds opened column count {} >= {}",
                col,
                self.db.opened_column_count
            );
        }

        let key = self
            .db
            .maybe_encrypt(&compress_prepend_size(key), true)
            .await;
        let value = self
            .db
            .maybe_encrypt(&compress_prepend_size(value), false)
            .await;
        let mut inner = self.inner.lock();
        inner
            .dbt
            .as_mut()
            .ok_or_else(|| VeilidAPIError::generic("store failed, transaction already completed"))?
            .put_owned(col, key.to_vec(), value.to_vec());
        Ok(())
    }

    /// Store a key in json format with a value in a column in the TableDB
    ///
    /// Buffers the write into the transaction without touching disk; errors if the transaction is already committed or rolled back.
    ///
    /// Errors with `VeilidAPIError::Internal` if `value` fails to JSON-serialize, otherwise the same errors as `store`.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn store_json<T>(&self, col: u32, key: &[u8], value: &T) -> VeilidAPIResult<()>
    where
        T: serde::Serialize,
    {
        let value = serde_json::to_vec(value).map_err(VeilidAPIError::internal)?;
        self.store(col, key, &value).await
    }

    /// Delete key with from a column in the TableDB
    ///
    /// Buffers the delete into the transaction without touching disk; errors if the transaction is already committed or rolled back.
    ///
    /// Errors with `VeilidAPIError::Generic` if `col` is at or above the opened column count, or if this transaction (or a clone) was already committed or rolled back.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "tstore", skip_all)
    )]
    pub async fn delete(&self, col: u32, key: &[u8]) -> VeilidAPIResult<()> {
        if col >= self.db.opened_column_count {
            apibail_generic!(
                "Column exceeds opened column count {} >= {}",
                col,
                self.db.opened_column_count
            );
        }

        let key = self
            .db
            .maybe_encrypt(&compress_prepend_size(key), true)
            .await;
        let mut inner = self.inner.lock();
        inner
            .dbt
            .as_mut()
            .ok_or_else(|| VeilidAPIError::generic("delete failed, transaction already completed"))?
            .delete_owned(col, key.to_vec());
        Ok(())
    }
}
