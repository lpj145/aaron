use aaron_admin::{AdminConfig, AdminService};
use aaron_control_plane::{ControlPlaneConfig, ControlPlaneNode, ControlPlaneService};
use aaron_core::{Context, Env, EventHub, Network, NodeId, Service, Store, Uuid};
use aaron_shard::{ShardConfig, ShardService};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

#[tokio::main]
#[test]
async fn test_admin_shards_api_stage1() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir().unwrap();
    let store = Store::open(&tmp).unwrap();
    let network = Network::new();
    let event_hub = EventHub::new();
    let env = Arc::new(Env::detect());
    let uuid_a = Uuid::random();
    let identity = NodeId::new(uuid_a, 1, None);
    let token = CancellationToken::new();

    let ctx = Context::new(event_hub.clone(), network, store, identity, env, token.clone());

    let http_port = 18996;
    let cp_port = 18997;

    let cp_config = ControlPlaneConfig {
        bind_addr: format!("127.0.0.1:{cp_port}").parse().unwrap(),
        node_id: Some(1),
        election_timeout_min_ms: 100,
        election_timeout_max_ms: 200,
        heartbeat_interval_ms: 30,
        snapshot_threshold: 50,
    };

    let (cp_svc, cp_handle) = ControlPlaneService::pair_with_config(cp_config);
    let (shard_svc, shard_handle) = ShardService::coordinator(cp_handle.clone());
    let shard_svc = shard_svc.with_config(ShardConfig {
        total_shards: 16,
        replication_factor: 3,
        is_coordinator: true,
    });

    let admin_svc = AdminService::with_config(AdminConfig {
        enabled: true,
        bind_addr: format!("127.0.0.1:{http_port}").parse().unwrap(),
        static_dir: None,
    })
    .with_control_plane_handle(cp_handle.clone())
    .with_shard_handle(shard_handle.clone());

    let cp_ctx = ctx.clone();
    tokio::spawn(async move {
        let _ = cp_svc.run(cp_ctx).await;
    });

    let shard_ctx = ctx.clone();
    tokio::spawn(async move {
        let _ = shard_svc.run(shard_ctx).await;
    });

    let admin_ctx = ctx.clone();
    tokio::spawn(async move {
        let _ = admin_svc.run(admin_ctx).await;
    });

    tokio::time::sleep(Duration::from_millis(300)).await;

    // 1. Initialize Control Plane Raft
    let mut voters = BTreeMap::new();
    voters.insert(1, ControlPlaneNode::new(format!("127.0.0.1:{cp_port}"), uuid_a));
    let _ = cp_handle.initialize(voters).await;

    for _ in 0..50 {
        if cp_handle.is_leader() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{http_port}/api/shards");

    // 2. GET /api/shards -> initially 0 assigned
    let overview_res = client.get(&base_url).send().await?.json::<serde_json::Value>().await?;
    assert_eq!(overview_res["total_shards"], 16);
    assert_eq!(overview_res["assigned_count"], 0);

    // 3. POST /api/shards/bootstrap (Round-Robin with 3 nodes)
    let uuid_b = Uuid::random();
    let uuid_c = Uuid::random();

    let bootstrap_res = client
        .post(format!("{base_url}/bootstrap"))
        .json(&serde_json::json!({
            "nodes": [uuid_a, uuid_b, uuid_c]
        }))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    assert_eq!(bootstrap_res["success"], true);
    assert_eq!(bootstrap_res["assigned_count"], 16);

    // 4. POST /api/shards/assign (Manual assignment with >= 3 nodes)
    let uuid_d = Uuid::random();
    let assign_res = client
        .post(format!("{base_url}/assign"))
        .json(&serde_json::json!({
            "shard_id": 4,
            "primary": uuid_b,
            "replicas": [uuid_c, uuid_d]
        }))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    assert_eq!(assign_res["success"], true);
    assert_eq!(assign_res["placement"]["shard_id"], 4);

    // 5. GET /api/shards -> confirms update
    let updated_res = client.get(&base_url).send().await?.json::<serde_json::Value>().await?;
    assert_eq!(updated_res["assigned_count"], 16);

    token.cancel();
    Ok(())
}
