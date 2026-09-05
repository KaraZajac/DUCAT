use super::*;

impl StorageManager {
    /// Create a local record from scratch with a new owner key, open it, and return the opened descriptor
    pub async fn create_record(
        &self,
        kind: CryptoKind,
        schema: DHTSchema,
        owner: Option<KeyPair>,
        safety_selection: SafetySelection,
    ) -> VeilidAPIResult<DHTRecordDescriptor> {
        let Ok(_guard) = self.startup_lock.enter() else {
            apibail_not_initialized!();
        };

        // Validate schema
        schema.validate()?;

        // Create a new owned local record from scratch
        let (key, owner) = self
            .create_new_owned_local_record(kind, schema, owner, safety_selection.clone())
            .await?;

        // Lock the record key
        let records_lock = self
            .record_lock_table
            .lock_record(key.opaque(), StorageManagerRecordLockPurpose::Create)
            .await;

        // Now that the record is made we should always succeed to open the existing record
        // The initial writer is the owner of the record
        self.open_existing_record_locked(&records_lock, key, Some(owner), safety_selection)
            .map(|r| r.unwrap_or_log())
    }

    ////////////////////////////////////////////////////////////////////////

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn create_new_owned_local_record(
        &self,
        kind: CryptoKind,
        schema: DHTSchema,
        owner: Option<KeyPair>,
        safety_selection: SafetySelection,
    ) -> VeilidAPIResult<(RecordKey, KeyPair)> {
        // Get cryptosystem
        let crypto = self.crypto();
        let Some(vcrypto) = crypto.get_async(kind) else {
            apibail_generic!("unsupported cryptosystem");
        };

        // Get local record store
        let local_record_store = self.get_local_record_store()?;

        // Verify the dht schema does not contain the node id
        {
            let config = self.config();
            if let Some(node_id) = config.network.routing_table.public_keys.get(kind) {
                let node_member_id = BareMemberId::new(node_id.ref_value());
                if schema.is_member(&node_member_id) {
                    apibail_invalid_argument!(
                        "node id can not be schema member",
                        "schema",
                        node_id.value()
                    );
                }
            }
        }

        // Compile the dht schema
        let schema_data = schema.compile();

        // New values require a new owner key if not given
        let owner = if let Some(owner) = owner {
            if owner.kind() != vcrypto.kind() {
                apibail_invalid_argument!("owner is wrong crypto kind", "owner", owner);
            }
            owner
        } else {
            vcrypto.generate_keypair().await
        };

        // Always create a new encryption key
        let encryption_key = Some(vcrypto.random_shared_secret().await.into_value());

        // Calculate dht key
        let record_key = Self::make_record_key(
            &vcrypto,
            owner.ref_value().ref_key(),
            &schema_data,
            encryption_key,
        )
        .await;

        // Make a signed value descriptor for this dht value
        let signed_value_descriptor = Arc::new(
            SignedValueDescriptor::make_signature(
                owner.key(),
                schema_data.into(),
                &vcrypto,
                owner.secret(),
            )
            .await?,
        );

        // Add new local value record
        let cur_ts = Timestamp::now();
        let local_record_detail = LocalRecordDetail::new(safety_selection);
        let record =
            Record::<LocalRecordDetail>::new(cur_ts, signed_value_descriptor, local_record_detail)?;

        let opaque_record_key = record_key.opaque();
        local_record_store
            .new_record(opaque_record_key, record)
            .await?;

        Ok((record_key, owner))
    }
}
