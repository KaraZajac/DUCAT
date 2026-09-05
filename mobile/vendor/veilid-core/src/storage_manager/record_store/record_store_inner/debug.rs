use super::*;

impl<D> RecordStoreInner<D>
where
    D: RecordDetail,
{
    pub fn debug_records(&self) -> String {
        let mut out = String::new();

        out += &self.record_index.debug();
        out += &self.inbound_transactions.debug();
        out += &self.inbound_watches.debug();

        out
    }

    pub fn debug_record_info(&self, opaque_record_key: &OpaqueRecordKey) -> String {
        let record_info = self
            .peek_record(opaque_record_key, |r| format!("{:#?}", r))
            .unwrap_or("Not found".to_owned());
        let watched_record = match self.inbound_watches.get(&RecordTableKey {
            record_key: opaque_record_key.clone(),
        }) {
            Some(w) => {
                format!("Inbound Watches: {:#?}", w)
            }
            None => "No inbound watches".to_owned(),
        };
        format!("{}\n{}\n", record_info, watched_record)
    }
}
