use aaron_membership::{
    EgressTransport, Member, MembershipConfig, MembershipEvent, MembershipService,
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
async fn test_joiner_without_cluster_id_fails_fast() {
    let token = CancellationToken::new();
    let (ctx, _tmp) = make_test_context(token).await;

    let config = MembershipConfig {
        bind_addr: "127.0.0.1:19140".to_string(),
        seeds: vec!["127.0.0.1:19141".to_string()],
        cluster_id: None, // Missing cluster_id for joiner!
        ..Default::default()
    };

    let service = MembershipService::with_config(config);
    let result = service.run(ctx).await;
    assert!(result.is_err());
    let err_str = format!("{}", result.unwrap_err());
    assert!(err_str.contains("MEMBERSHIP_CLUSTER_ID"));
}

#[tokio::test]
async fn test_two_nodes_quic_join_and_discovery() {
    let cluster_id = Uuid::new(0xAAAA_1111, 0xBBBB_2222);

    let token1 = CancellationToken::new();
    let (ctx1, _tmp1) = make_test_context(token1.clone()).await;

    let token2 = CancellationToken::new();
    let (ctx2, _tmp2) = make_test_context(token2.clone()).await;

    let node1_port = 19146;
    let node2_port = 19147;

    // Node 1: Seed node
    let config1 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{node1_port}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(100),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    // Node 2: Joining node configured with Node 1 as seed and required cluster_id token
    let config2 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{node2_port}"),
        seeds: vec![format!("127.0.0.1:{node1_port}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(100),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let service1 = MembershipService::with_config(config1);
    let service2 = MembershipService::with_config(config2);

    let mut sub1 = ctx1.event_hub.subscribe::<MembershipEvent>().await;
    let mut sub2 = ctx2.event_hub.subscribe::<MembershipEvent>().await;

    let ctx1_task = ctx1.clone();
    let node1_handle = tokio::spawn(async move {
        service1.run(ctx1_task).await.unwrap();
    });

    // Wait for Node 1 to bind and listen
    tokio::time::sleep(Duration::from_millis(150)).await;

    let ctx2_task = ctx2.clone();
    let node2_handle = tokio::spawn(async move {
        service2.run(ctx2_task).await.unwrap();
    });

    // Node 1 should receive MembershipEvent::Joined for Node 2
    let event1 = tokio::time::timeout(Duration::from_secs(3), sub1.recv())
        .await
        .expect("timed out waiting for node1 to discover node2")
        .expect("channel closed");
    assert!(
        matches!(event1, MembershipEvent::Joined(ref m) if m.node_id.id() == ctx2.identity.id())
    );

    // Node 2 should have discovered Node 1
    let event2 = tokio::time::timeout(Duration::from_secs(3), sub2.recv())
        .await
        .expect("timed out waiting for node2 to discover node1")
        .expect("channel closed");
    assert!(
        matches!(event2, MembershipEvent::Joined(ref m) if m.node_id.id() == ctx1.identity.id())
    );

    // Clean cancellation
    token1.cancel();
    token2.cancel();

    node1_handle.await.unwrap();
    node2_handle.await.unwrap();
}

#[tokio::test]
async fn test_quic_failure_detection_and_suspect_to_dead() {
    let cluster_id = Uuid::new(0xCCCC_3333, 0xDDDD_4444);

    let token1 = CancellationToken::new();
    let (ctx1, _tmp1) = make_test_context(token1.clone()).await;

    let token2 = CancellationToken::new();
    let (ctx2, _tmp2) = make_test_context(token2.clone()).await;

    let node1_port = 19166;
    let node2_port = 19167;

    let config1 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{node1_port}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(250),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let config2 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{node2_port}"),
        seeds: vec![format!("127.0.0.1:{node1_port}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(250),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let service1 = MembershipService::with_config(config1);
    let service2 = MembershipService::with_config(config2);

    let mut sub = ctx1.event_hub.subscribe::<MembershipEvent>().await;

    let ctx1_task = ctx1.clone();
    let node1_handle = tokio::spawn(async move {
        service1.run(ctx1_task).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let ctx2_task = ctx2.clone();
    let node2_handle = tokio::spawn(async move {
        service2.run(ctx2_task).await.unwrap();
    });

    // 1. Wait for Node 2 to join Node 1
    let joined = tokio::time::timeout(Duration::from_secs(3), sub.recv())
        .await
        .expect("timed out waiting for node2 join")
        .expect("channel closed");
    assert!(
        matches!(joined, MembershipEvent::Joined(ref m) if m.node_id.id() == ctx2.identity.id())
    );

    // 2. Kill Node 2 abruptly
    token2.cancel();
    let _ = node2_handle.await;

    // 3. Node 1 should detect failure and transition Node 2 to Suspect
    let suspect = tokio::time::timeout(Duration::from_secs(3), sub.recv())
        .await
        .expect("timed out waiting for node2 suspect transition")
        .expect("channel closed");
    assert!(
        matches!(suspect, MembershipEvent::Suspect(ref m) if m.node_id.id() == ctx2.identity.id())
    );

    // 4. Node 1 suspect timer should expire and transition Node 2 to Dead
    let dead = tokio::time::timeout(Duration::from_secs(3), sub.recv())
        .await
        .expect("timed out waiting for node2 dead transition")
        .expect("channel closed");
    assert!(matches!(dead, MembershipEvent::Dead(ref m) if m.node_id.id() == ctx2.identity.id()));

    token1.cancel();
    node1_handle.await.unwrap();
}

#[tokio::test]
async fn test_cluster_id_isolation_rejects_foreign_clusters() {
    let token1 = CancellationToken::new();
    let (mut ctx1, _tmp1) = make_test_context(token1.clone()).await;

    let cluster_a = Uuid::new(0xAAAA, 0x1111);
    ctx1.identity.cluster_id = Some(cluster_a);

    let node1_port = 19186;
    let config1 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{node1_port}"),
        seeds: vec![],
        cluster_id: Some(cluster_a),
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(250),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let service1 = MembershipService::with_config(config1);
    let mut sub = ctx1.event_hub.subscribe::<MembershipEvent>().await;

    let ctx1_task = ctx1.clone();
    let node1_handle = tokio::spawn(async move {
        service1.run(ctx1_task).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    // A foreign node explicitly configured in Cluster B attempts to join Cluster A
    let foreign_network = Network::new();
    let cluster_b = Uuid::new(0xBBBB, 0x2222);
    let foreign_id = NodeId::new(Uuid::random(), 1, Some(cluster_b));
    let foreign_addr = "127.0.0.1:19187".parse().unwrap();
    let foreign_member = Member::new(foreign_id, foreign_addr);

    // Send JoinRequest from foreign cluster to Node 1
    let target_addr = format!("127.0.0.1:{node1_port}").parse().unwrap();
    let join_res = EgressTransport::join(
        &foreign_network.quic,
        target_addr,
        foreign_member,
        Duration::from_millis(200),
    )
    .await;

    // Join must fail (Node 1 finishes stream without admitting due to cluster_id mismatch)
    assert!(join_res.is_err());

    // Node 1 must NOT admit the foreign node to its cluster table
    let event = tokio::time::timeout(Duration::from_millis(200), sub.recv()).await;
    assert!(
        event.is_err(),
        "Foreign cluster member should not be admitted to Cluster A"
    );

    token1.cancel();
    node1_handle.await.unwrap();
}

#[tokio::test]
async fn test_three_nodes_cluster_gossip_convergence_a_b_c() {
    let cluster_id = Uuid::new(0xEEEE_5555, 0xFFFF_6666);

    let token_a = CancellationToken::new();
    let (ctx_a, _tmp_a) = make_test_context(token_a.clone()).await;

    let token_b = CancellationToken::new();
    let (ctx_b, _tmp_b) = make_test_context(token_b.clone()).await;

    let token_c = CancellationToken::new();
    let (ctx_c, _tmp_c) = make_test_context(token_c.clone()).await;

    let port_a = 19201;
    let port_b = 19202;
    let port_c = 19203;

    // Node A: Bootstrap / Seed Node
    let config_a = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_a}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(80),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 5,
    };

    // Node B: Joins via Node A with matching cluster_id
    let config_b = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_b}"),
        seeds: vec![format!("127.0.0.1:{port_a}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(80),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 5,
    };

    // Node C: Joins via Node A with matching cluster_id
    let config_c = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_c}"),
        seeds: vec![format!("127.0.0.1:{port_a}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(80),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 5,
    };

    let service_a = MembershipService::with_config(config_a);
    let service_b = MembershipService::with_config(config_b);
    let service_c = MembershipService::with_config(config_c);

    let mut sub_a = ctx_a.event_hub.subscribe::<MembershipEvent>().await;
    let mut sub_b = ctx_b.event_hub.subscribe::<MembershipEvent>().await;
    let mut sub_c = ctx_c.event_hub.subscribe::<MembershipEvent>().await;

    // 1. Start Node A (Bootstrap)
    let ctx_a_task = ctx_a.clone();
    let handle_a = tokio::spawn(async move {
        service_a.run(ctx_a_task).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    // 2. Start Node B (Joins Node A)
    let ctx_b_task = ctx_b.clone();
    let handle_b = tokio::spawn(async move {
        service_b.run(ctx_b_task).await.unwrap();
    });

    // Verify Node A discovers Node B
    let event_a_sees_b = tokio::time::timeout(Duration::from_secs(3), sub_a.recv())
        .await
        .expect("timeout waiting for A to see B")
        .expect("channel closed");
    assert!(
        matches!(event_a_sees_b, MembershipEvent::Joined(ref m) if m.node_id.id() == ctx_b.identity.id())
    );

    // Verify Node B discovers Node A
    let event_b_sees_a = tokio::time::timeout(Duration::from_secs(3), sub_b.recv())
        .await
        .expect("timeout waiting for B to see A")
        .expect("channel closed");
    assert!(
        matches!(event_b_sees_a, MembershipEvent::Joined(ref m) if m.node_id.id() == ctx_a.identity.id())
    );

    // 3. Start Node C (Joins Node A)
    let ctx_c_task = ctx_c.clone();
    let handle_c = tokio::spawn(async move {
        service_c.run(ctx_c_task).await.unwrap();
    });

    // Verify Node A discovers Node C
    let event_a_sees_c = tokio::time::timeout(Duration::from_secs(3), sub_a.recv())
        .await
        .expect("timeout waiting for A to see C")
        .expect("channel closed");
    assert!(
        matches!(event_a_sees_c, MembershipEvent::Joined(ref m) if m.node_id.id() == ctx_c.identity.id())
    );

    // Verify Node C discovers Node A and Node B (from JoinResponse or gossip)
    let mut c_discovered_peers = Vec::new();
    while c_discovered_peers.len() < 2 {
        let event = tokio::time::timeout(Duration::from_secs(3), sub_c.recv())
            .await
            .expect("timeout waiting for C to discover cluster peers")
            .expect("channel closed");
        if let MembershipEvent::Joined(m) = event {
            c_discovered_peers.push(m.node_id.id());
        }
    }
    assert!(c_discovered_peers.contains(&ctx_a.identity.id()));
    assert!(c_discovered_peers.contains(&ctx_b.identity.id()));

    // 4. CRUCIAL CHECK: Node B receives gossip about Node C and discovers Node C!
    let event_b_sees_c = tokio::time::timeout(Duration::from_secs(3), sub_b.recv())
        .await
        .expect("timeout waiting for B to receive gossip about C")
        .expect("channel closed");
    assert!(
        matches!(event_b_sees_c, MembershipEvent::Joined(ref m) if m.node_id.id() == ctx_c.identity.id())
    );

    // Teardown
    token_a.cancel();
    token_b.cancel();
    token_c.cancel();

    let _ = tokio::join!(handle_a, handle_b, handle_c);
}

#[tokio::test]
async fn test_membership_handle_queries_and_dynamic_join() {
    use aaron_membership::JoinClusterCommand;

    let cluster_id = Uuid::new(0x7777_8888, 0x9999_0000);

    let token1 = CancellationToken::new();
    let (ctx1, _tmp1) = make_test_context(token1.clone()).await;

    let token2 = CancellationToken::new();
    let (ctx2, _tmp2) = make_test_context(token2.clone()).await;

    let port1 = 19251;
    let port2 = 19252;

    let config1 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port1}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(80),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    // Node 2 starts with empty seeds and uses MembershipHandle/JoinClusterCommand to join dynamically at runtime
    let config2 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port2}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(80),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let (service1, handle1) = MembershipService::pair_with_config(config1);
    let (service2, handle2) = MembershipService::pair_with_config(config2);

    let ctx1_task = ctx1.clone();
    let node1_handle = tokio::spawn(async move {
        service1.run(ctx1_task).await.unwrap();
    });

    let ctx2_task = ctx2.clone();
    let node2_handle = tokio::spawn(async move {
        service2.run(ctx2_task).await.unwrap();
    });

    // Wait for services to initialize
    handle1.wait_ready().await;
    handle2.wait_ready().await;

    // Verify handle queries on Node 1
    assert_eq!(handle1.cluster_id().await, Some(cluster_id));
    let local1 = handle1
        .local_member()
        .await
        .expect("local member should exist");
    assert_eq!(local1.node_id.id(), ctx1.identity.id());

    // Trigger dynamic join from Node 2 to Node 1 via EventHub JoinClusterCommand
    let node1_addr = format!("127.0.0.1:{port1}").parse().unwrap();
    ctx2.event_hub
        .publish(JoinClusterCommand::new(node1_addr, Some(cluster_id)))
        .await;

    // Wait briefly for discovery and convergence
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Verify both nodes see each other via MembershipHandle queries
    assert!(handle1.is_alive(&ctx2.identity.id()).await);
    assert!(handle2.is_alive(&ctx1.identity.id()).await);

    let active_on_1 = handle1.active_members().await;
    assert_eq!(active_on_1.len(), 2);

    let active_on_2 = handle2.active_members().await;
    assert_eq!(active_on_2.len(), 2);

    token1.cancel();
    token2.cancel();

    let _ = tokio::join!(node1_handle, node2_handle);
}
