use super::*;

impl_veilid_log_facility!("stor");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineSubkeyWrite {
    /// Safety selection to use when writing this record to the network
    pub safety_selection: SafetySelection,
    /// The subkeys that are queued up needing to be sent to the network in the background
    pub subkeys: ValueSubkeyRangeSet,
    /// The subkeys currently being sent to the network in the background
    #[serde(default)]
    pub subkeys_in_flight: ValueSubkeyRangeSet,
}

impl StorageManager {
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub(super) fn add_offline_subkey_write(
        &self,
        opaque_record_key: OpaqueRecordKey,
        subkey: ValueSubkey,
        safety_selection: SafetySelection,
    ) {
        self.inner
            .lock()
            .offline_subkey_writes
            .entry(opaque_record_key)
            .and_modify(|x| {
                x.subkeys.insert(subkey);
            })
            .or_insert(OfflineSubkeyWrite {
                safety_selection,
                subkeys: ValueSubkeyRangeSet::single(subkey),
                subkeys_in_flight: ValueSubkeyRangeSet::new(),
            });
    }

    /// If we write to a subkey, we first clear out any existing offline write to that subkey.
    /// If the new write succeeds, then this stays cleared
    /// If the new write was offline, then it will add it back in
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub(super) fn remove_offline_subkey_write_inner(
        &self,
        inner: &mut StorageManagerInner,
        opaque_record_key: &OpaqueRecordKey,
        subkey: ValueSubkey,
    ) {
        // Get the offline subkey write record
        match inner.offline_subkey_writes.entry(opaque_record_key.clone()) {
            hashlink::linked_hash_map::Entry::Occupied(mut o) => {
                let finished = {
                    let osw = o.get_mut();

                    // Remove the subkey from the list if it is there
                    osw.subkeys.remove(subkey);

                    // If we have no new work to do, and not still doing work, then this record is done
                    osw.subkeys.is_empty() && osw.subkeys_in_flight.is_empty()
                };
                if finished {
                    veilid_log!(self debug "Dropped offline write {}", opaque_record_key);
                    o.remove();
                    inner.flush_record_waiters.remove(opaque_record_key);
                }
            }
            hashlink::linked_hash_map::Entry::Vacant(_) => {}
        }
    }

    /// When we finish a offline subkey write, we mark subkeys as no longer in-flight
    /// and if we didn't finish all the subkeys they are returned to the list of offline subkeys
    /// so we can try again later. If the data associated with the write is no longer necessary
    /// we can drop it.
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "stor", skip_all)
    )]
    pub(super) fn finish_offline_subkey_writes(
        &self,
        opaque_record_key: &OpaqueRecordKey,
        subkeys_written: ValueSubkeyRangeSet,
        subkeys_still_offline: ValueSubkeyRangeSet,
    ) {
        if !subkeys_written.is_disjoint(&subkeys_still_offline) {
            veilid_log!(self error "subkeys can not be written and still offline");
            return;
        }

        // Get the offline subkey write record
        let mut inner = self.inner.lock();
        match inner.offline_subkey_writes.entry(opaque_record_key.clone()) {
            hashlink::linked_hash_map::Entry::Occupied(mut o) => {
                let finished = {
                    let osw = o.get_mut();

                    // Now any left over are still offline, so merge them with any subkeys that have been added while we were working
                    osw.subkeys = osw.subkeys.union(&subkeys_still_offline);

                    // Remove subkeys that were successfully written from in_flight status
                    osw.subkeys_in_flight = osw.subkeys_in_flight.difference(&subkeys_written);

                    // If we have no new work to do, and not still doing work, then this record is done
                    osw.subkeys.is_empty() && osw.subkeys_in_flight.is_empty()
                };
                if finished {
                    veilid_log!(self debug "offline subkey write finished key {}", opaque_record_key);
                    o.remove();
                    inner.flush_record_waiters.remove(opaque_record_key);
                }
            }
            hashlink::linked_hash_map::Entry::Vacant(_) => {
                veilid_log!(self warn "can't finish missing offline subkey write: ignoring key {}", opaque_record_key);
            }
        }
    }
}
