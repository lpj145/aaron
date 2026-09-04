use aaron_membership::{
    EgressTransport, Member, MemberStatus, MembershipConfig, MembershipEvent, MembershipService,
    Message,
};
use aaron_core::{Context, Env, EventHub, Network, NodeId, Service, Store, Uuid};
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
async fn test_graceful_leave_gossip_dissemination() {
    let cluster_id = Uuid::new(0x8888_9999, 0xAAAA_BBBB);

    let token_a = CancellationToken::new();
    let (ctx_a, _tmp_a) = make_test_context(token_a.clone()).await;

    let token_b = CancellationToken::new();
    let (ctx_b, _tmp_b) = make_test_context(token_b.clone()).await;

    let port_a = 19341;
    let port_b = 19342;

    let config_a = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_a}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(80),
        suspect_timeout: Duration::from_millis(600),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let config_b = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_b}"),
        seeds: vec![format!("127.0.0.1:{port_a}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(80),
        suspect_timeout: Duration::from_millis(600),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let (service_a, handle_a) = MembershipService::pair_with_config(config_a);
    let service_b = MembershipService::with_config(config_b);

    let mut sub_a = ctx_a.event_hub.subscribe::<MembershipEvent>().await;

    let ctx_a_task = ctx_a.clone();
    let handle_a_task = tokio::spawn(async move {
        service_a.run(ctx_a_task).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let ctx_b_task = ctx_b.clone();
    let handle_b_task = tokio::spawn(async move {
        service_b.run(ctx_b_task).await.unwrap();
    });

    handle_a.wait_ready().await;

    // 1. Wait for Node A to discover Node B
    let join_event = tokio::time::timeout(Duration::from_secs(3), sub_a.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(join_event, MembershipEvent::Joined(ref m) if m.node_id.id() == ctx_b.identity.id())
    );
    assert_eq!(handle_a.active_members().await.len(), 2);

    // 2. Node B announces graceful Leave by sending a Ping update with MemberStatus::Left
    let network = Network::new();
    let leaving_b_member = Member::with_status(
        NodeId::new(ctx_b.identity.id(), 1, Some(cluster_id)),
        format!("127.0.0.1:{port_b}").parse().unwrap(),
        MemberStatus::Left,
        1,
    );

    let ping = Message::Ping {
        seq: 50,
        sender: leaving_b_member,
        gossip: vec![],
    };

    let node_a_addr = format!("127.0.0.1:{port_a}").parse().unwrap();
    let _ =
        EgressTransport::ping(&network.quic, node_a_addr, ping, Duration::from_millis(300)).await;

    // Node A must receive and publish MembershipEvent::Left(Node B)
    let left_event = tokio::time::timeout(Duration::from_secs(3), sub_a.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(left_event, MembershipEvent::Left(ref m) if m.node_id.id() == ctx_b.identity.id())
    );

    // Node A's active members table should now exclude Node B
    assert_eq!(handle_a.active_members().await.len(), 1);
    assert!(!handle_a.is_alive(&ctx_b.identity.id()).await);

    token_a.cancel();
    token_b.cancel();

    let _ = tokio::join!(handle_a_task, handle_b_task);
}
