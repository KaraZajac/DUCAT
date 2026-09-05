use super::*;

/////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, PartialOrd, PartialEq, Eq, Ord, Serialize, Deserialize, GetSize)]
pub struct SignedValueDescriptor {
    #[serde(with = "public_key_try_untyped_vld0")]
    owner: PublicKey,
    #[get_size(size_fn = bytes_size_helper)]
    schema_data: Bytes,
    #[serde(with = "signature_try_untyped_vld0")]
    signature: Signature,
}
impl SignedValueDescriptor {
    pub fn new(owner: PublicKey, schema_data: Bytes, signature: Signature) -> Self {
        Self {
            owner,
            schema_data,
            signature,
        }
    }

    pub async fn validate(
        &self,
        vcrypto: &AsyncCryptoSystemGuard<'_>,
        opaque_record_key: &OpaqueRecordKey,
    ) -> VeilidAPIResult<()> {
        if self.owner.kind() != vcrypto.kind() {
            apibail_parse_error!(
                "wrong kind of owner for signed value descriptor",
                &self.owner
            );
        }
        if self.signature.kind() != vcrypto.kind() {
            apibail_parse_error!(
                "wrong kind of signature for signed value descriptor",
                &self.signature
            );
        }
        // validate signature
        if !vcrypto
            .verify(&self.owner, self.schema_data.clone(), &self.signature)
            .await?
        {
            apibail_parse_error!(
                "failed to validate signature of signed value descriptor",
                &self.signature
            );
        }
        // validate schema bytes
        let _ = DHTSchema::try_from(self.schema_data.as_ref())?;

        // Verify record key matches
        let verify_key = StorageManager::make_opaque_record_key(
            vcrypto,
            self.ref_owner().ref_value(),
            self.schema_data(),
        )
        .await;
        if opaque_record_key != &verify_key {
            apibail_parse_error!("failed to validate record key match", verify_key);
        }

        Ok(())
    }

    pub fn owner(&self) -> PublicKey {
        self.owner.clone()
    }

    pub fn ref_owner(&self) -> &PublicKey {
        &self.owner
    }

    pub fn schema_data(&self) -> &[u8] {
        &self.schema_data
    }

    pub fn schema(&self) -> VeilidAPIResult<DHTSchema> {
        DHTSchema::try_from(self.schema_data.as_ref())
    }

    #[expect(dead_code)]
    pub fn signature(&self) -> Signature {
        self.signature.clone()
    }

    pub fn ref_signature(&self) -> &Signature {
        &self.signature
    }

    pub async fn make_signature(
        owner: PublicKey,
        schema_data: Bytes,
        vcrypto: &AsyncCryptoSystemGuard<'_>,
        owner_secret: SecretKey,
    ) -> VeilidAPIResult<Self> {
        if owner.kind() != vcrypto.kind() {
            apibail_parse_error!(
                "wrong kind of owner for signed value descriptor signature",
                &owner
            );
        }
        // create signature
        let signature = vcrypto
            .sign(&owner, &owner_secret, schema_data.clone())
            .await?;
        Ok(Self {
            owner,
            schema_data,
            signature,
        })
    }

    pub fn cmp_no_sig(&self, other: &Self) -> cmp::Ordering {
        let o = self.owner.cmp(&other.owner);
        if o != cmp::Ordering::Equal {
            return o;
        }
        self.schema_data.cmp(&other.schema_data)
    }
}

impl fmt::Debug for SignedValueDescriptor {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("SignedValueDescriptor")
            .field("owner", &self.owner)
            .field("schema_data", &format!("{:?}", &self.schema_data))
            .field("signature", &self.signature)
            .finish()
    }
}
