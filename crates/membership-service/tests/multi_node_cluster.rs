use membership_service::{MembershipConfig, MembershipService};
use node::{Context, Env, EventHub, Network, NodeId, Service, Store, Uuid};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

async fn make_test_context(token: CancellationToken) -> (Context, tempfile::TempDir) {
    let tmp = tempdir().unwrap();
    let store = Store::open(&tmp).unwrap();
    let network = Network::new();
    let event_hub = EventHub::new();
    let env = Arc::new(Env::detect());
    let identity = NodeId::new(Uuid::random(), 1, None);

    let ctx = Context::new(event_hub, network, store, identity, env, token);
    (ctx, tmp)
}

#[tokio::test]
async fn test_five_nodes_concurrent_cluster_mesh_convergence() {
    let cluster_id = Uuid::new(0x5555_6666, 0x7777_8888);
    let base_port = 19360;
    let node_count = 5;

    let mut tokens = Vec::new();
    let mut contexts = Vec::new();
    let mut _dirs = Vec::new();
    let mut handles = Vec::new();
    let mut tasks = Vec::new();

    // 1. Initialize Node 1 (Bootstrap)
    let token1 = CancellationToken::new();
    let (ctx1, tmp1) = make_test_context(token1.clone()).await;
    let config1 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{base_port}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 4,
    };
    let (service1, handle1) = MembershipService::pair_with_config(config1);
    let ctx1_task = ctx1.clone();
    let task1 = tokio::spawn(async move {
        service1.run(ctx1_task).await.unwrap();
    });

    tokens.push(token1);
    contexts.push(ctx1);
    _dirs.push(tmp1);
    handles.push(handle1);
    tasks.push(task1);

    // Wait for bootstrap node to bind
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 2. Start Nodes 2..5 concurrently pointing to Node 1
    for i in 1..node_count {
        let token = CancellationToken::new();
        let (ctx, tmp) = make_test_context(token.clone()).await;
        let port = base_port + i;
        let config = MembershipConfig {
            bind_addr: format!("127.0.0.1:{port}"),
            seeds: vec![format!("127.0.0.1:{base_port}")],
            cluster_id: Some(cluster_id),
            probe_interval: Duration::from_millis(100),
            probe_timeout: Duration::from_millis(50),
            suspect_timeout: Duration::from_millis(500),
            indirect_ping_targets: 3,
            gossip_fanout: 4,
        };
        let (service, handle) = MembershipService::pair_with_config(config);
        let ctx_task = ctx.clone();
        let task = tokio::spawn(async move {
            service.run(ctx_task).await.unwrap();
        });

        tokens.push(token);
        contexts.push(ctx);
        _dirs.push(tmp);
        handles.push(handle);
        tasks.push(task);
    }

    // Wait for all handles to be ready
    for h in &handles {
        h.wait_ready().await;
    }

    // 3. Poll for convergence: every node must know all 5 nodes within 5 seconds
    let mut converged = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut all_see_all = true;
        for h in &handles {
            if h.active_members().await.len() < node_count {
                all_see_all = false;
                break;
            }
        }
        if all_see_all {
            converged = true;
            break;
        }
    }

    assert!(
        converged,
        "All 5 nodes must converge and discover all cluster peers via gossip"
    );

    for h in &handles {
        let members = h.active_members().await;
        assert_eq!(members.len(), 5);
        for ctx in &contexts {
            assert!(h.is_alive(&ctx.identity.id()).await);
        }
    }

    // Teardown
    for token in tokens {
        token.cancel();
    }
    for task in tasks {
        let _ = task.await;
    }
}
