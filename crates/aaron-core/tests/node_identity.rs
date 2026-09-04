use aaron_core::{Node, NodeId, Store, service_fn};
use std::sync::Arc;

#[tokio::test]
async fn test_node_load_or_create_identity_lifecycle() {
    let temp_dir = std::env::temp_dir().join(format!("test_node_identity_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    // 1. First run: Node starts, creates NodeId and saves in "node" keyspace
    let node1 = Node::new("test-identity-node")
        .with_dir_path(&temp_dir)
        .with(service_fn("exit_1", |_ctx| async move { Ok(()) }));

    node1.run().await.unwrap();

    let initial_uuid;
    let initial_inc;
    {
        let store1 = Store::open(&temp_dir).unwrap();
        let node_ks1 = store1.keyspace("node").unwrap();
        let raw_bytes1 = node_ks1.get("id").unwrap().unwrap();
        let saved_node1 = NodeId::from_flatbuffer_bytes(&raw_bytes1).unwrap();
        initial_uuid = saved_node1.id();
        initial_inc = saved_node1.incarnation;
        assert!(initial_inc > 0);
    } // drop store1 to release file lock

    // Wait 5ms so timestamp differs
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    // 2. Second run: Node reopens same directory, reuses UUID and updates incarnation
    let node2 = Node::new("test-identity-node")
        .with_dir_path(&temp_dir)
        .with(service_fn("exit_2", |_ctx| async move { Ok(()) }));

    node2.run().await.unwrap();

    {
        let store2 = Store::open(&temp_dir).unwrap();
        let node_ks2 = store2.keyspace("node").unwrap();
        let raw_bytes2 = node_ks2.get("id").unwrap().unwrap();
        let saved_node2 = NodeId::from_flatbuffer_bytes(&raw_bytes2).unwrap();

        assert_eq!(saved_node2.id(), initial_uuid); // ID remains stable!
        assert!(saved_node2.incarnation >= initial_inc); // Incarnation updated
    }

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[tokio::test]
async fn test_node_with_dir_path_execution() {
    let temp_dir = std::env::temp_dir().join(format!("test_node_with_dir_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let executed_clone = Arc::clone(&executed);

    let node = Node::new("test-identity-node")
        .with_dir_path(&temp_dir)
        .with(service_fn("quick_exit", move |_ctx| {
            let executed = Arc::clone(&executed_clone);
            async move {
                executed.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        }));

    node.run().await.unwrap();

    assert!(executed.load(std::sync::atomic::Ordering::SeqCst));

    // Verify "node" keyspace and store files exist at temp_dir
    let store = Store::open(&temp_dir).unwrap();
    let node_ks = store.keyspace("node").unwrap();
    assert!(node_ks.contains_key("id").unwrap());

    let _ = std::fs::remove_dir_all(temp_dir);
}
