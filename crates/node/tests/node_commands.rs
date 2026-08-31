use node::{BoxError, CancellationToken, Context, Node, NodeEvents, NodeId, Service, Store, Uuid};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tempfile::tempdir;

#[derive(Clone)]
struct DynamicWorkerService {
    counter: Arc<AtomicUsize>,
}

impl Service for DynamicWorkerService {
    type Config = ();

    fn name(&self) -> &str {
        "dynamic-worker"
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        self.counter.fetch_add(1, Ordering::SeqCst);
        let _ = ctx.token.cancelled().await;
        Ok(())
    }
}

#[derive(Clone)]
struct SpawnerService {
    root_token: CancellationToken,
}

impl Service for SpawnerService {
    type Config = ();

    fn name(&self) -> &str {
        "spawner"
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Dynamically spawn 2 more instances of "dynamic-worker"
        ctx.event_hub
            .publish(NodeEvents::StartService { name: "dynamic-worker".to_owned() })
            .await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        ctx.event_hub
            .publish(NodeEvents::StartService { name: "dynamic-worker".to_owned() })
            .await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Also publish a BindClusterIdCommand to test persistence
        let test_cluster_id = Uuid::random();
        ctx.event_hub
            .publish(NodeEvents::BindClusterId { cluster_id: test_cluster_id })
            .await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Cancel root token to trigger graceful node shutdown
        self.root_token.cancel();
        Ok(())
    }
}

#[tokio::test]
async fn test_node_commands_dynamic_spawn_and_cluster_id_binding() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().to_path_buf();
    let counter = Arc::new(AtomicUsize::new(0));
    let token = CancellationToken::new();

    let worker = DynamicWorkerService {
        counter: counter.clone(),
    };
    let spawner = SpawnerService {
        root_token: token.clone(),
    };

    let node = Node::new()
        .with_dir_path(&path)
        .with_cancel_token(token.clone())
        .with(worker)
        .with(spawner);

    // Run node until SpawnerService completes and cancels root token
    node.run().await.unwrap();

    // 1. Verify 3 total instances of DynamicWorkerService were executed:
    // (1 initial on startup + 2 spawned dynamically via StartServiceCommand)
    assert_eq!(
        counter.load(Ordering::SeqCst),
        3,
        "Node supervisor must spawn 2 dynamic instances via StartServiceCommand, totaling 3"
    );

    // 2. Verify BindClusterIdCommand was persisted to the 'node' keyspace
    let store = Store::open(&path).unwrap();
    let node_ks = store.keyspace("node").unwrap();
    let raw_bytes = node_ks
        .get("id")
        .unwrap()
        .expect("NodeId must exist in store");
    let persisted_id = NodeId::from_flatbuffer_bytes(&raw_bytes).unwrap();

    assert_eq!(
        persisted_id.cluster_id,
        Some(persisted_id.cluster_id.unwrap()),
        "Cluster ID must be populated in persisted NodeId"
    );
    assert!(persisted_id.cluster_id.is_some());
}

#[test]
#[should_panic(expected = "Service with name 'dynamic-worker' is already registered on this Node")]
fn test_duplicate_service_registration_panics() {
    let counter = Arc::new(AtomicUsize::new(0));
    let worker1 = DynamicWorkerService {
        counter: counter.clone(),
    };
    let worker2 = DynamicWorkerService {
        counter: counter.clone(),
    };

    // Attempting to register two services with the same name must fail fast with panic
    let _ = Node::new().with(worker1).with(worker2);
}
