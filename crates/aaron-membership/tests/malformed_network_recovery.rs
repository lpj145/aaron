use aaron_membership::{EgressTransport, Member, MembershipConfig, MembershipService, Message};
use aaron_core::{Context, Env, EventHub, Network, NodeId, Service, Store, Uuid, write_frame};
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
async fn test_malformed_quic_streams_do_not_crash_membership_service() {
    let cluster_id = Uuid::new(0x9999_AAAA, 0xBBBB_CCCC);
    let token = CancellationToken::new();
    let (ctx, _tmp) = make_test_context(token.clone()).await;

    let port = 19380;
    let config = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(80),
        suspect_timeout: Duration::from_millis(500),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let (service, handle) = MembershipService::pair_with_config(config);
    let ctx_task = ctx.clone();
    let service_task = tokio::spawn(async move {
        service.run(ctx_task).await.unwrap();
    });

    handle.wait_ready().await;

    let client_network = Network::new();
    let target_addr = format!("127.0.0.1:{port}").parse().unwrap();

    // 1. Send totally invalid random garbage data over a QUIC stream
    let conn = client_network
        .quic
        .connect(target_addr, "localhost")
        .await
        .unwrap();
    let (mut send, _recv) = conn.open_bi().await.unwrap();
    write_frame(&mut send, b"GARBAGE_PAYLOAD_NOT_FLATBUFFERS")
        .await
        .unwrap();
    let _ = send.finish();

    // Give service a moment to process and discard error
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2. Open an empty stream that immediately closes EOF
    let (mut send_empty, _recv_empty) = conn.open_bi().await.unwrap();
    let _ = send_empty.finish();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // 3. Verify service is still completely healthy and responds to valid Ping probes!
    let local_id = NodeId::new(Uuid::random(), 1, Some(cluster_id));
    let sender_member = Member::new(local_id, "127.0.0.1:19389".parse().unwrap());
    let valid_ping = Message::Ping {
        seq: 99,
        sender: sender_member,
        gossip: vec![],
    };

    let ping_res = EgressTransport::ping(
        &client_network.quic,
        target_addr,
        valid_ping,
        Duration::from_millis(500),
    )
    .await;

    assert!(
        ping_res.is_ok(),
        "Service must remain alive and responsive after receiving malformed data"
    );
    assert!(matches!(ping_res.unwrap(), Message::Ack { seq: 99, .. }));

    token.cancel();
    service_task.await.unwrap();
}
