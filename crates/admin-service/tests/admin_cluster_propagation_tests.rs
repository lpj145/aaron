use admin_service::{AdminConfig, AdminService};
use membership_service::{MembershipConfig, MembershipService, UpdateSwimConfig};
use node::{Context, Env, EventHub, Network, NodeId, Service, Store, Uuid};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use tracing_service::ChangeLogLevel;

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
async fn test_admin_api_cluster_propagation_e2e() {
    let cluster_id = Uuid::new(0xABCD_1111, 0x2222_EEEE);
    let mem_port1 = 19490;
    let admin_port1 = 18490;
    let mem_port2 = 19491;
    let admin_port2 = 18491;

    // 1. Start Node 1 with Membership & Admin Service
    let token1 = CancellationToken::new();
    let (ctx1, _tmp1) = make_test_context(token1.clone()).await;

    let mem_cfg1 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{mem_port1}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(1000),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };
    let (mem_svc1, mem_handle1) = MembershipService::pair_with_config(mem_cfg1);
    let admin_cfg1 = AdminConfig {
        bind_addr: format!("127.0.0.1:{admin_port1}").parse::<SocketAddr>().unwrap(),
        enabled: true,
        static_dir: None,
    };
    let admin_svc1 = AdminService::with_config(admin_cfg1)
        .with_membership_handle(mem_handle1.clone());

    let ctx1_task_mem = ctx1.clone();
    let task_mem1 = tokio::spawn(async move {
        mem_svc1.run(ctx1_task_mem).await.unwrap();
    });
    let ctx1_task_admin = ctx1.clone();
    let task_admin1 = tokio::spawn(async move {
        admin_svc1.run(ctx1_task_admin).await.unwrap();
    });

    // 2. Start Node 2 with Membership pointing to Node 1 & Admin Service
    let token2 = CancellationToken::new();
    let (ctx2, _tmp2) = make_test_context(token2.clone()).await;

    let mem_cfg2 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{mem_port2}"),
        seeds: vec![format!("127.0.0.1:{mem_port1}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(1000),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };
    let (mem_svc2, mem_handle2) = MembershipService::pair_with_config(mem_cfg2);
    let admin_cfg2 = AdminConfig {
        bind_addr: format!("127.0.0.1:{admin_port2}").parse::<SocketAddr>().unwrap(),
        enabled: true,
        static_dir: None,
    };
    let admin_svc2 = AdminService::with_config(admin_cfg2)
        .with_membership_handle(mem_handle2.clone());

    let ctx2_task_mem = ctx2.clone();
    let task_mem2 = tokio::spawn(async move {
        mem_svc2.run(ctx2_task_mem).await.unwrap();
    });
    let ctx2_task_admin = ctx2.clone();
    let task_admin2 = tokio::spawn(async move {
        admin_svc2.run(ctx2_task_admin).await.unwrap();
    });

    mem_handle1.wait_ready().await;
    mem_handle2.wait_ready().await;

    // Wait until both nodes are active in the cluster table
    let mut ready = false;
    for _ in 0..30 {
        if mem_handle1.active_members().await.len() >= 2 && mem_handle2.active_members().await.len() >= 2 {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "Cluster must converge with both nodes active");

    let client = reqwest::Client::new();
    let node1_admin_url = format!("http://127.0.0.1:{admin_port1}");

    // 3. Test Tracing Propagation via Node 1's Admin HTTP REST API
    let mut node2_tracing_events = ctx2.event_hub.subscribe::<ChangeLogLevel>().await;

    let res = client
        .post(format!("{node1_admin_url}/api/config/tracing"))
        .json(&serde_json::json!({
            "filter": "node=trace,membership=debug,fjall=warn",
            "propagate_cluster": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), 200);
    let json: serde_json::Value = res.json().await.unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["propagated_nodes"], 1);

    // Verify Node 2 received ChangeLogLevel event over QUIC mesh
    let event = tokio::time::timeout(Duration::from_millis(1500), node2_tracing_events.recv())
        .await
        .expect("Timeout waiting for propagated ChangeLogLevel on Node 2")
        .expect("Recv error");
    assert_eq!(event.filter, "node=trace,membership=debug,fjall=warn");

    // 4. Test SWIM Config Propagation via Node 1's Admin HTTP REST API
    let mut node2_swim_events = ctx2.event_hub.subscribe::<UpdateSwimConfig>().await;

    let res = client
        .post(format!("{node1_admin_url}/api/config/swim"))
        .json(&serde_json::json!({
            "probe_interval_ms": 600,
            "probe_timeout_ms": 150,
            "suspect_timeout_ms": 3000,
            "indirect_ping_targets": 4,
            "gossip_fanout": 5,
            "propagate_cluster": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), 200);
    let json: serde_json::Value = res.json().await.unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["propagated_nodes"], 1);

    // Verify Node 2 received UpdateSwimConfig event over QUIC mesh
    let swim_event = tokio::time::timeout(Duration::from_millis(1500), node2_swim_events.recv())
        .await
        .expect("Timeout waiting for propagated UpdateSwimConfig on Node 2")
        .expect("Recv error");
    assert_eq!(swim_event.probe_interval, Some(Duration::from_millis(600)));
    assert_eq!(swim_event.probe_timeout, Some(Duration::from_millis(150)));
    assert_eq!(swim_event.suspect_timeout, Some(Duration::from_millis(3000)));
    assert_eq!(swim_event.indirect_ping_targets, Some(4));
    assert_eq!(swim_event.gossip_fanout, Some(5));

    // Verify Node 2 dynamically applied new parameters
    tokio::time::sleep(Duration::from_millis(100)).await;
    let node2_cfg = mem_handle2.config().await.expect("Node 2 config");
    assert_eq!(node2_cfg.probe_interval, Duration::from_millis(600));
    assert_eq!(node2_cfg.indirect_ping_targets, 4);
    assert_eq!(node2_cfg.gossip_fanout, 5);

    // 5. Test Environment Variable Propagation via Node 1's Admin HTTP REST API
    let mut node2_env_events = ctx2.event_hub.subscribe::<node::SetEnvVar>().await;

    let res = client
        .post(format!("{node1_admin_url}/api/env"))
        .json(&serde_json::json!({
            "key": "APP_SECRET_TOKEN",
            "value": "super-cluster-token-123",
            "propagate_cluster": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), 200);
    let json: serde_json::Value = res.json().await.unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["propagated_nodes"], 1);

    // Verify Node 2 received SetEnvVar event over QUIC mesh
    let env_event = tokio::time::timeout(Duration::from_millis(1500), node2_env_events.recv())
        .await
        .expect("Timeout waiting for propagated SetEnvVar on Node 2")
        .expect("Recv error");
    assert_eq!(env_event.key, "APP_SECRET_TOKEN");
    assert_eq!(env_event.value, "super-cluster-token-123");

    // Verify local node set it too
    assert_eq!(ctx1.env.get_raw("APP_SECRET_TOKEN"), Some("super-cluster-token-123".to_string()));

    // Teardown
    token1.cancel();
    token2.cancel();
    let _ = task_mem1.await;
    let _ = task_admin1.await;
    let _ = task_mem2.await;
    let _ = task_admin2.await;
}
