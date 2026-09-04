use aaron_core::{KeyspaceExt, Store, Uuid};
use aaron_shard::{Router, ShardHandle, ShardKey, ShardPlacement};
use tempfile::tempdir;

#[test]
fn test_stage4_deterministic_routing_distribution() {
    let router = Router::new(16);
    assert_eq!(router.total_shards(), 16);

    // Consistency check: repeatedly hashing the same key produces the identical shard ID
    let k1 = b"account_balance:0042";
    let shard_a = router.route(k1);
    let shard_b = router.route(k1);
    assert_eq!(shard_a, shard_b);
    assert!(shard_a < 16);

    // Distribution check across 16 shards
    let mut distribution = std::collections::BTreeMap::new();
    for i in 0..10_000 {
        let key = format!("user_wallet:{i}");
        let s = router.route_str(&key);
        *distribution.entry(s).or_insert(0usize) += 1;
    }

    // All 16 shards must receive assignments with reasonable uniformity
    assert_eq!(distribution.len(), 16);
    for (_shard_id, count) in distribution {
        assert!(count > 400 && count < 800, "Unexpected distribution skew: {count}");
    }
}

#[tokio::test]
async fn test_stage4_shard_handle_lookup_route() {
    let node_primary = Uuid::random();
    let node_rep1 = Uuid::random();
    let node_rep2 = Uuid::random();

    let handle = ShardHandle::new(node_primary, 8);

    // Pre-populate placements for 8 shards under service "treasurer"
    for s in 0..8 {
        let placement = ShardPlacement::with_service(
            "treasurer",
            s,
            node_primary,
            vec![node_rep1, node_rep2],
            10,
        );
        handle.update_placement(placement).await;
    }

    // User-space route lookup
    let user_key = b"tx_transfer_99482";
    let target_shard = handle.route_key(user_key).await;
    assert!(target_shard < 8);

    let route_info = handle.lookup_route("treasurer", user_key).await;
    assert!(route_info.is_some());
    let placement = route_info.unwrap();
    assert_eq!(placement.service_name, "treasurer");
    assert_eq!(placement.shard_id, target_shard);
    assert_eq!(placement.primary, node_primary);
    assert_eq!(placement.replicas, vec![node_rep1, node_rep2]);
}

#[test]
fn test_stage4_big_endian_lsm_key_ordering_and_prefix_scan() {
    let tmp = tempdir().unwrap();
    let store = Store::open(&tmp).unwrap();
    let ks = store.keyspace("data").unwrap();

    // Numerical shard IDs in arbitrary insertion order
    let shards: Vec<u16> = vec![10, 2, 256, 0, 1, 11, 9, 255, 65535];

    for &shard_id in &shards {
        for item_idx in 1..=3 {
            let user_key = format!("item_{item_idx}");
            let encoded_key = ShardKey::encode_u16(shard_id, user_key.as_bytes());
            let value = format!("val_shard_{shard_id}_item_{item_idx}");
            ks.insert(encoded_key, value.as_bytes()).unwrap();
        }
    }

    // Full scan over keyspace: entries MUST be ordered by Big-Endian numeric shard ID
    let all_items = ks.scan_prefix(b"", None::<&[u8]>, 100).unwrap();
    assert_eq!(all_items.items.len(), shards.len() * 3);

    let mut scanned_shard_ids = Vec::new();
    for kv in &all_items.items {
        let (shard_id, _raw_key) = ShardKey::decode_u16(&kv.key).unwrap();
        if scanned_shard_ids.last() != Some(&shard_id) {
            scanned_shard_ids.push(shard_id);
        }
    }

    // Expected numeric order
    let expected_order = vec![0, 1, 2, 9, 10, 11, 255, 256, 65535];
    assert_eq!(
        scanned_shard_ids, expected_order,
        "LSM byte order must strictly follow Big-Endian numeric order"
    );

    // Prefix scan: Scanning specifically for Shard 10 MUST only return Shard 10 items
    let shard_10_prefix = ShardKey::prefix_u16(10);
    let shard_10_page = ks.scan_prefix(&shard_10_prefix, None::<&[u8]>, 10).unwrap();
    assert_eq!(shard_10_page.items.len(), 3);

    for kv in shard_10_page.items {
        let (shard_id, raw_key) = ShardKey::decode_u16(&kv.key).unwrap();
        assert_eq!(shard_id, 10);
        let raw_str = std::str::from_utf8(raw_key).unwrap();
        assert!(raw_str.starts_with("item_"));
    }
}

#[test]
fn test_stage4_zero_padded_raft_metadata_keys_order() {
    let tmp = tempdir().unwrap();
    let store = Store::open(&tmp).unwrap();
    let ks = store.keyspace("control-plane").unwrap();

    // Insert keys in disordered fashion
    let test_ids = vec![10, 2, 100, 1, 0, 20, 3];
    for &id in &test_ids {
        let key = format!("shards/treasurer/{id:05}");
        ks.insert(key.as_bytes(), b"dummy_placement").unwrap();
    }

    let page = ks.scan_prefix(b"shards/treasurer/", None::<&[u8]>, 50).unwrap();
    let scanned_keys: Vec<String> = page
        .items
        .into_iter()
        .map(|kv| String::from_utf8(kv.key.to_vec()).unwrap())
        .collect();

    let expected_keys = vec![
        "shards/treasurer/00000".to_string(),
        "shards/treasurer/00001".to_string(),
        "shards/treasurer/00002".to_string(),
        "shards/treasurer/00003".to_string(),
        "shards/treasurer/00010".to_string(),
        "shards/treasurer/00020".to_string(),
        "shards/treasurer/00100".to_string(),
    ];

    assert_eq!(scanned_keys, expected_keys);
}
