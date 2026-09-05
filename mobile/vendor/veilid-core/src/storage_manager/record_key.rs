use super::*;

impl StorageManager {
    /// Builds the record key for a given schema and owner
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub async fn get_record_key(
        &self,
        schema: DHTSchema,
        owner_key: &PublicKey,
        encryption_key: Option<SharedSecret>,
    ) -> VeilidAPIResult<RecordKey> {
        // Get cryptosystem
        let crypto = self.crypto();
        let Some(vcrypto) = crypto.get_async(owner_key.kind()) else {
            apibail_generic!("unsupported cryptosystem");
        };

        // Encryption key must match owner key
        if let Some(ek) = &encryption_key {
            vcrypto.check_shared_secret(ek)?;
        }

        // Validate schema
        schema.validate()?;
        let schema_data = schema.compile();

        Ok(Self::make_record_key(
            &vcrypto,
            owner_key.ref_value(),
            &schema_data,
            encryption_key.map(|x| x.into_value()),
        )
        .await)
    }

    /// Validate a record key
    pub fn check_record_key(&self, record_key: &RecordKey) -> VeilidAPIResult<()> {
        let crypto = self.crypto();
        let Some(vcrypto) = crypto.get(record_key.kind()) else {
            apibail_generic!("unsupported record key kind");
        };

        if record_key.value().key().len() != HASH_COORDINATE_LENGTH {
            apibail_generic!(
                "invalid record key length: {} != {}",
                record_key.value().key().len(),
                HASH_COORDINATE_LENGTH
            );
        }
        if let Some(encryption_key) = record_key.value().encryption_key() {
            if encryption_key.len() != vcrypto.shared_secret_length() {
                apibail_generic!(
                    "invalid encryption key length: {} != {}",
                    encryption_key.len(),
                    vcrypto.shared_secret_length()
                );
            }
        }

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////

    pub(super) async fn make_opaque_record_key(
        vcrypto: &AsyncCryptoSystemGuard<'_>,
        owner_key: &BarePublicKey,
        schema_data: &[u8],
    ) -> OpaqueRecordKey {
        let mut hash_data = BytesMut::with_capacity(owner_key.len() + 4 + schema_data.len());
        hash_data.extend_from_slice(vcrypto.kind().bytes());
        hash_data.extend_from_slice(owner_key);
        hash_data.extend_from_slice(schema_data);
        let hash = vcrypto.generate_hash(hash_data.freeze()).await;

        OpaqueRecordKey::new(vcrypto.kind(), BareOpaqueRecordKey::new(hash.ref_value()))
    }

    pub(super) async fn make_record_key(
        vcrypto: &AsyncCryptoSystemGuard<'_>,
        owner_key: &BarePublicKey,
        schema_data: &[u8],
        encryption_key: Option<BareSharedSecret>,
    ) -> RecordKey {
        let opaque = Self::make_opaque_record_key(vcrypto, owner_key, schema_data).await;

        RecordKey::new(
            vcrypto.kind(),
            BareRecordKey::new(opaque.into_value(), encryption_key),
        )
    }
}
