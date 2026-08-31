use admin_service::{AdminConfig, AdminService};
use membership_service::MembershipService;
use node::{CancellationToken, Context, EventHub, Network, NodeId, Service, Store, Uuid};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing_service::{ChangeLogLevel, TracingService};

async fn setup_test_context() -> (Context, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).unwrap();
    let id = NodeId::new(Uuid::random(), 100, Some(Uuid::random()));
    let env = Arc::new(node::Env::detect());
    let token = CancellationToken::new();
    let event_hub = EventHub::new();
    let network = Network::new();

    let ctx = Context::new(event_hub, network, store, id, env, token);
    (ctx, tmp)
}

#[tokio::test]
async fn test_admin_api_and_spa_serving() {
    let (ctx, _tmp) = setup_test_context().await;

    // Allocate random ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let bind_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = AdminConfig {
        bind_addr,
        enabled: true,
        static_dir: None,
    };

    let (mem_svc, mem_handle) = MembershipService::pair();
    let tracing_svc = TracingService::new();

    let admin_svc = AdminService::with_config(config)
        .with_membership_handle(mem_handle)
        .with_service_schema(&mem_svc)
        .with_service_schema(&tracing_svc);

    let admin_ctx = ctx.clone();
    let service_task = tokio::spawn(async move {
        let _ = admin_svc.run(admin_ctx).await;
    });

    // Wait for server to bind
    tokio::time::sleep(Duration::from_millis(150)).await;

    let base_url = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();

    // 1. Test Static SPA index.html serving
    let res = client.get(&base_url).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body = res.text().await.unwrap();
    assert!(body.contains("Aaron Admin Console"), "HTML body should contain app title");

    // 2. Test SPA route fallback
    let res = client.get(format!("{base_url}/cluster")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body = res.text().await.unwrap();
    assert!(body.contains("Aaron Admin Console"), "Fallback should return index.html");

    // 3. Test GET /api/health
    let res = client.get(format!("{base_url}/api/health")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let json: serde_json::Value = res.json().await.unwrap();
    assert_eq!(json["status"], "ok");

    // 4. Test GET /api/node
    let res = client.get(format!("{base_url}/api/node")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let json: serde_json::Value = res.json().await.unwrap();
    assert_eq!(json["id"], ctx.identity.id().to_string());
    assert_eq!(json["incarnation"], 100);

    // 5. Test GET /api/services
    let res = client.get(format!("{base_url}/api/services")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let json: serde_json::Value = res.json().await.unwrap();
    let services = json["services"].as_array().unwrap();
    assert!(services.iter().any(|s| s["name"] == "membership-service"));
    assert!(services.iter().any(|s| s["name"] == "tracing-service"));
    assert!(services.iter().any(|s| s["name"] == "admin-service"));

    // 6. Test Store API (Keyspaces, Set, Get, Scan, Delete)
    // 6a. Create keyspace
    let res = client
        .post(format!("{base_url}/api/store/keyspaces"))
        .json(&serde_json::json!({ "name": "test_app" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    // 6b. Set key in test_app
    let res = client
        .post(format!("{base_url}/api/store/test_app/set"))
        .json(&serde_json::json!({
            "key": "user:1001",
            "value": "{\"name\":\"Aaron\",\"role\":\"Admin\"}"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    // 6c. Get key from test_app
    let res = client
        .get(format!("{base_url}/api/store/test_app/get?key=user:1001"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let json: serde_json::Value = res.json().await.unwrap();
    assert_eq!(json["exists"], true);
    assert_eq!(json["value"], "{\"name\":\"Aaron\",\"role\":\"Admin\"}");

    // 6d. Scan keyspace
    let res = client
        .get(format!("{base_url}/api/store/test_app/scan?prefix=user:"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let json: serde_json::Value = res.json().await.unwrap();
    let entries = json["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["key"], "user:1001");

    // 6e. Delete key
    let res = client
        .delete(format!("{base_url}/api/store/test_app/delete?key=user:1001"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    // 7. Test Dynamic Tracing Reload API
    let mut tracing_events = ctx.event_hub.subscribe::<ChangeLogLevel>().await;

    let res = client
        .post(format!("{base_url}/api/tracing/level"))
        .json(&serde_json::json!({ "filter": "node=trace,fjall=warn" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    // Verify ChangeLogLevel event arrived on EventHub
    let event = tokio::time::timeout(Duration::from_millis(500), tracing_events.recv())
        .await
        .expect("Timeout waiting for ChangeLogLevel")
        .expect("Recv error");
    assert_eq!(event.filter, "node=trace,fjall=warn");

    // 8. Test Dynamic SWIM Config API
    let mut swim_events = ctx.event_hub.subscribe::<membership_service::UpdateSwimConfig>().await;

    let res = client.get(format!("{base_url}/api/config/swim")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let json: serde_json::Value = res.json().await.unwrap();
    assert_eq!(json["probe_interval_ms"], 1000);

    let res = client
        .post(format!("{base_url}/api/config/swim"))
        .json(&serde_json::json!({
            "probe_interval_ms": 500,
            "probe_timeout_ms": 100,
            "suspect_timeout_ms": 2000,
            "indirect_ping_targets": 4,
            "gossip_fanout": 4,
            "propagate_cluster": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    // Verify UpdateSwimConfig event arrived on EventHub
    let swim_update = tokio::time::timeout(Duration::from_millis(500), swim_events.recv())
        .await
        .expect("Timeout waiting for UpdateSwimConfig")
        .expect("Recv error");
    assert_eq!(swim_update.probe_interval, Some(Duration::from_millis(500)));
    assert_eq!(swim_update.indirect_ping_targets, Some(4));

    // 9. Test GET /api/env
    let res = client.get(format!("{base_url}/api/env")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let json: serde_json::Value = res.json().await.unwrap();
    assert!(json["envs"].as_array().is_some());

    // 10. Test Graceful Shutdown
    ctx.shutdown();
    let _ = tokio::time::timeout(Duration::from_millis(1000), service_task).await;
}
