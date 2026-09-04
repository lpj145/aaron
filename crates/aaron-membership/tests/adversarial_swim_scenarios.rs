//! Exploratory/adversarial chaos scenarios for the SWIM protocol implementation, beyond
//! the existing chaos/fuzz coverage. These document current behavior under hostile or
//! extreme inputs — they do not modify any production code, and some are expected to
//! currently fail, which is the point: they pinpoint exactly where the protocol's
//! guarantees break down.

use aaron_membership::{
    Member, MemberStatus, MembershipConfig, MembershipEvent, MembershipService, MembershipTable,
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

/// The self-refutation path computes `local.incarnation = update.incarnation + 1`. A
/// forged or corrupted claim about the local node carrying `incarnation = u64::MAX` would
/// overflow that addition. Run the upsert inside a spawned task so a panic surfaces as a
/// clean assertion failure (`JoinError::is_panic()`) instead of aborting the test binary.
#[tokio::test]
async fn test_self_refutation_at_max_incarnation_does_not_panic() {
    let local_id = NodeId::new(Uuid::random(), u64::MAX, None);
    let local_addr: SocketAddr = "127.0.0.1:19500".parse().unwrap();
    let table = MembershipTable::new(local_id.clone(), local_addr);

    let forged_claim = Member::with_status(local_id, local_addr, MemberStatus::Suspect, u64::MAX);

    let table_clone = table.clone();
    let join_result = tokio::spawn(async move { table_clone.upsert(forged_claim).await }).await;

    assert!(
        join_result.is_ok(),
        "MembershipTable::upsert panicked while self-refuting a claim at incarnation \
         u64::MAX (arithmetic overflow on `update.incarnation + 1`): {:?}",
        join_result.err()
    );
}

/// SWIM's incarnation ordering has no cryptographic binding to the node that supposedly
/// issued it: any already-authorized cluster peer can gossip a claim about a *third*,
/// uninvolved node carrying an arbitrarily inflated incarnation. This documents that a
/// single gossiping peer can unilaterally declare a healthy peer permanently Dead this way.
#[tokio::test]
async fn test_any_peer_can_forge_arbitrary_incarnation_for_an_uninvolved_third_party() {
    let cluster_id = Uuid::random();
    let local_id = NodeId::new(Uuid::random(), 1, Some(cluster_id));
    let local_addr: SocketAddr = "127.0.0.1:19510".parse().unwrap();
    let table = MembershipTable::new(local_id, local_addr);

    // A legitimate, currently-healthy third party at incarnation 5.
    let victim_id = NodeId::new(Uuid::random(), 5, Some(cluster_id));
    let victim_addr: SocketAddr = "127.0.0.1:19511".parse().unwrap();
    let victim = Member::with_status(victim_id.clone(), victim_addr, MemberStatus::Alive, 5);
    table.upsert(victim).await;
    assert!(table.get(&victim_id.id()).await.unwrap().is_alive());

    // Nothing in `upsert` verifies that a Dead/incarnation claim about `victim_id`
    // actually originated from (or was authorized by) the victim itself.
    let forged_dead_claim =
        Member::with_status(victim_id.clone(), victim_addr, MemberStatus::Dead, 9_999);
    table.upsert(forged_dead_claim).await;

    let after = table.get(&victim_id.id()).await.unwrap();
    assert!(
        after.is_dead(),
        "expected the current (unsigned) incarnation trust model to accept a forged Dead \
         claim about an uninvolved third party; if this assertion now fails, incarnation \
         claims have gained provenance checking and this test's premise should be revisited"
    );
}

/// A thundering herd of joiners hitting a single bootstrap seed at the same instant,
/// rather than staggered like the existing churn/rejoin test. Explores whether the seed's
/// concurrent JoinRequest handling and gossip convergence hold up under a burst instead
/// of a trickle.
#[tokio::test]
async fn test_thundering_herd_simultaneous_joins_to_single_seed_converge() {
    const JOINERS: usize = 10;
    let cluster_id = Uuid::new(0x1111_2222, 0x3333_4444);
    let seed_port = 19520;

    let seed_token = CancellationToken::new();
    let (seed_ctx, _seed_tmp) = make_test_context(seed_token.clone()).await;
    let seed_config = MembershipConfig {
        bind_addr: format!("127.0.0.1:{seed_port}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(150),
        probe_timeout: Duration::from_millis(80),
        suspect_timeout: Duration::from_millis(1500),
        indirect_ping_targets: 3,
        gossip_fanout: 6,
    };
    let (seed_service, seed_handle) = MembershipService::pair_with_config(seed_config);
    let seed_ctx_task = seed_ctx.clone();
    let seed_task = tokio::spawn(async move {
        seed_service.run(seed_ctx_task).await.unwrap();
    });
    seed_handle.wait_ready().await;

    // Fire all joiners at (as close to) the same instant, with no staggering.
    let mut joiner_tasks = Vec::new();
    let mut joiner_tokens = Vec::new();
    let mut joiner_ids = Vec::new();
    for i in 0..JOINERS {
        let token = CancellationToken::new();
        let (ctx, _tmp) = make_test_context(token.clone()).await;
        joiner_ids.push(ctx.identity.id());
        let config = MembershipConfig {
            bind_addr: format!("127.0.0.1:{}", 19521 + i),
            seeds: vec![format!("127.0.0.1:{seed_port}")],
            cluster_id: Some(cluster_id),
            probe_interval: Duration::from_millis(150),
            probe_timeout: Duration::from_millis(80),
            suspect_timeout: Duration::from_millis(1500),
            indirect_ping_targets: 3,
            gossip_fanout: 6,
        };
        let (service, handle) = MembershipService::pair_with_config(config);
        let ctx_task = ctx.clone();
        joiner_tasks.push((
            tokio::spawn(async move { service.run(ctx_task).await.unwrap() }),
            handle,
        ));
        joiner_tokens.push(token);
        // Deliberately no delay/stagger here — that's the thundering-herd point.
    }

    for (_, handle) in &joiner_tasks {
        handle.wait_ready().await;
    }

    // Wait for full mesh convergence: seed must see all joiners, and every joiner must
    // see the full cluster (seed + every other joiner).
    let expected_size = JOINERS + 1;
    let mut converged = false;
    for _ in 0..100 {
        let seed_ok = seed_handle.active_members().await.len() == expected_size;
        let joiners_ok = {
            let mut all_ok = true;
            for (_, handle) in &joiner_tasks {
                if handle.active_members().await.len() != expected_size {
                    all_ok = false;
                    break;
                }
            }
            all_ok
        };
        if seed_ok && joiners_ok {
            converged = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        converged,
        "cluster failed to converge to {expected_size} members within timeout after a \
         thundering-herd join burst of {JOINERS} simultaneous joiners; seed saw {} members",
        seed_handle.active_members().await.len()
    );

    seed_token.cancel();
    for t in &joiner_tokens {
        t.cancel();
    }
    let _ = seed_task.await;
    for (task, _) in joiner_tasks {
        let _ = task.await;
    }
}

/// Floods a live node with a burst of well-formed but bogus `Ping` messages carrying large
/// fabricated gossip payloads, concurrently with a genuine peer crashing. Explores whether
/// gossip-processing overhead from the flood can starve the failure detector and delay (or
/// prevent) marking the crashed peer Dead within its configured timeout.
#[tokio::test]
async fn test_gossip_flood_does_not_starve_failure_detection() {
    let cluster_id = Uuid::new(0x5555_6666, 0x7777_8888);
    let node_a_port = 19540;
    let node_b_port = 19541;

    let token_a = CancellationToken::new();
    let (ctx_a, _tmp_a) = make_test_context(token_a.clone()).await;
    let config_a = MembershipConfig {
        bind_addr: format!("127.0.0.1:{node_a_port}"),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(60),
        suspect_timeout: Duration::from_millis(400),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };
    let (service_a, handle_a) = MembershipService::pair_with_config(config_a);
    let mut sub_a = ctx_a.event_hub.subscribe::<MembershipEvent>().await;
    let ctx_a_task = ctx_a.clone();
    let task_a = tokio::spawn(async move { service_a.run(ctx_a_task).await.unwrap() });
    handle_a.wait_ready().await;

    let token_b = CancellationToken::new();
    let (ctx_b, _tmp_b) = make_test_context(token_b.clone()).await;
    let config_b = MembershipConfig {
        bind_addr: format!("127.0.0.1:{node_b_port}"),
        seeds: vec![format!("127.0.0.1:{node_a_port}")],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(100),
        probe_timeout: Duration::from_millis(60),
        suspect_timeout: Duration::from_millis(400),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };
    let (service_b, handle_b) = MembershipService::pair_with_config(config_b);
    let ctx_b_task = ctx_b.clone();
    let task_b = tokio::spawn(async move { service_b.run(ctx_b_task).await.unwrap() });
    handle_b.wait_ready().await;

    // Wait for A to see B join.
    let joined = tokio::time::timeout(Duration::from_secs(3), sub_a.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(joined, MembershipEvent::Joined(ref m) if m.node_id.id() == ctx_b.identity.id())
    );

    let target_addr: SocketAddr = format!("127.0.0.1:{node_a_port}").parse().unwrap();

    // Crash node B abruptly.
    token_b.cancel();
    let _ = task_b.await;

    // Concurrently, flood node A with attacker connections sending well-formed Ping
    // messages carrying large fabricated gossip payloads (within the same cluster, so
    // they aren't gate-rejected by cluster_id isolation).
    let mut flood_tasks = Vec::new();
    for i in 0..6 {
        flood_tasks.push(tokio::spawn(async move {
            let attacker_net = Network::new();
            for j in 0..40 {
                if let Ok(conn) = attacker_net.quic.connect(target_addr, "localhost").await
                    && let Ok((mut send, _recv)) = conn.open_bi().await
                {
                    let sender = Member::new(
                        NodeId::new(Uuid::random(), 1, Some(cluster_id)),
                        "127.0.0.1:1".parse().unwrap(),
                    );
                    let fabricated_gossip: Vec<Member> = (0..64)
                        .map(|k| {
                            Member::with_status(
                                NodeId::new(Uuid::random(), 1, Some(cluster_id)),
                                format!("127.0.0.1:{}", 20000 + ((i * 40 + j + k) % 5000))
                                    .parse()
                                    .unwrap(),
                                MemberStatus::Alive,
                                1,
                            )
                        })
                        .collect();
                    let flood_ping = Message::Ping {
                        seq: (i * 1000 + j) as u64,
                        sender,
                        gossip: fabricated_gossip,
                    };
                    let _ = write_frame(&mut send, &flood_ping.to_bytes()).await;
                    let _ = send.finish();
                }
            }
        }));
    }

    // Node A must still detect B as Dead within a bounded time despite the concurrent flood.
    let dead_detected = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match sub_a.recv().await {
                Ok(MembershipEvent::Dead(ref m)) if m.node_id.id() == ctx_b.identity.id() => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    })
    .await;

    for t in flood_tasks {
        let _ = t.await;
    }

    assert!(
        dead_detected.is_ok(),
        "node A failed to detect crashed peer B as Dead within 5s while under a concurrent \
         gossip flood — failure detection may be starved by gossip-processing overhead"
    );

    token_a.cancel();
    let _ = task_a.await;
}
