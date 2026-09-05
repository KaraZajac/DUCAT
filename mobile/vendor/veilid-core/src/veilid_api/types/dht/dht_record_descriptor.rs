use super::*;

/// DHT Record Descriptor
#[apply(api_data_struct!)]
#[api(eq, ord)]
pub struct DHTRecordDescriptor {
    /// DHT Key = Hash(ownerKeyKind) of: [ ownerKeyValue, schema ]
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    key: RecordKey,
    /// The public key of the owner
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    owner: PublicKey,
    /// If this key is being created: Some(the secret key of the owner)
    /// If this key is just being opened: None
    #[cfg_attr(feature = "schemars", schemars(with = "Option<String>"))]
    owner_secret: Option<SecretKey>,
    /// The schema in use associated with the key
    schema: DHTSchema,
}

impl DHTRecordDescriptor {
    pub(crate) fn new(
        key: RecordKey,
        owner: PublicKey,
        owner_secret: Option<SecretKey>,
        schema: DHTSchema,
    ) -> Self {
        if let Some(owner_secret) = &owner_secret {
            debug_assert_eq!(owner_secret.kind(), owner.kind());
        }
        Self {
            key,
            owner,
            owner_secret,
            schema,
        }
    }
    /// The DHT key of the record, by reference
    pub fn ref_key(&self) -> &RecordKey {
        &self.key
    }
    /// The owner's public key, by reference
    pub fn ref_owner(&self) -> &PublicKey {
        &self.owner
    }
    /// The owner's secret key when the record was created by this node, by reference, or `None` when it was opened
    #[must_use]
    pub fn ref_owner_secret(&self) -> Option<&SecretKey> {
        self.owner_secret.as_ref()
    }
    /// The schema associated with the record, by reference
    pub fn ref_schema(&self) -> &DHTSchema {
        &self.schema
    }

    /// The DHT key of the record
    pub fn key(&self) -> RecordKey {
        self.key.clone()
    }

    /// The owner's public key
    pub fn owner(&self) -> PublicKey {
        self.owner.clone()
    }

    /// The owner's secret key when the record was created by this node, or `None` when it was opened
    #[must_use]
    pub fn owner_secret(&self) -> Option<SecretKey> {
        self.owner_secret.clone()
    }

    /// The schema associated with the record
    pub fn schema(&self) -> DHTSchema {
        self.schema.clone()
    }

    /// The owner's public and secret keys as a [KeyPair], or `None` when the secret is unknown
    #[must_use]
    pub fn owner_keypair(&self) -> Option<KeyPair> {
        self.owner_secret
            .as_ref()
            .map(|s| KeyPair::new_from_parts(self.owner.clone(), s.ref_value().clone()))
    }
}
