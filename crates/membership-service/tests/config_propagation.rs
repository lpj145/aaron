use membership_service::{MembershipConfig, MembershipService, UpdateSwimConfig};
use node::{Context, Env, EventHub, Network, NodeId, Service, Store, Uuid};
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
async fn test_cluster_config_propagation_over_quic() {
    let cluster_id = Uuid::new(0x1234_5678, 0x9ABC_DEF0);
    let port1 = 19480;
    let port2 = 19481;

    let token1 = CancellationToken::new();
    let (ctx1, _tmp1) = make_test_context(token1.clone()).await;
    let config1 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port1}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(1000),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };
    let (service1, handle1) = MembershipService::pair_with_config(config1);
    let ctx1_task = ctx1.clone();
    let task1 = tokio::spawn(async move {
        service1.run(ctx1_task).await.unwrap();
    });

    // Start Node 2 connected to Node 1
    let token2 = CancellationToken::new();
    let (ctx2, _tmp2) = make_test_context(token2.clone()).await;
    let config2 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port2}"),
        seeds: vec![format!("127.0.0.1:{port1}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(1000),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };
    let (service2, handle2) = MembershipService::pair_with_config(config2);
    let ctx2_task = ctx2.clone();
    let task2 = tokio::spawn(async move {
        service2.run(ctx2_task).await.unwrap();
    });

    handle1.wait_ready().await;
    handle2.wait_ready().await;

    // Wait until both nodes discover each other
    let mut discovered = false;
    for _ in 0..30 {
        if handle1.active_members().await.len() >= 2 && handle2.active_members().await.len() >= 2 {
            discovered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(discovered, "Nodes must discover each other in cluster");

    // 1. Subscribe to events on Node 2
    let mut node2_tracing_events = ctx2.event_hub.subscribe::<ChangeLogLevel>().await;
    let mut node2_swim_events = ctx2.event_hub.subscribe::<UpdateSwimConfig>().await;
    let mut node2_env_events = ctx2.event_hub.subscribe::<node::SetEnvVar>().await;

    // 2. Node 1 broadcasts a Tracing Config update to the cluster over QUIC
    let (prop_t, fail_t) = handle1
        .broadcast_config_update(Some("node=trace,fjall=error".to_string()), None, None)
        .await;

    assert_eq!(prop_t, 1, "Must propagate tracing update to 1 peer");
    assert_eq!(fail_t, 0, "No failures expected");

    // Verify Node 2 received ChangeLogLevel event on its local EventHub
    let tracing_event = tokio::time::timeout(Duration::from_millis(1000), node2_tracing_events.recv())
        .await
        .expect("Timeout waiting for ChangeLogLevel on Node 2")
        .expect("Recv error");
    assert_eq!(tracing_event.filter, "node=trace,fjall=error");

    // 3. Node 1 broadcasts a SWIM Config update to the cluster over QUIC
    let swim_update = UpdateSwimConfig {
        probe_interval: Some(Duration::from_millis(450)),
        probe_timeout: Some(Duration::from_millis(120)),
        suspect_timeout: Some(Duration::from_millis(2500)),
        indirect_ping_targets: Some(5),
        gossip_fanout: Some(4),
    };

    let (prop_s, fail_s) = handle1
        .broadcast_config_update(None, Some(swim_update), None)
        .await;

    assert_eq!(prop_s, 1, "Must propagate SWIM update to 1 peer");
    assert_eq!(fail_s, 0, "No failures expected");

    // Verify Node 2 received UpdateSwimConfig event on its local EventHub
    let swim_event = tokio::time::timeout(Duration::from_millis(1000), node2_swim_events.recv())
        .await
        .expect("Timeout waiting for UpdateSwimConfig on Node 2")
        .expect("Recv error");
    assert_eq!(swim_event.probe_interval, Some(Duration::from_millis(450)));
    assert_eq!(swim_event.probe_timeout, Some(Duration::from_millis(120)));
    assert_eq!(swim_event.suspect_timeout, Some(Duration::from_millis(2500)));
    assert_eq!(swim_event.indirect_ping_targets, Some(5));
    assert_eq!(swim_event.gossip_fanout, Some(4));

    // Verify Node 2's ProbeLoop dynamically reloaded the config
    tokio::time::sleep(Duration::from_millis(100)).await;
    let node2_config = handle2.config().await.expect("Node 2 config");
    assert_eq!(node2_config.probe_interval, Duration::from_millis(450));
    assert_eq!(node2_config.probe_timeout, Duration::from_millis(120));
    assert_eq!(node2_config.indirect_ping_targets, 5);
    assert_eq!(node2_config.gossip_fanout, 4);

    // 4. Node 1 broadcasts an Environment Variable update to the cluster over QUIC
    let (prop_e, fail_e) = handle1
        .broadcast_config_update(
            None,
            None,
            Some(("DATABASE_URL".to_string(), "postgres://master:5432/app".to_string())),
        )
        .await;

    assert_eq!(prop_e, 1, "Must propagate env var to 1 peer");
    assert_eq!(fail_e, 0, "No failures expected");

    let env_event = tokio::time::timeout(Duration::from_millis(1000), node2_env_events.recv())
        .await
        .expect("Timeout waiting for SetEnvVar on Node 2")
        .expect("Recv error");
    assert_eq!(env_event.key, "DATABASE_URL");
    assert_eq!(env_event.value, "postgres://master:5432/app");

    // Teardown
    token1.cancel();
    token2.cancel();
    let _ = task1.await;
    let _ = task2.await;
}
