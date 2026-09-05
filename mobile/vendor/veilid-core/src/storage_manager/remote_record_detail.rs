use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, GetSize)]
pub struct RemoteRecordDetail {}

impl RecordDetail for RemoteRecordDetail {
    fn is_new(&self) -> bool {
        true
    }
}
