use futures_util::StreamExt as _;

use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, GetSize)]
pub(in crate::storage_manager) struct RecordSnapshot {
    all_value_data: Vec<Option<Arc<SignedValueData>>>,
}

impl RecordSnapshot {
    pub fn new(all_value_data: Vec<Option<Arc<SignedValueData>>>) -> Self {
        Self { all_value_data }
    }
    pub fn subkey_value_data(
        &self,
        subkey: ValueSubkey,
    ) -> VeilidAPIResult<Option<Arc<SignedValueData>>> {
        let subkey_index = usize::try_from(subkey).map_err(VeilidAPIError::internal)?;
        if subkey_index >= self.all_value_data.len() {
            apibail_invalid_argument!("subkey out of range", "subkey", subkey)
        }
        Ok(self.all_value_data[subkey_index].clone())
    }

    pub fn seq(&self, subkey: ValueSubkey) -> VeilidAPIResult<ValueSeqNum> {
        let subkey_index = usize::try_from(subkey).map_err(VeilidAPIError::internal)?;
        if subkey_index >= self.all_value_data.len() {
            apibail_invalid_argument!("subkey out of range", "subkey", subkey)
        }
        Ok(self.all_value_data[subkey_index]
            .as_ref()
            .map(|svd| svd.value_data().seq())
            .unwrap_or_default())
    }

    pub fn seqs(&self) -> Vec<ValueSeqNum> {
        self.all_value_data
            .iter()
            .map(|opt_svd| {
                opt_svd
                    .as_ref()
                    .map(|svd| svd.value_data().seq())
                    .unwrap_or_default()
            })
            .collect()
    }
}

impl<D> RecordStore<D>
where
    D: RecordDetail,
{
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub async fn prepare_snapshot_locks(
        &self,
        opaque_record_keys: Vec<OpaqueRecordKey>,
    ) -> RecordStoreRecordsLockGuard {
        self.record_store_lock_table
            .lock_records(opaque_record_keys, RecordStoreRecordLockPurpose::Snapshot)
            .await
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all, err)
    )]
    pub async fn snapshot_record_locked(
        &self,
        record_lock: &RecordStoreRecordLockGuard,
    ) -> VeilidAPIResult<Option<Arc<RecordSnapshot>>> {
        let opaque_record_key = record_lock.record();

        // Get all load actions for the snapshot
        let mut all_value_load_actions = {
            // Perform snapshot all inside one synchronous lock to ensure state is consistent
            let mut inner = self.inner.lock();

            let Some((max_subkey, stored_subkeys)) = inner
                .with_record(&opaque_record_key, |record| {
                    (record.max_subkey(), record.stored_subkeys().clone())
                })?
            else {
                // Record not available
                return Ok(None);
            };

            // Snapshot all subkeys
            let mut all_value_load_actions: Vec<Option<LoadAction>> =
                Vec::with_capacity(max_subkey as usize + 1);

            // Prepare all load actions
            let mut opt_prepare_error = None;
            for subkey in 0..=max_subkey {
                if !stored_subkeys.contains(subkey) {
                    all_value_load_actions.push(None);
                    continue;
                }

                let load_action_result =
                    match inner.prepare_get_load_action(&opaque_record_key, subkey) {
                        Ok(load_action_result) => load_action_result,
                        Err(e) => {
                            opt_prepare_error = Some(e);
                            break;
                        }
                    };

                match load_action_result {
                    LoadActionResult::NoRecord => {
                        opt_prepare_error = Some(VeilidAPIError::internal("Should not get a no-record result here since it was asserted the record existed earlier"));
                        break;
                    }
                    LoadActionResult::NoSubkey { descriptor: _ } => {
                        opt_prepare_error = Some(VeilidAPIError::internal("Should not get a no-subkey result here since it was listed in stored_subkeys"));
                        break;
                    }
                    LoadActionResult::Subkey {
                        descriptor: _,
                        load_action,
                    } => {
                        all_value_load_actions.push(Some(load_action));
                    }
                }
            }

            if let Some(prepare_error) = opt_prepare_error {
                // Finish all load actions
                {
                    let mut inner = self.inner.lock();
                    for load_action in all_value_load_actions.into_iter().flatten() {
                        inner.finish_load_action(load_action);
                    }
                }
                return Err(prepare_error);
            }

            all_value_load_actions
        };

        // Make array to store load action output
        let mut all_value_data: Vec<Option<Arc<SignedValueData>>> =
            vec![None; all_value_load_actions.len()];

        {
            // Enqueue all load actions
            let mut unord = FuturesUnordered::<
                Pin<
                    Box<
                        dyn Future<Output = VeilidAPIResult<(usize, Option<Arc<SignedValueData>>)>>
                            + Send,
                    >,
                >,
            >::new();

            for (n, opt_load_action) in all_value_load_actions.iter_mut().enumerate() {
                if let Some(load_action) = opt_load_action {
                    unord.push(pin_dyn_future!(async move {
                        let res = load_action.load().await;
                        let opt_value = res?.map(|x| x.signed_value_data());
                        Ok((n, opt_value))
                    }));
                }
            }

            // Wait for all load actions
            while let Some(res) = unord.next().await {
                match res {
                    Ok((n, opt_value)) => {
                        all_value_data[n] = opt_value;
                    }
                    Err(e) => {
                        // Fail out of this on the first error we see
                        return Err(e);
                    }
                }
            }
        }

        // Finish all load actions
        {
            let mut inner = self.inner.lock();
            for load_action in all_value_load_actions.into_iter().flatten() {
                inner.finish_load_action(load_action);
            }
        }

        let out = Arc::new(RecordSnapshot::new(all_value_data));

        Ok(Some(out))
    }
}
