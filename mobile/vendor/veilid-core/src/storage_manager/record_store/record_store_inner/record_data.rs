use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, GetSize)]
pub struct RecordData {
    signed_value_data: Arc<SignedValueData>,
}

impl RecordData {
    pub fn new(signed_value_data: Arc<SignedValueData>) -> Self {
        Self { signed_value_data }
    }
    pub fn signed_value_data(&self) -> Arc<SignedValueData> {
        self.signed_value_data.clone()
    }
    pub fn data_size(&self) -> usize {
        self.signed_value_data.data_size()
    }
}
