use super::*;

impl StorageManager {
    /// Get the full record key for an opened OpaqueRecordKey
    ///
    /// Includes an encryption key if available.
    /// Opaque record keys must have been opened with their full record key in order to be read.
    pub(super) fn get_record_key_for_opaque_record_key(
        &self,
        opaque_record_key: &OpaqueRecordKey,
    ) -> VeilidAPIResult<RecordKey> {
        let inner = self.inner.lock();
        let Some(opened_record) = inner.opened_records.get(opaque_record_key) else {
            apibail_generic!("record must be open to resolve opaque record key");
        };
        let encryption_key = opened_record.encryption_key();
        let record_key = RecordKey::from_opaque(opaque_record_key.clone(), encryption_key);
        Ok(record_key)
    }

    /// Encrypt value data if the record key contains an encryption key.
    /// Leave it unchanged otherwise.
    pub(super) async fn maybe_encrypt_value_data(
        &self,
        record_key: &RecordKey,
        value_data: &ValueData,
    ) -> VeilidAPIResult<EncryptedValueData> {
        if let Some(encryption_key) = record_key.ref_value().ref_encryption_key() {
            let crypto = self.registry.crypto();

            let Some(vcrypto) = crypto.get_async(record_key.kind()) else {
                apibail_generic!("decrypt_value_data: unsupported crypto kind")
            };

            let nonce = vcrypto.random_nonce().await;
            let encryption_key = SharedSecret::new(record_key.kind(), encryption_key.clone());

            let in_data = value_data.data_bytes();
            let out_data = BytesMut::zeroed(value_data.data_bytes().len());

            let out_data = vcrypto
                .crypt_b2b_no_auth(in_data, out_data, 0, &nonce, &encryption_key)
                .await?;

            Ok(EncryptedValueData::new(
                value_data.seq(),
                out_data.freeze(),
                value_data.writer(),
                Some(nonce),
            )?)
        } else {
            Ok(EncryptedValueData::new(
                value_data.seq(),
                value_data.data_bytes(),
                value_data.writer(),
                None,
            )?)
        }
    }

    /// Decrypt value data if the record key contains an encryption key and value data contains nonce.
    /// Leave data unchanged if both are none.
    /// Returns error if either encryption key or nonce is None.
    pub(super) async fn maybe_decrypt_value_data(
        &self,
        record_key: &RecordKey,
        encrypted_value_data: &EncryptedValueData,
    ) -> VeilidAPIResult<ValueData> {
        match (
            record_key.ref_value().ref_encryption_key(),
            encrypted_value_data.nonce(),
        ) {
            (Some(encryption_key), Some(nonce)) => {
                let crypto = self.registry.crypto();

                let Some(vcrypto) = crypto.get_async(record_key.kind()) else {
                    apibail_generic!("cannot decrypt value data: unsupported crypto kind")
                };

                let data = encrypted_value_data.data().to_vec();
                let encryption_key = SharedSecret::new(record_key.kind(), encryption_key.clone());

                let in_data = Bytes::from(data);
                let out_data = BytesMut::zeroed(in_data.len());
                let out_data = vcrypto
                    .crypt_b2b_no_auth(in_data, out_data, 0, &nonce, &encryption_key)
                    .await?;
                Ok(ValueData::new_with_seq(
                    encrypted_value_data.seq(),
                    out_data.freeze(),
                    encrypted_value_data.writer(),
                )?)
            }
            (None, None) => Ok(ValueData::new_with_seq(
                encrypted_value_data.seq(),
                encrypted_value_data.data(),
                encrypted_value_data.writer(),
            )?),
            (Some(_), None) => {
                // Should not happen in normal circumstances
                apibail_generic!("cannot decrypt value data: missing nonce")
            }
            (None, Some(_)) => {
                // Should not happen in normal circumstances
                apibail_generic!("cannot decrypt value data: missing encryption key")
            }
        }
    }
}
