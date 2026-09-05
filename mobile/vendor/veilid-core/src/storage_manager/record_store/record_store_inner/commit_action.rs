use super::*;

pub(super) type UncommittedRecordChanges<D> = BTreeMap<RecordTableKey, UncommittedRecordChange<D>>;
pub(super) type UncommittedSubkeyChanges = BTreeMap<SubkeyTableKey, UncommittedSubkeyChange>;

#[derive(Debug)]
pub(super) enum UncommittedRecordChange<D>
where
    D: RecordDetail,
{
    Create {
        /// The record being created
        new_record: Record<D>,
        /// The amount of space for the new record
        new_record_size: u64,
    },

    Update {
        /// The new record data
        new_record: Record<D>,
        /// The amount of space for the new record
        new_record_size: u64,
        /// The old record data
        old_record: Record<D>,
        /// The amount of space for the old record
        old_record_size: u64,
    },

    Delete {
        /// The record data being deleted
        old_record: Record<D>,
        /// The amount of space for the old record
        old_record_size: u64,
        is_lru: bool,
    },
}

#[derive(Debug)]
pub(super) enum UncommittedSubkeyChange {
    Create {
        /// The subkey data being created
        new_data: RecordData,
    },

    Update {
        /// The new subkey data
        new_data: RecordData,

        /// The old subkey data if it was in the cache
        opt_old_data: Option<RecordData>,
    },

    Delete {
        /// The subkey data being deleted if it was in the cache
        opt_old_data: Option<RecordData>,
        is_lru: bool,
    },
}

#[derive(Debug)]
#[must_use]
pub struct CommitAction<D>
where
    D: RecordDetail,
{
    // XXX: Someday these should be the same table
    rt_xact: Option<TableDBTransaction>,
    st_xact: Option<TableDBTransaction>,
    uncommitted_record_changes: Arc<UncommittedRecordChanges<D>>,
    uncommitted_subkey_changes: Arc<UncommittedSubkeyChanges>,

    finished: bool,
    committed: bool,
}

impl<D> CommitAction<D>
where
    D: RecordDetail,
{
    pub(super) fn new(
        rt_xact: TableDBTransaction,
        st_xact: TableDBTransaction,
        uncommitted_record_changes: Arc<UncommittedRecordChanges<D>>,
        uncommitted_subkey_changes: Arc<UncommittedSubkeyChanges>,
    ) -> Self {
        Self {
            rt_xact: Some(rt_xact),
            st_xact: Some(st_xact),
            uncommitted_record_changes,
            uncommitted_subkey_changes,
            finished: false,
            committed: false,
        }
    }

    pub async fn commit(&mut self) -> VeilidAPIResult<()> {
        if self.committed {
            apibail_internal!("commit action already committed");
        }
        self.committed = true;

        let rt_xact = self
            .rt_xact
            .take()
            .ok_or_else(|| VeilidAPIError::internal("rt_xact is dead"))?;
        let st_xact = self
            .st_xact
            .take()
            .ok_or_else(|| VeilidAPIError::internal("st_xact is dead"))?;

        let do_xact = async {
            for (rtk, urc) in self.uncommitted_record_changes.iter() {
                match urc {
                    UncommittedRecordChange::Create {
                        new_record: record,
                        new_record_size: _,
                    } => {
                        rt_xact.store_json(0, &rtk.bytes(), &record).await?;
                    }
                    UncommittedRecordChange::Update {
                        new_record,
                        new_record_size: _,
                        old_record: _,
                        old_record_size: _,
                    } => {
                        rt_xact.store_json(0, &rtk.bytes(), &new_record).await?;
                    }
                    UncommittedRecordChange::Delete {
                        old_record: _,
                        old_record_size: _,
                        is_lru: _,
                    } => rt_xact.delete(0, &rtk.bytes()).await?,
                }
            }
            for (stk, usc) in self.uncommitted_subkey_changes.iter() {
                match usc {
                    UncommittedSubkeyChange::Create { new_data: data } => {
                        st_xact.store_json(0, &stk.bytes(), &data).await?;
                    }
                    UncommittedSubkeyChange::Update {
                        new_data,
                        opt_old_data: _,
                    } => {
                        st_xact.store_json(0, &stk.bytes(), &new_data).await?;
                    }
                    UncommittedSubkeyChange::Delete {
                        opt_old_data: _,
                        is_lru: _,
                    } => st_xact.delete(0, &stk.bytes()).await?,
                }
            }
            Ok(())
        };

        match do_xact.await {
            Ok(()) => {}
            Err(e) => {
                self.rt_xact = Some(rt_xact);
                self.st_xact = Some(st_xact);
                return Err(e);
            }
        }

        if let Err(e) = rt_xact.clone().commit().await {
            self.rt_xact = Some(rt_xact);
            self.st_xact = Some(st_xact);
            return Err(e);
        }
        if let Err(e) = st_xact.clone().commit().await {
            self.rt_xact = Some(rt_xact);
            self.st_xact = Some(st_xact);
            return Err(e);
        }
        Ok(())
    }

    pub(super) fn into_rollback_changes(
        mut self,
    ) -> Option<(
        Arc<UncommittedRecordChanges<D>>,
        Arc<UncommittedSubkeyChanges>,
    )> {
        self.finished = true;

        if self.rt_xact.is_none() || self.st_xact.is_none() {
            return None;
        }

        let rt_xact = self.rt_xact.take().unwrap_or_log();
        rt_xact.rollback();
        let st_xact = self.st_xact.take().unwrap_or_log();
        st_xact.rollback();

        Some((
            self.uncommitted_record_changes.clone(),
            self.uncommitted_subkey_changes.clone(),
        ))
    }
}

impl<D> Drop for CommitAction<D>
where
    D: RecordDetail,
{
    fn drop(&mut self) {
        if !self.finished {
            error!(target:"stor", "CommitAction dropped without being finished: {:#?}", self);
        }
    }
}
