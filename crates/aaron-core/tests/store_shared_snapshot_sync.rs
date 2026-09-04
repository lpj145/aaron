use aaron_core::{Context, Env, EventHub, KeyspaceExt, Network, NodeId, Store, Uuid};
use std::sync::Arc;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_store_install_snapshot_shared_state_across_clones() {
    let main_tmp = tempdir().unwrap();
    let snap_tmp = tempdir().unwrap();

    // 1. Create a snapshot directory with Dataset B
    {
        let snap_store = Store::open(&snap_tmp).unwrap();
        snap_store.set("dataset_version", "v2_snapshot").unwrap();
        snap_store.set("cluster_leader", "node_99").unwrap();
        let auth_ks = snap_store.keyspace("auth").unwrap();
        auth_ks.insert("admin", "token_xyz").unwrap();
        snap_store.persist().unwrap();
    }

    // 2. Open active Store with Dataset A
    let active_store = Store::open(&main_tmp).unwrap();
    active_store.set("dataset_version", "v1_original").unwrap();
    active_store.set("temp_key", "will_be_wiped").unwrap();

    // Create Context instances (as would be distributed to multiple supervised services)
    let env = Arc::new(Env::detect());
    let token = CancellationToken::new();
    let ctx_service_1 = Context::new(
        EventHub::new(),
        Network::new(),
        active_store.clone(),
        NodeId::new(Uuid::random(), 1, None),
        env.clone(),
        token.clone(),
    );
    let ctx_service_2 = ctx_service_1.clone();
    let ctx_service_3 = ctx_service_1.clone();

    // Before snapshot: All services see Dataset A
    assert_eq!(
        ctx_service_1.store.get_string("dataset_version").unwrap(),
        Some("v1_original".to_string())
    );
    assert_eq!(
        ctx_service_2.store.get_string("dataset_version").unwrap(),
        Some("v1_original".to_string())
    );

    // 3. Service 2 installs the snapshot in-place
    ctx_service_2
        .store
        .install_snapshot(&snap_tmp)
        .expect("Snapshot installation must succeed");

    // 4. Verify Service 1, 2, and 3 all seamlessly see Dataset B via the shared state!
    assert_eq!(
        ctx_service_1.store.get_string("dataset_version").unwrap(),
        Some("v2_snapshot".to_string()),
        "Service 1 must see new snapshot data without restarting"
    );
    assert_eq!(
        ctx_service_3.store.get_string("cluster_leader").unwrap(),
        Some("node_99".to_string()),
        "Service 3 must see new snapshot data"
    );
    assert_eq!(
        ctx_service_1.store.get("temp_key").unwrap(),
        None,
        "Wiped keys must not exist"
    );

    let auth = ctx_service_1.store.keyspace("auth").unwrap();
    assert_eq!(
        auth.get_string("admin").unwrap(),
        Some("token_xyz".to_string())
    );

    // 5. Subsequent writes work seamlessly
    ctx_service_3
        .store
        .set("post_snapshot_write", "ok")
        .unwrap();
    assert_eq!(
        ctx_service_1
            .store
            .get_string("post_snapshot_write")
            .unwrap(),
        Some("ok".to_string())
    );
}

#[test]
fn test_store_restore_cleans_dirty_destination_remnants() {
    let snap_tmp = tempdir().unwrap();
    let dst_tmp = tempdir().unwrap();

    // 1. Create a snapshot source
    {
        let snap_store = Store::open(&snap_tmp).unwrap();
        snap_store.set("clean_key", "clean_value").unwrap();
        snap_store.persist().unwrap();
    }

    // 2. Populate destination directory with dirty garbage files (e.g. from corrupt run)
    let dirty_file = dst_tmp.path().join("orphaned_corrupt_sst.sst");
    std::fs::write(&dirty_file, b"corrupted_bytes_from_crashed_node").unwrap();
    assert!(dirty_file.exists());

    // 3. Restore snapshot into dirty destination
    let restored = Store::restore(&snap_tmp, &dst_tmp).expect("Restore must succeed");

    // 4. Destination dirty file was wiped prior to restore, clean database opens perfectly
    assert!(
        !dirty_file.exists(),
        "Dirty remnant files must be wiped on restore"
    );
    assert_eq!(
        restored.get_string("clean_key").unwrap(),
        Some("clean_value".to_string())
    );
}
