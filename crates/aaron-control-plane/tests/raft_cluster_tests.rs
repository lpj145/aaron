use aaron_control_plane::types::ControlPlaneNode;
use aaron_control_plane::{ControlPlaneConfig, ControlPlaneService};
use aaron_core::{Context, Env, EventHub, Network, NodeId, Service, Store, Uuid};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

async fn make_test_context(token: CancellationToken, uuid: Uuid) -> (Context, tempfile::TempDir) {
    let tmp = tempdir().unwrap();
    let store = Store::open(&tmp).unwrap();
    let network = Network::new();
    let event_hub = EventHub::new();
    let env = Arc::new(Env::detect());
    let identity = NodeId::new(uuid, 1, None);

    let ctx = Context::new(event_hub, network, store, identity, env, token);
    (ctx, tmp)
}

#[tokio::test]
async fn test_single_node_bootstrap_and_write_read() {
    let token = CancellationToken::new();
    let uuid = Uuid::random();
    let (ctx, _tmp) = make_test_context(token.clone(), uuid).await;

    let port = 18950;
    let bind_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

    let (control_plane, handle) = ControlPlaneService::pair_with_config(ControlPlaneConfig {
        bind_addr,
        node_id: Some(1),
        election_timeout_min_ms: 100,
        election_timeout_max_ms: 200,
        heartbeat_interval_ms: 30,
        snapshot_threshold: 50,
    });

    let svc_ctx = ctx.clone();
    let runner = tokio::spawn(async move {
        let _ = control_plane.run(svc_ctx).await;
    });

    // Wait a brief moment for service initialization
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Bootstrap single-node Raft cluster
    let mut voters = BTreeMap::new();
    voters.insert(1, ControlPlaneNode::new(bind_addr.to_string(), uuid));

    let init_res = handle.initialize(voters).await;
    assert!(init_res.is_ok(), "Single node bootstrap should succeed: {:?}", init_res);

    // Wait for leadership election
    let mut elected = false;
    for _ in 0..50 {
        if handle.is_leader() {
            elected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(elected, "Node 1 should be elected leader");

    // Write state
    let write_res = handle.set("system/version", "v2.0.0").await;
    assert!(write_res.is_ok(), "Raft write should succeed: {:?}", write_res);

    let val = handle.get_string("system/version").await;
    assert_eq!(val.as_deref(), Some("v2.0.0"));

    let all = handle.all_data_strings().await;
    assert_eq!(all.get("system/version").map(|s| s.as_str()), Some("v2.0.0"));

    // Delete state
    let del_res = handle.delete("system/version").await;
    assert!(del_res.is_ok(), "Raft delete should succeed: {:?}", del_res);

    let val_after = handle.get_string("system/version").await;
    assert_eq!(val_after, None);

    token.cancel();
    runner.abort();
}

#[tokio::test]
async fn test_multi_node_cluster_replication() {
    let token1 = CancellationToken::new();
    let token2 = CancellationToken::new();
    let token3 = CancellationToken::new();

    let uuid1 = Uuid::random();
    let uuid2 = Uuid::random();
    let uuid3 = Uuid::random();

    let (ctx1, _tmp1) = make_test_context(token1.clone(), uuid1).await;
    let (ctx2, _tmp2) = make_test_context(token2.clone(), uuid2).await;
    let (ctx3, _tmp3) = make_test_context(token3.clone(), uuid3).await;

    let addr1: SocketAddr = "127.0.0.1:18951".parse().unwrap();
    let addr2: SocketAddr = "127.0.0.1:18952".parse().unwrap();
    let addr3: SocketAddr = "127.0.0.1:18953".parse().unwrap();

    let (cp1, handle1) = ControlPlaneService::pair_with_config(ControlPlaneConfig {
        bind_addr: addr1,
        node_id: Some(1),
        election_timeout_min_ms: 150,
        election_timeout_max_ms: 300,
        heartbeat_interval_ms: 40,
        snapshot_threshold: 50,
    });

    let (cp2, handle2) = ControlPlaneService::pair_with_config(ControlPlaneConfig {
        bind_addr: addr2,
        node_id: Some(2),
        election_timeout_min_ms: 150,
        election_timeout_max_ms: 300,
        heartbeat_interval_ms: 40,
        snapshot_threshold: 50,
    });

    let (cp3, handle3) = ControlPlaneService::pair_with_config(ControlPlaneConfig {
        bind_addr: addr3,
        node_id: Some(3),
        election_timeout_min_ms: 150,
        election_timeout_max_ms: 300,
        heartbeat_interval_ms: 40,
        snapshot_threshold: 50,
    });

    let r1 = tokio::spawn(async move { let _ = cp1.run(ctx1).await; });
    let r2 = tokio::spawn(async move { let _ = cp2.run(ctx2).await; });
    let r3 = tokio::spawn(async move { let _ = cp3.run(ctx3).await; });

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Bootstrap node 1 & node 2 as Voters
    let mut voters = BTreeMap::new();
    voters.insert(1, ControlPlaneNode::new(addr1.to_string(), uuid1));
    voters.insert(2, ControlPlaneNode::new(addr2.to_string(), uuid2));

    let init_res = handle1.initialize(voters).await;
    assert!(init_res.is_ok(), "Init voters should succeed: {:?}", init_res);

    // Wait for node 1 to be elected leader and commit initial membership
    let mut elected = false;
    for _ in 0..50 {
        if handle1.is_leader() {
            elected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(elected, "Node 1 should become leader");

    // Add node 3 as Learner
    let learner_res = handle1.add_learner(3, ControlPlaneNode::new(addr3.to_string(), uuid3), true).await;
    assert!(learner_res.is_ok(), "Add learner should succeed: {:?}", learner_res);

    // Write through leader
    let write_res = handle1.set("cluster/mode", "raft-replicated").await;
    assert!(write_res.is_ok(), "Leader write should succeed: {:?}", write_res);

    // Verify replication to followers/learners
    let mut replicated_node2 = false;
    let mut replicated_node3 = false;

    for _ in 0..50 {
        if handle2.get_string("cluster/mode").await.as_deref() == Some("raft-replicated") {
            replicated_node2 = true;
        }
        if handle3.get_string("cluster/mode").await.as_deref() == Some("raft-replicated") {
            replicated_node3 = true;
        }
        if replicated_node2 && replicated_node3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(replicated_node2, "Node 2 (Voter) should replicate state machine");
    assert!(replicated_node3, "Node 3 (Learner) should replicate state machine");

    token1.cancel();
    token2.cancel();
    token3.cancel();
    r1.abort();
    r2.abort();
    r3.abort();
}
