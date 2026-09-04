use aaron_membership::{EgressTransport, Member, MembershipConfig, MembershipService, Message};
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
async fn test_indirect_ping_req_forwarding_and_ack_resolution() {
    let cluster_id = Uuid::new(0x1111_2222, 0x3333_4444);

    let token_a = CancellationToken::new();
    let (ctx_a, _tmp_a) = make_test_context(token_a.clone()).await;

    let token_b = CancellationToken::new();
    let (ctx_b, _tmp_b) = make_test_context(token_b.clone()).await;

    let token_c = CancellationToken::new();
    let (ctx_c, _tmp_c) = make_test_context(token_c.clone()).await;

    let port_a = 19301;
    let port_b = 19302;
    let port_c = 19303;

    // Node A (Bootstrap)
    let config_a = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_a}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(100),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    // Node B (Mediator)
    let config_b = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_b}"),
        seeds: vec![format!("127.0.0.1:{port_a}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(100),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    // Node C (Target)
    let config_c = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_c}"),
        seeds: vec![format!("127.0.0.1:{port_a}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(100),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let service_a = MembershipService::with_config(config_a);
    let (service_b, handle_b) = MembershipService::pair_with_config(config_b);
    let (service_c, handle_c) = MembershipService::pair_with_config(config_c);

    let ctx_a_task = ctx_a.clone();
    let handle_a = tokio::spawn(async move {
        service_a.run(ctx_a_task).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let ctx_b_task = ctx_b.clone();
    let handle_b_task = tokio::spawn(async move {
        service_b.run(ctx_b_task).await.unwrap();
    });

    let ctx_c_task = ctx_c.clone();
    let handle_c_task = tokio::spawn(async move {
        service_c.run(ctx_c_task).await.unwrap();
    });

    handle_b.wait_ready().await;
    handle_c.wait_ready().await;

    // Wait until Node B has discovered Node C via cluster gossip
    let mut discovered = false;
    for _ in 0..50 {
        if handle_b.get(&ctx_c.identity.id()).await.is_some() {
            discovered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        discovered,
        "Node B must discover Node C via cluster gossip before probing"
    );

    // Node B acts as mediator: an external client asks Node B via PingReq to probe Node C
    let network = Network::new();
    let local_id = NodeId::new(Uuid::random(), 1, Some(cluster_id));
    let sender_member = Member::new(local_id, "127.0.0.1:19309".parse().unwrap());
    let target_c_member = handle_c.local_member().await.unwrap();

    let ping_req = Message::PingReq {
        seq: 42,
        target: target_c_member,
        sender: sender_member,
        gossip: vec![],
    };

    let mediator_addr = format!("127.0.0.1:{port_b}").parse().unwrap();
    let ack_result = EgressTransport::ping_req(
        &network.quic,
        mediator_addr,
        ping_req,
        Duration::from_millis(1000),
    )
    .await;

    assert!(
        ack_result.is_ok(),
        "Mediator Node B should forward Ping to Node C and return Ack"
    );
    let resp = ack_result.unwrap();
    assert!(
        matches!(resp, Message::Ack { seq: 42, ref sender, .. } if sender.node_id.id() == ctx_c.identity.id())
    );

    token_a.cancel();
    token_b.cancel();
    token_c.cancel();

    let _ = tokio::join!(handle_a, handle_b_task, handle_c_task);
}
