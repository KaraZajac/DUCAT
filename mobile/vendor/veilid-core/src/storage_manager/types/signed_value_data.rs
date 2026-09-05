use super::*;

/////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, GetSize, Hash)]
pub struct SignedValueData {
    value_data: EncryptedValueData,
    signature: Signature,
}
impl SignedValueData {
    pub fn new(encrypted_value_data: EncryptedValueData, signature: Signature) -> Self {
        Self {
            value_data: encrypted_value_data,
            signature,
        }
    }

    pub async fn validate(
        &self,
        owner: &PublicKey,
        subkey: ValueSubkey,
        vcrypto: &AsyncCryptoSystemGuard<'_>,
    ) -> VeilidAPIResult<bool> {
        let writer = self.value_data.writer();
        if vcrypto.kind() != writer.kind() {
            return Ok(false);
        }
        if vcrypto.kind() != self.signature.kind() {
            return Ok(false);
        }

        if let Some(_nonce) = self.value_data.nonce() {
            // new approach, verify the whole capnp blob as is
            let signature_bytes = Self::make_signature_bytes(&self.value_data, owner, subkey)?;
            // validate signature
            vcrypto
                .verify(&writer, signature_bytes, &self.signature)
                .await
        } else {
            // old approach, use make_signature_bytes()
            let signature_bytes =
                Self::legacy_make_signature_bytes(&self.value_data, owner, subkey)?;
            // validate signature
            vcrypto
                .verify(&writer, signature_bytes, &self.signature)
                .await
        }
    }

    pub async fn make_signature(
        value_data: EncryptedValueData,
        owner: &PublicKey,
        subkey: ValueSubkey,
        vcrypto: &AsyncCryptoSystemGuard<'_>,
        writer_secret: &SecretKey,
    ) -> VeilidAPIResult<Self> {
        let writer = value_data.writer();

        let signature = if let Some(_nonce) = value_data.nonce() {
            // new approach, sign the whole capnp blob as is
            let signature_bytes = Self::make_signature_bytes(&value_data, owner, subkey)?;
            // create signature
            vcrypto
                .sign(&writer, writer_secret, signature_bytes)
                .await?
        } else {
            // old approach, no capnp in use
            let signature_bytes = Self::legacy_make_signature_bytes(&value_data, owner, subkey)?;
            // create signature
            vcrypto
                .sign(&writer, writer_secret, signature_bytes)
                .await?
        };

        Ok(Self {
            value_data,
            signature,
        })
    }

    pub fn value_data(&self) -> &EncryptedValueData {
        &self.value_data
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    pub fn data_size(&self) -> usize {
        self.value_data.data_size()
    }

    pub fn total_size(&self) -> usize {
        (mem::size_of::<Self>() - mem::size_of::<EncryptedValueData>())
            + self.value_data.total_size()
    }

    fn legacy_make_signature_bytes(
        value_data: &EncryptedValueData,
        owner: &PublicKey,
        subkey: ValueSubkey,
    ) -> VeilidAPIResult<Bytes> {
        let owner_len;
        let is_vld0 = if owner.kind() == CRYPTO_KIND_VLD0 {
            owner_len = owner.ref_value().len();
            true
        } else {
            owner_len = owner.ref_value().len() + 4;
            false
        };

        let mut signature_bytes =
            BytesMut::with_capacity(owner_len + 4 + 4 + value_data.data().len());

        // Add owner to signature
        if !is_vld0 {
            signature_bytes.extend_from_slice(owner.kind().bytes());
        }
        signature_bytes.extend_from_slice(owner.ref_value());
        // Add subkey to signature
        signature_bytes.extend_from_slice(&subkey.to_le_bytes());
        // Add sequence number to signature
        signature_bytes.extend_from_slice(&u32::from(value_data.seq()).to_le_bytes());
        // Add data to signature
        signature_bytes.extend_from_slice(&value_data.data());

        Ok(signature_bytes.freeze())
    }

    fn make_signature_bytes(
        value_data: &EncryptedValueData,
        owner: &PublicKey,
        subkey: ValueSubkey,
    ) -> VeilidAPIResult<Bytes> {
        let owner_len = owner.ref_value().len();
        let raw_blob_len = value_data.raw_blob().len();
        let mut signature_bytes = BytesMut::with_capacity(4 + 4 + owner_len + 4 + 4 + raw_blob_len);

        // Add raw capnp blob and length to signature
        let raw_blob_len = raw_blob_len as u32;
        signature_bytes.extend_from_slice(&raw_blob_len.to_le_bytes());
        signature_bytes.extend_from_slice(value_data.raw_blob());
        // Add owner to signature
        signature_bytes.extend_from_slice(owner.kind().bytes());
        let owner_len = owner_len as u32;
        signature_bytes.extend_from_slice(&owner_len.to_le_bytes());
        signature_bytes.extend_from_slice(owner.ref_value());
        // Add subkey to signature
        signature_bytes.extend_from_slice(&subkey.to_le_bytes());

        Ok(signature_bytes.freeze())
    }
}

impl fmt::Display for SignedValueData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{{len={}, seq={}, writer={}, data_size={}, total_size={}}}",
            self.value_data().data().len(),
            self.value_data().seq(),
            self.value_data().writer(),
            self.data_size(),
            self.total_size()
        )
    }
}
