use aaron_admin::{AdminConfig, AdminService};
use aaron_control_plane::{ControlPlaneConfig, ControlPlaneService};
use aaron_core::{CancellationToken, Context, EventHub, Network, NodeId, Service, Store, Uuid};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

async fn setup_test_context() -> (Context, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).unwrap();
    let id = NodeId::new(Uuid::random(), 100, Some(Uuid::random()));
    let env = Arc::new(aaron_core::Env::detect());
    let token = CancellationToken::new();
    let event_hub = EventHub::new();
    let network = Network::new();

    let ctx = Context::new(event_hub, network, store, id, env, token);
    (ctx, tmp)
}

#[tokio::test]
async fn test_admin_control_plane_endpoints() {
    let (ctx, _tmp) = setup_test_context().await;

    // Ephemeral port for Admin HTTP
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin_port = listener.local_addr().unwrap().port();
    drop(listener);

    // Ephemeral port for Raft QUIC
    let raft_listener = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let raft_port = raft_listener.local_addr().unwrap().port();
    drop(raft_listener);

    let raft_addr: SocketAddr = format!("127.0.0.1:{raft_port}").parse().unwrap();
    let (cp_svc, cp_handle) = ControlPlaneService::pair_with_config(ControlPlaneConfig {
        bind_addr: raft_addr,
        node_id: Some(42),
        election_timeout_min_ms: 100,
        election_timeout_max_ms: 200,
        heartbeat_interval_ms: 30,
        snapshot_threshold: 50,
    });

    let admin_addr: SocketAddr = format!("127.0.0.1:{admin_port}").parse().unwrap();
    let admin_svc = AdminService::with_config(AdminConfig {
        bind_addr: admin_addr,
        enabled: true,
        static_dir: None,
    })
    .with_control_plane_handle(cp_handle.clone());

    let cp_ctx = ctx.clone();
    let admin_ctx = ctx.clone();

    let _t1 = tokio::spawn(async move { let _ = cp_svc.run(cp_ctx).await; });
    let _t2 = tokio::spawn(async move { let _ = admin_svc.run(admin_ctx).await; });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let base_url = format!("http://127.0.0.1:{admin_port}");
    let client = reqwest::Client::new();

    // 1. Initial status before bootstrap
    let res = client.get(format!("{base_url}/api/control-plane/status")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let json: serde_json::Value = res.json().await.unwrap();
    assert_eq!(json["available"], true);
    assert_eq!(json["node_id"], 42);
    assert_eq!(json["is_leader"], false);
    assert_eq!(json["voters"].as_array().unwrap().len(), 0);

    // 2. Initialize cluster via POST /api/control-plane/init (auto-bootstrap local node)
    let init_res = client
        .post(format!("{base_url}/api/control-plane/init"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(init_res.status(), 200, "Init should succeed: {:?}", init_res.text().await);

    // 3. Second initialize call must return 409 Conflict (not 500!)
    let dup_res = client
        .post(format!("{base_url}/api/control-plane/init"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(dup_res.status(), 409, "Duplicate init must return 409 Conflict");

    // 4. Wait for node to be elected leader
    let mut elected = false;
    for _ in 0..50 {
        let res = client.get(format!("{base_url}/api/control-plane/status")).send().await.unwrap();
        let json: serde_json::Value = res.json().await.unwrap();
        if json["is_leader"] == true {
            elected = true;
            assert_eq!(json["current_leader"], 42);
            assert_eq!(json["voters"], serde_json::json!([42]));
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(elected, "Node should become Raft leader");

    // 5. Propose linearizable write via POST /api/control-plane/write
    let write_res = client
        .post(format!("{base_url}/api/control-plane/write"))
        .json(&serde_json::json!({
            "key": "cluster/service_mode",
            "value": "distributed_raft"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(write_res.status(), 200, "Write must succeed");

    // 6. Verify replicated state in GET /api/control-plane/status
    let status_res = client.get(format!("{base_url}/api/control-plane/status")).send().await.unwrap();
    let status_json: serde_json::Value = status_res.json().await.unwrap();
    assert_eq!(
        status_json["state_data"]["cluster/service_mode"],
        "distributed_raft"
    );

    // 7. Delete key via POST /api/control-plane/delete
    let del_res = client
        .post(format!("{base_url}/api/control-plane/delete"))
        .json(&serde_json::json!({
            "key": "cluster/service_mode"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(del_res.status(), 200, "Delete must succeed");

    // 8. Verify key was deleted
    let after_res = client.get(format!("{base_url}/api/control-plane/status")).send().await.unwrap();
    let after_json: serde_json::Value = after_res.json().await.unwrap();
    assert!(after_json["state_data"].get("cluster/service_mode").is_none());
}
