use aaron_membership::{
    EgressTransport, Member, MemberStatus, MembershipConfig, MembershipService, MembershipTable,
    Message,
};
use aaron_core::{Context, Env, EventHub, Network, NodeId, Service, Store, Uuid, write_frame};
use std::net::SocketAddr;
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
async fn test_network_chaos_and_corrupted_flood() {
    let cluster_id = Uuid::new(0xABCD_0001, 0xDCBA_0002);
    let token = CancellationToken::new();
    let (ctx, _tmp) = make_test_context(token.clone()).await;

    let port = 19410;
    let config = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(400),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let (service, handle) = MembershipService::pair_with_config(config);
    let ctx_task = ctx.clone();
    let service_task = tokio::spawn(async move {
        service.run(ctx_task).await.unwrap();
    });

    handle.wait_ready().await;
    let target_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

    // Spawn 8 concurrent attackers blasting the node with various corrupt/unauthorized frames
    let mut flood_tasks = Vec::new();
    for i in 0..8 {
        let attacker_net = Network::new();
        flood_tasks.push(tokio::spawn(async move {
            for j in 0..25 {
                let conn_res = attacker_net.quic.connect(target_addr, "localhost").await;
                if let Ok(conn) = conn_res
                    && let Ok((mut send, _recv)) = conn.open_bi().await
                {
                    match (i + j) % 5 {
                        // Pattern 0: Random corrupt binary garbage
                        0 => {
                            let garbage = vec![(i * 37 + j * 13) as u8; 64];
                            let _ = write_frame(&mut send, &garbage).await;
                        }
                        // Pattern 1: Empty stream EOF
                        1 => {
                            let _ = send.finish();
                        }
                        // Pattern 2: Foreign cluster JoinRequest
                        2 => {
                            let rogue_member = Member::new(
                                NodeId::new(Uuid::random(), 1, Some(Uuid::random())),
                                "127.0.0.1:19999".parse().unwrap(),
                            );
                            let req = Message::JoinRequest {
                                sender: rogue_member,
                            };
                            let _ = write_frame(&mut send, &req.to_bytes()).await;
                        }
                        // Pattern 3: Unauthorized PingReq SSRF attempt to arbitrary external IP
                        3 => {
                            let fake_sender = Member::new(
                                NodeId::new(Uuid::random(), 1, Some(Uuid::random())),
                                "127.0.0.1:19999".parse().unwrap(),
                            );
                            let fake_target = Member::new(
                                NodeId::new(Uuid::random(), 1, None),
                                "127.0.0.1:9".parse().unwrap(), // Discard port
                            );
                            let req = Message::PingReq {
                                seq: 1,
                                target: fake_target,
                                sender: fake_sender,
                                gossip: vec![],
                            };
                            let _ = write_frame(&mut send, &req.to_bytes()).await;
                        }
                        // Pattern 4: Truncated frame header
                        _ => {
                            let truncated = b"\x00\x00\x00\x10incomplete";
                            let _ = write_frame(&mut send, truncated).await;
                        }
                    }
                    let _ = send.finish();
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }));
    }

    for t in flood_tasks {
        let _ = t.await;
    }

    // Assert: Node is still completely healthy and processes legitimate cluster probes!
    let client_net = Network::new();
    let local_id = NodeId::new(Uuid::random(), 1, Some(cluster_id));
    let legitimate_member = Member::new(local_id, "127.0.0.1:19419".parse().unwrap());
    let valid_ping = Message::Ping {
        seq: 555,
        sender: legitimate_member,
        gossip: vec![],
    };

    let ping_res = EgressTransport::ping(
        &client_net.quic,
        target_addr,
        valid_ping,
        Duration::from_millis(1000),
    )
    .await;

    assert!(
        ping_res.is_ok(),
        "Node must remain responsive to legitimate traffic after flood"
    );
    assert!(matches!(ping_res.unwrap(), Message::Ack { seq: 555, .. }));

    token.cancel();
    let _ = service_task.await;
}

#[tokio::test]
async fn test_state_machine_fuzzing_invariants() {
    let local_cluster = Uuid::new(0x1234_5678, 0x9ABC_DEF0);
    let local_id = NodeId::new(Uuid::new(0, 1), 10, Some(local_cluster));
    let local_addr: SocketAddr = "127.0.0.1:19420".parse().unwrap();
    let table = MembershipTable::new(local_id.clone(), local_addr);

    let mut distinct_uuids = Vec::new();
    for i in 1..=20 {
        distinct_uuids.push(Uuid::new(i, i * 10));
    }

    // Generate and apply 500 pseudo-random member updates with chaotic incarnations and states
    let mut rng_state: u64 = 0xFEDC_BA98_7654_3210;
    for step in 0..500 {
        // xorshift step
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 7;
        rng_state ^= rng_state << 17;

        let uuid_idx = (rng_state as usize) % (distinct_uuids.len() + 1);
        let target_uuid = if uuid_idx == distinct_uuids.len() {
            // Update targeting local node!
            local_id.id()
        } else {
            distinct_uuids[uuid_idx]
        };

        let incarnation = (rng_state >> 8) % 50;
        let status = match (rng_state >> 16) % 4 {
            0 => MemberStatus::Alive,
            1 => MemberStatus::Suspect,
            2 => MemberStatus::Dead,
            _ => MemberStatus::Left,
        };

        let cluster = match (rng_state >> 24) % 3 {
            0 => Some(local_cluster),
            1 => None,
            _ => Some(Uuid::random()),
        };

        let member_id = NodeId::new(target_uuid, incarnation, cluster);
        let member_addr: SocketAddr = format!("127.0.0.1:{}", 20000 + (step % 500))
            .parse()
            .unwrap();
        let member = Member::with_status(member_id, member_addr, status, incarnation);

        table.upsert(member).await;

        // Invariant 1: Local node is NEVER in Dead or Suspect state
        let current_local = table.local_member().await;
        assert_eq!(
            current_local.status,
            MemberStatus::Alive,
            "Local node must always refute and remain Alive"
        );
        assert!(
            current_local.incarnation >= 10,
            "Local node incarnation must never decrease"
        );
    }

    // Invariant 2: No member with mismatched/foreign cluster_id was ever admitted
    let all_active = table.all_active_members().await;
    for m in all_active {
        assert_eq!(
            m.node_id.cluster_id,
            Some(local_cluster),
            "Every member in the table must strictly belong to the local cluster"
        );
    }
}

#[tokio::test]
async fn test_cluster_churn_and_rejoin_simulation() {
    let cluster_id = Uuid::new(0x3333_4444, 0x5555_6666);
    let port_a = 19430;
    let port_b = 19431;
    let port_c = 19432;

    // 1. Start Node A (Bootstrap)
    let token_a = CancellationToken::new();
    let (ctx_a, _tmp_a) = make_test_context(token_a.clone()).await;
    let config_a = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_a}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(300),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };
    let (service_a, handle_a) = MembershipService::pair_with_config(config_a);
    let ctx_a_task = ctx_a.clone();
    let task_a = tokio::spawn(async move {
        service_a.run(ctx_a_task).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    // 2. Start Node B
    let token_b = CancellationToken::new();
    let (ctx_b, _tmp_b) = make_test_context(token_b.clone()).await;
    let config_b = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_b}"),
        seeds: vec![format!("127.0.0.1:{port_a}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(300),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };
    let (service_b, handle_b) = MembershipService::pair_with_config(config_b);
    let ctx_b_task = ctx_b.clone();
    let task_b = tokio::spawn(async move {
        service_b.run(ctx_b_task).await.unwrap();
    });

    // 3. Start Node C (Ephemeral Churn Node)
    let token_c = CancellationToken::new();
    let (ctx_c, _tmp_c) = make_test_context(token_c.clone()).await;
    let config_c = MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_c}"),
        seeds: vec![format!("127.0.0.1:{port_a}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(300),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };
    let (service_c, handle_c) = MembershipService::pair_with_config(config_c);
    let ctx_c_task = ctx_c.clone();
    let task_c = tokio::spawn(async move {
        service_c.run(ctx_c_task).await.unwrap();
    });

    handle_a.wait_ready().await;
    handle_b.wait_ready().await;
    handle_c.wait_ready().await;

    // Wait until all 3 nodes discover each other
    let mut initial_mesh_ready = false;
    for _ in 0..40 {
        if handle_a.active_members().await.len() == 3
            && handle_b.active_members().await.len() == 3
            && handle_c.active_members().await.len() == 3
        {
            initial_mesh_ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(initial_mesh_ready, "Initial 3-node cluster must converge");

    // 4. Simulate abrupt crash of Node C
    token_c.cancel();
    let _ = task_c.await;

    // Nodes A and B should detect Node C as Dead within suspect_timeout + probe_interval (~600ms)
    let mut node_c_dead = false;
    for _ in 0..40 {
        let a_alive_c = handle_a.is_alive(&ctx_c.identity.id()).await;
        let b_alive_c = handle_b.is_alive(&ctx_c.identity.id()).await;
        if !a_alive_c && !b_alive_c {
            node_c_dead = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        node_c_dead,
        "Nodes A and B must declare crashed Node C Dead"
    );

    // 5. Node C restarts with higher incarnation (e.g. new process start, inc=10)
    let token_c2 = CancellationToken::new();
    let (ctx_c2, _tmp_c2) = make_test_context(token_c2.clone()).await;
    let mut new_identity = ctx_c.identity.clone();
    new_identity.incarnation = 10;
    let mut ctx_c2_restarted = ctx_c2.clone();
    ctx_c2_restarted.identity = new_identity;

    let port_c2 = 19435;
    let (service_c2, handle_c2) = MembershipService::pair_with_config(MembershipConfig {
        bind_addr: format!("127.0.0.1:{port_c2}"),
        seeds: vec![format!("127.0.0.1:{port_a}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(50),
        suspect_timeout: Duration::from_millis(300),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    });

    let task_c2 = tokio::spawn(async move {
        service_c2.run(ctx_c2_restarted).await.unwrap();
    });
    handle_c2.wait_ready().await;

    // Node A and Node B should seamlessly accept the resurrected Node C (inc=10) back as Alive!
    let mut node_c_resurrected = false;
    for _ in 0..40 {
        if handle_a.is_alive(&ctx_c.identity.id()).await
            && handle_b.is_alive(&ctx_c.identity.id()).await
        {
            node_c_resurrected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        node_c_resurrected,
        "Node C must be restored to Alive on Nodes A and B with higher incarnation"
    );

    token_a.cancel();
    token_b.cancel();
    token_c2.cancel();

    let _ = tokio::join!(task_a, task_b, task_c2);
}

#[tokio::test]
async fn test_tombstone_retention_and_gc() {
    let local_id = NodeId::new(Uuid::random(), 1, None);
    let local_addr: SocketAddr = "127.0.0.1:19440".parse().unwrap();
    let table = MembershipTable::new(local_id, local_addr);

    let dead_peer_id = NodeId::new(Uuid::random(), 1, None);
    let dead_peer = Member::with_status(
        dead_peer_id.clone(),
        "127.0.0.1:19441".parse().unwrap(),
        MemberStatus::Dead,
        1,
    );
    table.upsert(dead_peer).await;

    // 1. Immediately after marking Dead, tombstone retention must preserve it
    let reaped = table.reap_tombstones(Duration::from_millis(100)).await;
    assert_eq!(
        reaped, 0,
        "Recent Dead entries must be retained in tombstone window"
    );
    assert!(table.get(&dead_peer_id.id()).await.is_some());

    // 2. Wait past tombstone timeout
    tokio::time::sleep(Duration::from_millis(120)).await;

    // 3. GC purges expired tombstone
    let reaped = table.reap_tombstones(Duration::from_millis(100)).await;
    assert_eq!(reaped, 1, "Expired tombstone must be purged");
    assert!(table.get(&dead_peer_id.id()).await.is_none());
}
