use super::*;

#[derive(Debug)]
#[must_use]
pub struct LoadAction {
    subkey_table: TableDB,
    subkey_table_key: SubkeyTableKey,
    opt_cached_record_data: Option<RecordData>,
    peek: bool,
    finished: bool,
}

impl LoadAction {
    pub(super) fn new(
        subkey_table: TableDB,
        subkey_table_key: SubkeyTableKey,
        opt_cached_record_data: Option<RecordData>,
        peek: bool,
    ) -> Self {
        Self {
            subkey_table,
            subkey_table_key,
            opt_cached_record_data,
            peek,
            finished: false,
        }
    }

    pub async fn load(&mut self) -> VeilidAPIResult<Option<RecordData>> {
        if let Some(cached_record_data) = &self.opt_cached_record_data {
            return Ok(Some(cached_record_data.clone()));
        }

        let opt_data = self
            .subkey_table
            .load_json(0, &self.subkey_table_key.bytes())
            .await?;

        self.opt_cached_record_data = opt_data.clone();

        Ok(opt_data)
    }

    pub(super) fn is_peek(&self) -> bool {
        self.peek
    }

    pub(super) fn into_cached_record_data(mut self) -> (SubkeyTableKey, Option<RecordData>) {
        self.finished = true;
        (
            self.subkey_table_key.clone(),
            self.opt_cached_record_data.take(),
        )
    }
}

impl Drop for LoadAction {
    fn drop(&mut self) {
        if !self.finished {
            error!(target:"stor", "LoadAction dropped without being finished: {}", self.subkey_table_key);
        }
    }
}

#[derive(Debug)]
#[must_use]
pub enum LoadActionResult {
    NoRecord,
    NoSubkey {
        descriptor: Arc<SignedValueDescriptor>,
    },
    Subkey {
        descriptor: Arc<SignedValueDescriptor>,
        load_action: LoadAction,
    },
}
