use node::{KeyspaceExt, ScanOptions, Store};
use std::sync::Arc;

#[test]
fn test_store_open_and_basic_crud() {
    let temp_dir = std::env::temp_dir().join(format!("test_store_crud_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();

    assert!(store.is_empty().unwrap());
    assert_eq!(store.len().unwrap(), 0);

    // Set and Get
    store.set("key1", "value1").unwrap();
    store.set("key2", "value2").unwrap();

    assert!(!store.is_empty().unwrap());
    assert_eq!(store.len().unwrap(), 2);
    assert!(store.contains_key("key1").unwrap());
    assert!(store.contains_key("key2").unwrap());
    assert!(!store.contains_key("key3").unwrap());

    assert_eq!(
        store.get_string("key1").unwrap(),
        Some("value1".to_string())
    );
    assert_eq!(
        store.get_string("key2").unwrap(),
        Some("value2".to_string())
    );
    assert_eq!(store.get_string("key3").unwrap(), None);

    // Overwrite
    store.set("key1", "value_updated").unwrap();
    assert_eq!(
        store.get_string("key1").unwrap(),
        Some("value_updated".to_string())
    );

    // Remove
    store.remove("key1").unwrap();
    assert!(!store.contains_key("key1").unwrap());
    assert_eq!(store.get("key1").unwrap(), None);
    assert_eq!(store.len().unwrap(), 1);

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_keyspaces_isolation() {
    let temp_dir = std::env::temp_dir().join(format!("test_store_ks_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();

    let users = store.keyspace("users").unwrap();
    let orders = store.keyspace("orders").unwrap();

    // Insert same key in different keyspaces
    store.set("100", "default_100").unwrap();
    users.insert("100", "user_alice").unwrap();
    orders.insert("100", "order_laptop").unwrap();

    assert_eq!(
        store.get_string("100").unwrap(),
        Some("default_100".to_string())
    );
    assert_eq!(
        users.get_string("100").unwrap(),
        Some("user_alice".to_string())
    );
    assert_eq!(
        orders.get_string("100").unwrap(),
        Some("order_laptop".to_string())
    );

    // Deleting in one keyspace does not affect others
    orders.remove("100").unwrap();
    assert!(!orders.contains_key("100").unwrap());
    assert_eq!(
        store.get_string("100").unwrap(),
        Some("default_100".to_string())
    );
    assert_eq!(
        users.get_string("100").unwrap(),
        Some("user_alice".to_string())
    );

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_persistence_across_reopen() {
    let temp_dir = std::env::temp_dir().join(format!("test_store_persist_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    {
        let store = Store::open(&temp_dir).unwrap();
        store.set("app_name", "aaron").unwrap();

        let settings = store.keyspace("settings").unwrap();
        settings.insert("theme", "dark").unwrap();
        settings.insert("auto_restart", "true").unwrap();

        store.persist().unwrap();
    }

    // Reopen database from same directory
    {
        let store = Store::open(&temp_dir).unwrap();
        assert_eq!(
            store.get_string("app_name").unwrap(),
            Some("aaron".to_string())
        );

        let settings = store.keyspace("settings").unwrap();
        assert_eq!(
            settings.get_string("theme").unwrap(),
            Some("dark".to_string())
        );
        assert_eq!(
            settings.get_string("auto_restart").unwrap(),
            Some("true".to_string())
        );
    }

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_binary_data() {
    let temp_dir = std::env::temp_dir().join(format!("test_store_bin_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();
    let binary_key = b"\x00\x01\x02";
    let binary_value = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0xFF];

    store.set(binary_key, binary_value.clone()).unwrap();

    let fetched = store.get(binary_key).unwrap().unwrap();
    assert_eq!(&*fetched, &binary_value[..]);

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_fetch_update_rmw() {
    let temp_dir = std::env::temp_dir().join(format!("test_store_update_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();

    // 1. Update on missing key inserts new value
    let prev = store
        .update("counter", |curr| {
            assert!(curr.is_none());
            Some("10")
        })
        .unwrap();
    assert!(prev.is_none());
    assert_eq!(store.get_string("counter").unwrap(), Some("10".to_string()));

    // 2. Update on existing key
    let prev = store
        .update("counter", |curr| {
            let slice = curr.unwrap();
            let val: i32 = std::str::from_utf8(&slice).unwrap().parse().unwrap();
            Some(format!("{}", val + 5))
        })
        .unwrap();
    assert_eq!(prev.as_deref(), Some(&b"10"[..]));
    assert_eq!(store.get_string("counter").unwrap(), Some("15".to_string()));

    // 3. Returning None deletes the key
    let prev = store.update::<_, String>("counter", |_| None).unwrap();
    assert_eq!(prev.as_deref(), Some(&b"15"[..]));
    assert_eq!(store.get("counter").unwrap(), None);

    // 4. Update on missing key returning None stays None
    let prev = store.update::<_, String>("non_existent", |_| None).unwrap();
    assert!(prev.is_none());
    assert_eq!(store.get("non_existent").unwrap(), None);

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_keyspace_rmw() {
    let temp_dir = std::env::temp_dir().join(format!("test_ks_rmw_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();
    let stats = store.keyspace("stats").unwrap();

    stats
        .update("requests", |curr| {
            let n = curr.map_or(0, |s| {
                std::str::from_utf8(&s).unwrap().parse::<u64>().unwrap()
            });
            Some(format!("{}", n + 1))
        })
        .unwrap();

    assert_eq!(stats.get_string("requests").unwrap(), Some("1".to_string()));

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_pagination_cursor_and_prefix() {
    let temp_dir = std::env::temp_dir().join(format!("test_store_page_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();

    // Insert 10 ordered items: user:01 .. user:10
    for i in 1..=10 {
        let key = format!("user:{:02}", i);
        let val = format!("Name_{:02}", i);
        store.set(key, val).unwrap();
    }
    store.set("other:1", "ignore").unwrap();

    // Page 1: limit 4
    let page1 = store
        .scan(ScanOptions::new().prefix("user:").limit(4))
        .unwrap();
    assert_eq!(page1.items.len(), 4);
    assert!(page1.has_more);
    assert_eq!(page1.items[0].key_str().unwrap(), "user:01");
    assert_eq!(page1.items[3].key_str().unwrap(), "user:04");
    assert_eq!(page1.next_cursor.as_deref(), Some(&b"user:04"[..]));

    // Page 2: limit 4 with start_after
    let page2 = store
        .scan(
            ScanOptions::new()
                .prefix("user:")
                .start_after(page1.next_cursor.as_deref().unwrap())
                .limit(4),
        )
        .unwrap();
    assert_eq!(page2.items.len(), 4);
    assert!(page2.has_more);
    assert_eq!(page2.items[0].key_str().unwrap(), "user:05");
    assert_eq!(page2.items[3].key_str().unwrap(), "user:08");

    // Page 3: limit 4 (only 2 left)
    let page3 = store
        .scan(
            ScanOptions::new()
                .prefix("user:")
                .start_after(page2.next_cursor.as_deref().unwrap())
                .limit(4),
        )
        .unwrap();
    assert_eq!(page3.items.len(), 2);
    assert!(!page3.has_more);
    assert_eq!(page3.items[0].key_str().unwrap(), "user:09");
    assert_eq!(page3.items[1].key_str().unwrap(), "user:10");

    // scan_prefix shortcut
    let page_short = store.scan_prefix("user:", None::<&str>, 3).unwrap();
    assert_eq!(page_short.items.len(), 3);
    assert!(page_short.has_more);
    assert_eq!(page_short.items[0].key_str().unwrap(), "user:01");

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_pagination_reverse_and_offset() {
    let temp_dir = std::env::temp_dir().join(format!("test_store_rev_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();

    for i in 1..=5 {
        let key = format!("item:{}", i);
        store.set(key, format!("val_{}", i)).unwrap();
    }

    // Reverse scan
    let page_rev = store
        .scan(ScanOptions::new().prefix("item:").reverse(true).limit(2))
        .unwrap();
    assert_eq!(page_rev.items.len(), 2);
    assert_eq!(page_rev.items[0].key_str().unwrap(), "item:5");
    assert_eq!(page_rev.items[1].key_str().unwrap(), "item:4");
    assert!(page_rev.has_more);

    // Offset scan
    let page_offset = store
        .scan(ScanOptions::new().prefix("item:").offset(2).limit(2))
        .unwrap();
    assert_eq!(page_offset.items.len(), 2);
    assert_eq!(page_offset.items[0].key_str().unwrap(), "item:3");
    assert_eq!(page_offset.items[1].key_str().unwrap(), "item:4");

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_scan_start_from_and_end_at_ranges() {
    let temp_dir = std::env::temp_dir().join(format!("test_store_range_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();

    for ch in ['a', 'b', 'c', 'd', 'e', 'f'] {
        store.set(ch.to_string(), format!("val_{ch}")).unwrap();
    }

    // Scan from 'b' to 'd' inclusive
    let page = store
        .scan(ScanOptions::new().start_from("b").end_at("d").limit(10))
        .unwrap();
    assert_eq!(page.items.len(), 3);
    assert_eq!(page.items[0].key_str().unwrap(), "b");
    assert_eq!(page.items[1].key_str().unwrap(), "c");
    assert_eq!(page.items[2].key_str().unwrap(), "d");
    assert!(!page.has_more);

    // Scan strictly after 'b' up to 'e'
    let page2 = store
        .scan(ScanOptions::new().start_after("b").end_at("e").limit(2))
        .unwrap();
    assert_eq!(page2.items.len(), 2);
    assert_eq!(page2.items[0].key_str().unwrap(), "c");
    assert_eq!(page2.items[1].key_str().unwrap(), "d");
    assert!(page2.has_more);

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_scan_boundaries_and_empty() {
    let temp_dir =
        std::env::temp_dir().join(format!("test_store_empty_bound_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();

    // 1. Scan on empty store
    let page = store.scan(ScanOptions::new()).unwrap();
    assert!(page.items.is_empty());
    assert!(!page.has_more);
    assert!(page.next_cursor.is_none());

    // Insert 2 items
    store.set("k1", "v1").unwrap();
    store.set("k2", "v2").unwrap();

    // 2. Non-matching prefix
    let page_none = store
        .scan(ScanOptions::new().prefix("nonexistent:"))
        .unwrap();
    assert!(page_none.items.is_empty());
    assert!(!page_none.has_more);

    // 3. start_after beyond all keys
    let page_beyond = store.scan(ScanOptions::new().start_after("z")).unwrap();
    assert!(page_beyond.items.is_empty());
    assert!(!page_beyond.has_more);

    // 4. limit: 0
    let page_zero = store.scan(ScanOptions::new().limit(0)).unwrap();
    assert!(page_zero.items.is_empty());
    assert!(page_zero.has_more); // items exist, but 0 returned

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_snapshot_isolation() {
    use fjall::Readable;

    let temp_dir = std::env::temp_dir().join(format!("test_store_snap_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();
    store.set("version", "v1.0").unwrap();

    // Take snapshot at v1.0
    let snapshot = store.snapshot();

    // Modify store afterwards to v2.0
    store.set("version", "v2.0").unwrap();
    store.set("new_key", "hello").unwrap();

    // Store has new values
    assert_eq!(
        store.get_string("version").unwrap(),
        Some("v2.0".to_string())
    );
    assert_eq!(
        store.get_string("new_key").unwrap(),
        Some("hello".to_string())
    );

    // Snapshot retains consistent point-in-time view
    let default_ks = store.default_keyspace();
    let snap_val = snapshot.get(&default_ks, "version").unwrap().unwrap();
    assert_eq!(&*snap_val, b"v1.0");

    let snap_new = snapshot.get(&default_ks, "new_key").unwrap();
    assert!(snap_new.is_none());

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_backup_and_restore() {
    let temp_dir = std::env::temp_dir().join(format!("test_store_src_{}", std::process::id()));
    let backup_dir = std::env::temp_dir().join(format!("test_store_dst_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::remove_dir_all(&backup_dir);

    {
        let store = Store::open(&temp_dir).unwrap();
        store.set("config:site", "mysite.com").unwrap();

        let metrics = store.keyspace("metrics").unwrap();
        metrics.insert("uptime", "99.99").unwrap();

        // Perform backup
        store.backup(&backup_dir).unwrap();

        // Backup to same directory should return error
        let same_dir_err = store.backup(store.path());
        assert!(same_dir_err.is_err());
    }

    // Open restored database from backup directory
    {
        let restored_store = Store::open(&backup_dir).unwrap();
        assert_eq!(
            restored_store.get_string("config:site").unwrap(),
            Some("mysite.com".to_string())
        );

        let metrics = restored_store.keyspace("metrics").unwrap();
        assert_eq!(
            metrics.get_string("uptime").unwrap(),
            Some("99.99".to_string())
        );
    }

    let _ = std::fs::remove_dir_all(temp_dir);
    let _ = std::fs::remove_dir_all(backup_dir);
}

#[tokio::test]
async fn test_store_concurrent_rmw_atomic_counter() {
    let temp_dir =
        std::env::temp_dir().join(format!("test_store_atomic_concur_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Arc::new(Store::open(&temp_dir).unwrap());
    let mut handles = Vec::new();

    for _ in 0..50 {
        let store_clone = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let _ = store_clone
                .update("shared_counter", |curr| {
                    let val = match curr {
                        Some(s) => std::str::from_utf8(&s).unwrap().parse::<i32>().unwrap(),
                        None => 0,
                    };
                    Some(format!("{}", val + 1))
                })
                .unwrap();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_install_snapshot_in_place() {
    let active_dir = std::env::temp_dir().join(format!("test_store_active_{}", std::process::id()));
    let snapshot_dir =
        std::env::temp_dir().join(format!("test_store_snap_src_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&active_dir);
    let _ = std::fs::remove_dir_all(&snapshot_dir);

    // 1. Create a snapshot source with initial data
    {
        let snap_store = Store::open(&snapshot_dir).unwrap();
        snap_store.set("cluster_epoch", "100").unwrap();
        let auth = snap_store.keyspace("auth").unwrap();
        auth.insert("admin", "secret").unwrap();
        snap_store.persist().unwrap();
    }

    // 2. Create an active store with different data
    let active_store = Store::open(&active_dir).unwrap();
    active_store.set("cluster_epoch", "1").unwrap();
    active_store.set("stale_key", "old").unwrap();

    // 3. Install the snapshot in-place into active_store
    active_store.install_snapshot(&snapshot_dir).unwrap();

    // 4. Verify active_store now reflects the snapshot state
    assert_eq!(
        active_store.get_string("cluster_epoch").unwrap(),
        Some("100".to_string())
    );
    assert_eq!(active_store.get("stale_key").unwrap(), None);

    let auth = active_store.keyspace("auth").unwrap();
    assert_eq!(
        auth.get_string("admin").unwrap(),
        Some("secret".to_string())
    );

    // 5. Verify active_store can continue writing after install_snapshot
    active_store.set("cluster_epoch", "101").unwrap();
    assert_eq!(
        active_store.get_string("cluster_epoch").unwrap(),
        Some("101".to_string())
    );

    let _ = std::fs::remove_dir_all(active_dir);
    let _ = std::fs::remove_dir_all(snapshot_dir);
}

#[test]
fn test_store_restore_to_new_path() {
    let src_dir =
        std::env::temp_dir().join(format!("test_store_restore_src_{}", std::process::id()));
    let dst_dir =
        std::env::temp_dir().join(format!("test_store_restore_dst_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&src_dir);
    let _ = std::fs::remove_dir_all(&dst_dir);

    {
        let store = Store::open(&src_dir).unwrap();
        store.set("node_id", "node_42").unwrap();
        store.persist().unwrap();
    }

    // Restore to brand new path
    let restored = Store::restore(&src_dir, &dst_dir).unwrap();
    assert_eq!(
        restored.get_string("node_id").unwrap(),
        Some("node_42".to_string())
    );

    let _ = std::fs::remove_dir_all(src_dir);
    let _ = std::fs::remove_dir_all(dst_dir);
}

#[test]
fn test_store_write_batch() {
    let temp_dir = std::env::temp_dir().join(format!("test_store_batch_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();
    let events = store.keyspace("events").unwrap();
    let default_ks = store.default_keyspace();

    let mut batch = store.batch();
    batch.insert(&default_ks, "batch_key_1", "val1");
    batch.insert(&default_ks, "batch_key_2", "val2");
    batch.insert(&events, "evt:1", "connected");
    batch.insert(&events, "evt:2", "authenticated");

    // Before commit, keys should not exist
    assert_eq!(store.get("batch_key_1").unwrap(), None);

    // Commit batch atomically
    batch.commit().unwrap();

    // After commit, keys are available
    assert_eq!(
        store.get_string("batch_key_1").unwrap(),
        Some("val1".to_string())
    );
    assert_eq!(
        store.get_string("batch_key_2").unwrap(),
        Some("val2".to_string())
    );
    assert_eq!(
        events.get_string("evt:1").unwrap(),
        Some("connected".to_string())
    );
    assert_eq!(
        events.get_string("evt:2").unwrap(),
        Some("authenticated".to_string())
    );

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_store_builder_custom_tuning() {
    let temp_dir = std::env::temp_dir().join(format!("test_store_builder_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::builder(&temp_dir)
        .cache_size(32 * 1024 * 1024) // 32 MB block cache
        .worker_threads(2)
        .max_cached_files(Some(64))
        .open()
        .unwrap();

    store.set("tuned_key", "tuned_val").unwrap();
    assert_eq!(
        store.get_string("tuned_key").unwrap(),
        Some("tuned_val".to_string())
    );

    let _ = std::fs::remove_dir_all(temp_dir);
}
