use membership_service::{
    EgressTransport, Member, MemberStatus, MembershipConfig, MembershipEvent, MembershipService,
    Message,
};
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
async fn test_false_suspicion_refutation_and_peer_recovery() {
    let cluster_id = Uuid::new(0x4444_5555, 0x6666_7777);

    let token1 = CancellationToken::new();
    let (ctx1, _tmp1) = make_test_context(token1.clone()).await;

    let token2 = CancellationToken::new();
    let (ctx2, _tmp2) = make_test_context(token2.clone()).await;

    let port1 = 19321;
    let port2 = 19322;

    let config1 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port1}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(80),
        suspect_timeout: Duration::from_millis(600),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let config2 = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port2}"),
        seeds: vec![format!("127.0.0.1:{port1}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(80),
        suspect_timeout: Duration::from_millis(600),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let service1 = MembershipService::with_config(config1);
    let (service2, handle2) = MembershipService::pair_with_config(config2);

    let mut sub1 = ctx1.event_hub.subscribe::<MembershipEvent>().await;
    let mut sub2 = ctx2.event_hub.subscribe::<MembershipEvent>().await;

    let ctx1_task = ctx1.clone();
    let handle1 = tokio::spawn(async move {
        service1.run(ctx1_task).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let ctx2_task = ctx2.clone();
    let handle2_task = tokio::spawn(async move {
        service2.run(ctx2_task).await.unwrap();
    });

    handle2.wait_ready().await;

    // 1. Wait for both nodes to discover each other
    let _ = tokio::time::timeout(Duration::from_secs(3), sub1.recv())
        .await
        .unwrap()
        .unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(3), sub2.recv())
        .await
        .unwrap()
        .unwrap();

    let node2_initial_member = handle2.local_member().await.unwrap();
    assert_eq!(node2_initial_member.incarnation, 1);

    // 2. Node 1 sends a Ping with gossip stating Node 2 is Suspect at inc=1
    let network = Network::new();
    let node1_member = Member::new(
        NodeId::new(ctx1.identity.id(), 1, Some(cluster_id)),
        format!("127.0.0.1:{port1}").parse().unwrap(),
    );
    let false_suspicion_of_node2 = Member::with_status(
        node2_initial_member.node_id.clone(),
        node2_initial_member.addr,
        MemberStatus::Suspect,
        1,
    );

    let ping = Message::Ping {
        seq: 100,
        sender: node1_member,
        gossip: vec![false_suspicion_of_node2],
    };

    // Send false suspicion to Node 2
    let node2_addr = format!("127.0.0.1:{port2}").parse().unwrap();
    let ack = EgressTransport::ping(&network.quic, node2_addr, ping, Duration::from_millis(500))
        .await
        .unwrap();

    // Node 2 must have refuted in its response Ack with incremented incarnation=2
    if let Message::Ack { sender, .. } = ack {
        assert_eq!(sender.incarnation, 2);
        assert_eq!(sender.status, MemberStatus::Alive);
    } else {
        panic!("expected Ack");
    }

    // Node 2 should have published MembershipEvent::Refuted on its EventHub
    let refuted_event = tokio::time::timeout(Duration::from_secs(3), sub2.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(refuted_event, MembershipEvent::Refuted(ref m) if m.incarnation == 2 && m.status == MemberStatus::Alive)
    );

    // 3. Node 1 receives the new gossip (incarnation=2) from Node 2's periodic probes and reaffirms Node 2 as Alive
    let alive_event = tokio::time::timeout(Duration::from_secs(3), sub1.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(alive_event, MembershipEvent::Alive(ref m) if m.incarnation == 2));

    token1.cancel();
    token2.cancel();

    let _ = tokio::join!(handle1, handle2_task);
}
