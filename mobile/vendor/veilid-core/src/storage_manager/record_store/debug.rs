use super::*;

impl<D> RecordStore<D>
where
    D: RecordDetail,
{
    pub fn debug_records(&self) -> String {
        self.inner.lock().debug_records()
    }

    pub fn debug_record_info(&self, opaque_record_key: &OpaqueRecordKey) -> String {
        self.inner.lock().debug_record_info(opaque_record_key)
    }

    pub async fn debug_record_subkey_info(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
    ) -> String {
        match self.peek_subkey(opaque_record_key, subkey, true).await {
            Ok(Some(v)) => {
                format!("{:#?}", v)
            }
            Ok(None) => "Subkey not available".to_owned(),
            Err(e) => format!("{}", e),
        }
    }
}
