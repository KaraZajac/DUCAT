use super::*;
use crate::tests::*;

fn add_mock_data(routing_table: &RoutingTable) {
    let pi = fix_peer_info(
        routing_table,
        fix_crypto_info_list(true),
        fix_crypto_info_list_secrets(),
    )
    .expect("should be valid");
    routing_table
        .register_node_with_peer_info(Arc::new(pi), false)
        .expect("should register");

    let _ = fix_peer_info(
        routing_table,
        fix_crypto_info_list(false),
        fix_crypto_info_list_secrets(),
    )
    .expect_err("should be invalid");

    let _ = fix_peer_info(
        routing_table,
        fix_crypto_info_list(true),
        SecretKeyGroup::new(),
    )
    .expect_err("should be missing a secret key");

    let pi3 =
        fix_unsigned_peer_info(routing_table, fix_crypto_info_list(true)).expect("should be valid");
    assert!(pi3.signatures().is_empty(), "should have no signatures");

    let _ = routing_table
        .register_node_with_peer_info(Arc::new(pi3.clone()), false)
        .expect_err("should fail with only no signatures");

    let _ = routing_table
        .register_node_with_peer_info(Arc::new(pi3), true)
        .expect("should succeed with allow_invalid");
}

pub async fn test_routingtable_buckets_round_trip() {
    let original_registry = mock_registry::init("a").await;
    let copy_registry = mock_registry::init("b").await;

    // Wrap to close lifetime of 'inner' which is borrowed here so terminate() can succeed
    // (it also .write() locks routing table inner)
    {
        let original = original_registry.routing_table();
        let copy = copy_registry.routing_table();

        add_mock_data(&original);

        let (serialized_bucket_map, all_entry_bytes) = original.serialized_buckets();

        RoutingTable::populate_routing_table_inner(
            &mut copy.inner.write(),
            serialized_bucket_map,
            all_entry_bytes,
        )
        .unwrap();

        let original_inner = &*original.inner.read();
        let copy_inner = &*copy.inner.read();

        let original_crypto_kinds: Vec<_> = original_inner.buckets.keys().clone().collect();
        let copy_crypto_kinds: Vec<_> = copy_inner.buckets.keys().clone().collect();

        assert_eq!(original_crypto_kinds.len(), copy_crypto_kinds.len());

        for crypto in original_crypto_kinds {
            // The same keys are present in the original and copy RoutingTables.
            let original_buckets = original_inner.buckets.get(crypto).unwrap();
            let copy_buckets = copy_inner.buckets.get(crypto).unwrap();

            // Recurse into RoutingTable.inner.buckets
            for (left_bucket, right_bucket) in original_buckets.iter().zip(copy_buckets.iter()) {
                // Recurse into RoutingTable.inner.buckets.entries
                for ((left_node_id, left_entry), (right_node_id, right_entry)) in
                    left_bucket.entries().zip(right_bucket.entries())
                {
                    assert_eq!(left_node_id, right_node_id);

                    let s = left_entry.with(|e| serialize_json(e));
                    let s2 = right_entry.with(|e| serialize_json(e));

                    assert_eq!(s, s2);
                }
            }
        }
    }

    // Even if these are mocks, we should still practice good hygiene.
    mock_registry::terminate(original_registry).await;
    mock_registry::terminate(copy_registry).await;
}

// REPRO: a routing table persisted with an entry that has no node ids (corrupt/legacy
// on-disk data) must not load that entry into a bucket. Pre-fix, the empty entry is
// bucketed and the first `best_node_id()` caller panics at startup.
pub async fn test_load_rejects_entries_without_node_ids() {
    let original_registry = mock_registry::init("a").await;
    let copy_registry = mock_registry::init("b").await;
    {
        let original = original_registry.routing_table();
        add_mock_data(&original);

        let (serialized_bucket_map, all_entry_bytes) = original.serialized_buckets();
        assert!(
            !all_entry_bytes.is_empty(),
            "should have at least one entry"
        );

        // Simulate corrupt/legacy data: blank out node_ids on every persisted entry.
        let tampered: Vec<Vec<u8>> = all_entry_bytes
            .iter()
            .map(|eb| {
                let mut v: serde_json::Value = deserialize_json_bytes(eb).unwrap();
                v["node_ids"] = serde_json::json!([]);
                serialize_json_bytes(v)
            })
            .collect();

        let copy = copy_registry.routing_table();
        RoutingTable::populate_routing_table_inner(
            &mut copy.inner.write(),
            serialized_bucket_map,
            tampered,
        )
        .unwrap();

        // INVARIANT: no entry may be in a bucket without a best node id. Calling
        // best_node_id() on every bucketed entry — exactly what startup code does —
        // must not hit the `expect_or_log("all entries must have one valid node id")`
        // at bucket_entry/mod.rs:370. Pre-fix, the blanked entries are bucketed and
        // this panics there.
        let copy_inner = &*copy.inner.read();
        for buckets in copy_inner.buckets.values() {
            for bucket in buckets {
                for (_k, entry) in bucket.entries() {
                    let _ = entry.with(|e| e.best_node_id());
                }
            }
        }
    }
    mock_registry::terminate(original_registry).await;
    mock_registry::terminate(copy_registry).await;
}

// REPRO: the lookup/kick race. `lookup_node_id` clones an entry's Arc under the inner
// read lock then bumps ref_count AFTER releasing it; a kick in that window observes
// ref_count==0 and evicts the entry. This reproduces the resulting state directly: an
// entry kicked while unreferenced, then handed to a NodeRef. Pre-fix, kick empties the
// entry and `best_node_id()` panics.
pub async fn test_kick_preserves_best_node_id() {
    let registry = mock_registry::init("a").await;
    {
        let rt = registry.routing_table();
        add_mock_data(&rt);

        // Clone an entry's Arc the way lookup_node_id does (ref_count stays 0), then
        // drop the lock.
        let (kind, bucket_idx, entry) = {
            let inner = rt.inner.read();
            let mut found = None;
            'outer: for (ck, buckets) in inner.buckets.iter() {
                for (i, bucket) in buckets.iter().enumerate() {
                    if let Some((_k, e)) = bucket.entries().next() {
                        found = Some((*ck, i, e.clone()));
                        break 'outer;
                    }
                }
            }
            found.expect("should have a registered entry")
        };

        // The race window: a kick evicts this entry while it is still unreferenced.
        {
            let mut inner = rt.inner.write();
            inner.buckets.get_mut(&kind).unwrap()[bucket_idx]
                .kick(0, &BTreeSet::<BareNodeId>::new());
        }

        // The deferred NodeRef::new now bumps ref_count and the caller uses the node.
        // best_node_id() routes to bucket_entry/mod.rs:370's expect_or_log; pre-fix the
        // kick emptied the entry and this panics there ("all entries must have one
        // valid node id").
        let nr = NodeRef::new(rt.registry(), entry.clone());
        let _ = nr.best_node_id();
    }
    mock_registry::terminate(registry).await;
}

// replace_node_ids must be atomic: a rejected replacement (no valid node id) leaves the
// entry's node ids and best node id unchanged. This is what lets create_node_ref reject a
// peer info update in its entirety when its node ids can't be applied.
pub async fn test_replace_node_ids_rejects_without_valid_id() {
    let registry = mock_registry::init("a").await;
    {
        let rt = registry.routing_table();
        add_mock_data(&rt);

        let entry = {
            let inner = rt.inner.read();
            let mut found = None;
            'outer: for (_ck, buckets) in inner.buckets.iter() {
                for bucket in buckets {
                    if let Some((_k, e)) = bucket.entries().next() {
                        found = Some(e.clone());
                        break 'outer;
                    }
                }
            }
            found.expect("should have a registered entry")
        };

        let before_ids = entry.with(|e| e.node_ids());
        let before_best = entry.with(|e| e.best_node_id());

        // Replacing with a set that has no valid node id must be rejected, atomically.
        let result = entry.with_mut(|e| e.replace_node_ids(&[]));
        assert!(result.is_err(), "replace with no valid node id must fail");

        assert_eq!(
            entry.with(|e| e.node_ids()),
            before_ids,
            "node ids must be unchanged on rejection"
        );
        assert_eq!(
            entry.with(|e| e.best_node_id()),
            before_best,
            "best node id must be unchanged on rejection"
        );
    }
    mock_registry::terminate(registry).await;
}

pub async fn test_round_trip_peerinfo() {
    let registry = mock_registry::init("a").await;
    let routing_table = registry.routing_table();

    let pi = fix_peer_info(
        &routing_table,
        fix_crypto_info_list(true),
        fix_crypto_info_list_secrets(),
    )
    .expect("should be valid");

    let s = serialize_json(&pi);
    let pi2 = deserialize_json(&s).expect("Should deserialize");
    let s2 = serialize_json(&pi2);

    assert_eq!(pi, pi2);
    assert_eq!(s, s2);
}
