use control_plane_service::{ControlPlaneConfig, ControlPlaneNode, ControlPlaneService};
use membership_service::{Member, MemberStatus};
use node::{Context, Env, EventHub, Network, NodeId, Service, Store, Uuid};
use shard_service::{
    ShardConfig, ShardCoordinator, ShardError, ShardEvent, ShardHandle, ShardPlacement, ShardRole,
    ShardService,
};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

#[tokio::main]
#[test]
async fn test_multi_service_group_isolation_and_namespacing() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir().unwrap();
    let store = Store::open(&tmp).unwrap();
    let network = Network::new();
    let event_hub = EventHub::new();
    let env = Arc::new(Env::detect());
    let uuid_root = Uuid::random();
    let identity = NodeId::new(uuid_root, 1, None);
    let token = CancellationToken::new();

    let ctx = Context::new(event_hub.clone(), network, store, identity, env, token.clone());

    let cp_port = 19101;
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
        total_shards: 8,
        replication_factor: 3,
        is_coordinator: true,
    });

    let cp_ctx = ctx.clone();
    tokio::spawn(async move {
        let _ = cp_svc.run(cp_ctx).await;
    });

    let shard_ctx = ctx.clone();
    tokio::spawn(async move {
        let _ = shard_svc.run(shard_ctx).await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Initialize Control Plane Raft
    let mut voters = BTreeMap::new();
    voters.insert(1, ControlPlaneNode::new(format!("127.0.0.1:{cp_port}"), uuid_root));
    let _ = cp_handle.initialize(voters).await;

    for _ in 0..50 {
        if cp_handle.is_leader() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let coord = ShardCoordinator::new(
        ShardConfig {
            total_shards: 8,
            replication_factor: 3,
            is_coordinator: true,
        },
        cp_handle.clone(),
        shard_handle.clone(),
    );

    // Nodes for "treasurer" service
    let t1 = Uuid::random();
    let t2 = Uuid::random();
    let t3 = Uuid::random();

    // Nodes for "bank" service
    let b1 = Uuid::random();
    let b2 = Uuid::random();
    let b3 = Uuid::random();
    let b4 = Uuid::random();

    // 1. Bootstrap service "treasurer" (8 shards)
    let assigned_treasurer = coord
        .bootstrap_service_round_robin("treasurer", &[t1, t2, t3], Some(&ctx))
        .await?;
    assert_eq!(assigned_treasurer, 8);

    // 2. Bootstrap service "bank" (8 shards)
    let assigned_bank = coord
        .bootstrap_service_round_robin("bank", &[b1, b2, b3, b4], Some(&ctx))
        .await?;
    assert_eq!(assigned_bank, 8);

    // 3. Verify total counts in handle
    assert_eq!(shard_handle.assigned_service_count("treasurer").await, 8);
    assert_eq!(shard_handle.assigned_service_count("bank").await, 8);
    assert_eq!(shard_handle.assigned_count().await, 16);

    // 4. Verify that treasurer shards strictly contain ONLY treasurer nodes
    let treasurer_placements = shard_handle.all_service_placements("treasurer").await;
    assert_eq!(treasurer_placements.len(), 8);
    for p in treasurer_placements {
        assert_eq!(p.service_name, "treasurer");
        assert!(p.all_nodes().iter().all(|n| *n == t1 || *n == t2 || *n == t3));
        assert!(!p.all_nodes().iter().any(|n| *n == b1 || *n == b2 || *n == b3 || *n == b4));
    }

    // 5. Verify that bank shards strictly contain ONLY bank nodes
    let bank_placements = shard_handle.all_service_placements("bank").await;
    assert_eq!(bank_placements.len(), 8);
    for p in bank_placements {
        assert_eq!(p.service_name, "bank");
        assert!(p.all_nodes().iter().all(|n| *n == b1 || *n == b2 || *n == b3 || *n == b4));
        assert!(!p.all_nodes().iter().any(|n| *n == t1 || *n == t2 || *n == t3));
    }

    // 6. Verify duplicate bootstrap is rejected per service
    let dup_treasurer = coord.bootstrap_service_round_robin("treasurer", &[t1, t2, t3], Some(&ctx)).await;
    assert!(matches!(dup_treasurer, Err(ShardError::AlreadyBootstrapped)));

    // 7. Test manual reassignment in service "bank" shard 0 without touching "treasurer" shard 0
    let manual_bank = coord
        .assign_service_manual("bank", 0, b2, vec![b3, b4], Some(&ctx))
        .await?;
    assert_eq!(manual_bank.service_name, "bank");
    assert_eq!(manual_bank.primary, b2);

    // Verify "treasurer" shard 0 remains unaffected
    let treasurer_s0 = shard_handle.get_service_placement("treasurer", 0).await.unwrap();
    assert_eq!(treasurer_s0.service_name, "treasurer");
    assert_eq!(treasurer_s0.primary, t1);

    token.cancel();
    Ok(())
}

#[test]
fn test_strict_control_plane_exclusion_under_discovery() {
    let dummy_config = ShardConfig::default();
    let (cp_svc, cp_handle) = ControlPlaneService::pair();
    let dummy_handle = ShardHandle::new(Uuid::random(), 1024);
    let coord = ShardCoordinator::new(dummy_config, cp_handle, dummy_handle);

    drop(cp_svc);

    let dummy_addr: SocketAddr = "127.0.0.1:18000".parse().unwrap();

    // Node 1: Dedicated Control Plane node
    let cp_node_1 = Member::new(NodeId::new(Uuid::random(), 1, None), dummy_addr)
        .with_tags(vec!["role:control-plane".to_string()]);

    // Node 2: Dedicated Control Plane node with prefix
    let cp_node_2 = Member::new(NodeId::new(Uuid::random(), 1, None), dummy_addr)
        .with_tags(vec!["role:control-plane-raft".to_string()]);

    // Node 3: Adversarial node (has both service tag AND control-plane tag)
    let bad_cp_node = Member::new(NodeId::new(Uuid::random(), 1, None), dummy_addr)
        .with_tags(vec!["service:treasurer".to_string(), "role:control-plane".to_string()]);

    // Node 4: Dead treasurer node
    let mut dead_treasurer = Member::new(NodeId::new(Uuid::random(), 1, None), dummy_addr)
        .with_tags(vec!["service:treasurer".to_string()]);
    dead_treasurer.status = MemberStatus::Dead;

    // Node 5 & 6: Legitimate Alive Treasurer nodes
    let good_treasurer_1 = Member::new(NodeId::new(Uuid::random(), 1, None), dummy_addr)
        .with_tags(vec!["service:treasurer".to_string()]);
    let good_treasurer_2 = Member::new(NodeId::new(Uuid::random(), 1, None), dummy_addr)
        .with_tags(vec!["treasurer".to_string()]);

    // Node 7: Alive Bank node (different service)
    let bank_node = Member::new(NodeId::new(Uuid::random(), 1, None), dummy_addr)
        .with_tags(vec!["service:bank".to_string()]);

    let members = vec![
        cp_node_1,
        cp_node_2,
        bad_cp_node,
        dead_treasurer,
        good_treasurer_1.clone(),
        good_treasurer_2.clone(),
        bank_node,
    ];

    // Filter nodes for "treasurer"
    let selected = coord.filter_service_nodes("treasurer", &members);

    // Assert: Only good_treasurer_1 and good_treasurer_2 must be selected!
    assert_eq!(selected.len(), 2);
    assert!(selected.contains(&good_treasurer_1.node_id.id()));
    assert!(selected.contains(&good_treasurer_2.node_id.id()));
}

#[tokio::test]
async fn test_leader_announcement_and_role_transitions() {
    let node_a = Uuid::random();
    let node_b = Uuid::random();
    let node_c = Uuid::random();

    let handle_a = ShardHandle::new(node_a, 16);
    let handle_b = ShardHandle::new(node_b, 16);

    let initial_placement = ShardPlacement::with_service(
        "orders",
        5,
        node_a,
        vec![node_b, node_c],
        100,
    );

    handle_a.update_placement(initial_placement.clone()).await;
    handle_b.update_placement(initial_placement).await;

    // Initially, node_a is Primary and node_b is Replica
    assert_eq!(handle_a.my_role(5).await, Some(ShardRole::Primary));
    assert_eq!(handle_b.my_role(5).await, Some(ShardRole::Replica));

    // Dynamic Failover: Node B wins election in its local Raft quórum for shard 5
    handle_a.announce_leader("orders", 5, node_b, 2).await;
    handle_b.announce_leader("orders", 5, node_b, 2).await;

    // Verify placements reflect Node B as the new primary
    let updated_on_a = handle_a.get_service_placement("orders", 5).await.unwrap();
    let updated_on_b = handle_b.get_service_placement("orders", 5).await.unwrap();
    assert_eq!(updated_on_a.primary, node_b);
    assert_eq!(updated_on_b.primary, node_b);

    // Verify roles transition accurately: Node A steps down to Replica, Node B becomes Primary
    assert_eq!(handle_a.my_role(5).await, Some(ShardRole::Replica));
    assert_eq!(handle_a.my_service_role("orders", 5).await, Some(ShardRole::Replica));
    assert_eq!(handle_b.my_role(5).await, Some(ShardRole::Primary));
    assert_eq!(handle_b.my_service_role("orders", 5).await, Some(ShardRole::Primary));
    assert!(handle_b.is_my_primary(5).await);
    assert!(!handle_b.is_my_replica(5).await);

    // Verify quórum invariants: leader is not in replicas, old leader is in replicas
    assert!(!updated_on_a.replicas.contains(&node_b));
    assert!(updated_on_a.replicas.contains(&node_a));
    assert_eq!(updated_on_a.node_count(), 3);
}

#[test]
fn test_flatbuffers_fuzzing_and_corrupted_payloads() {
    // 1. Empty slice
    assert!(ShardPlacement::from_bytes(&[]).is_err());

    // 2. Random binary garbage
    let random_noise: Vec<u8> = (0..64).map(|i| ((i * 37) % 256) as u8).collect();
    assert!(ShardPlacement::from_bytes(&random_noise).is_err());

    // 3. Truncated valid FlatBuffers payload
    let valid_placement = ShardPlacement::with_service(
        "fuzz_service",
        999,
        Uuid::random(),
        vec![Uuid::random(), Uuid::random()],
        u64::MAX,
    );
    let valid_bytes = valid_placement.to_bytes();
    assert!(!valid_bytes.is_empty());

    // Truncate at various lengths
    for len in 1..valid_bytes.len() {
        let truncated = &valid_bytes[..len];
        let _ = ShardPlacement::from_bytes(truncated); // Must not panic!
    }

    // 4. Boundary values (extremes)
    let extreme_placement = ShardPlacement::with_service(
        "a".repeat(1000), // very long service name
        u32::MAX,
        Uuid::new(u64::MAX, u64::MAX),
        vec![Uuid::new(0, 0)],
        u64::MAX,
    );
    let bytes = extreme_placement.to_bytes();
    let recovered = ShardPlacement::from_bytes(&bytes).expect("Should deserialize extreme values");
    assert_eq!(extreme_placement.shard_id, recovered.shard_id);
    assert_eq!(extreme_placement.service_name, recovered.service_name);
    assert_eq!(extreme_placement.epoch, recovered.epoch);
}

#[tokio::test]
async fn test_concurrent_multi_service_placements_and_announcements() {
    let handle = Arc::new(ShardHandle::new(Uuid::random(), 1024));
    let mut tasks = Vec::new();

    let services = ["treasurer", "bank", "orders", "inventory", "auth"];

    for task_id in 0..50 {
        let handle_clone = handle.clone();
        tasks.push(tokio::spawn(async move {
            let service = services[task_id % services.len()];
            let shard_id = (task_id % 16) as u32;
            let primary = Uuid::random();
            let replicas = vec![Uuid::random(), Uuid::random()];

            // Update placement
            let p = ShardPlacement::with_service(service, shard_id, primary, replicas, 1000 + task_id as u64);
            handle_clone.update_placement(p).await;

            // Announce leader change
            let new_leader = Uuid::random();
            handle_clone.announce_leader(service, shard_id, new_leader, 1).await;

            // Query
            let fetched = handle_clone.get_service_placement(service, shard_id).await;
            assert!(fetched.is_some());
            assert_eq!(fetched.unwrap().primary, new_leader);
        }));
    }

    for t in tasks {
        t.await.expect("Concurrent task failed");
    }

    // Verify all 5 services have placements registered
    for s in services {
        assert!(handle.assigned_service_count(s).await > 0);
    }
}

#[tokio::test]
async fn test_rebalance_group_membership_transition() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir().unwrap();
    let store = Store::open(&tmp).unwrap();
    let network = Network::new();
    let event_hub = EventHub::new();
    let env = Arc::new(Env::detect());
    let uuid_root = Uuid::random();
    let identity = NodeId::new(uuid_root, 1, None);
    let token = CancellationToken::new();

    let ctx = Context::new(event_hub.clone(), network, store, identity, env, token.clone());

    let cp_port = 19102;
    let cp_config = ControlPlaneConfig {
        bind_addr: format!("127.0.0.1:{cp_port}").parse().unwrap(),
        node_id: Some(1),
        election_timeout_min_ms: 100,
        election_timeout_max_ms: 200,
        heartbeat_interval_ms: 30,
        snapshot_threshold: 50,
    };

    let (cp_svc, cp_handle) = ControlPlaneService::pair_with_config(cp_config);
    let (_shard_svc, shard_handle) = ShardService::coordinator(cp_handle.clone());

    let cp_ctx = ctx.clone();
    tokio::spawn(async move {
        let _ = cp_svc.run(cp_ctx).await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut voters = BTreeMap::new();
    voters.insert(1, ControlPlaneNode::new(format!("127.0.0.1:{cp_port}"), uuid_root));
    let _ = cp_handle.initialize(voters).await;

    for _ in 0..50 {
        if cp_handle.is_leader() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let coord = ShardCoordinator::new(
        ShardConfig {
            total_shards: 8,
            replication_factor: 3,
            is_coordinator: true,
        },
        cp_handle.clone(),
        shard_handle.clone(),
    );

    let n_primary = Uuid::random();
    let n_rep1 = Uuid::random();
    let n_rep2_old = Uuid::random();
    let n_rep2_new = Uuid::random();

    // 1. Initial manual placement for shard 3 in service "treasurer"
    let p1 = coord
        .assign_service_manual("treasurer", 3, n_primary, vec![n_rep1, n_rep2_old], Some(&ctx))
        .await?;
    assert_eq!(p1.primary, n_primary);
    assert_eq!(p1.replicas, vec![n_rep1, n_rep2_old]);

    let handle_old = ShardHandle::new(n_rep2_old, 8);
    handle_old.update_placement(p1.clone()).await;
    assert_eq!(handle_old.my_role(3).await, Some(ShardRole::Replica));

    // 2. Rebalance: replace n_rep2_old with n_rep2_new
    let p2 = coord
        .assign_service_manual("treasurer", 3, n_primary, vec![n_rep1, n_rep2_new], Some(&ctx))
        .await?;
    assert!(p2.epoch >= p1.epoch);

    handle_old.update_placement(p2.clone()).await;
    // Old node is no longer part of shard 3
    assert_eq!(handle_old.my_role(3).await, None);

    let handle_new = ShardHandle::new(n_rep2_new, 8);
    handle_new.update_placement(p2).await;
    // New node is now confirmed replica
    assert_eq!(handle_new.my_role(3).await, Some(ShardRole::Replica));

    token.cancel();
    Ok(())
}

#[tokio::test]
async fn test_control_plane_full_reconstruction_from_raft() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir().unwrap();
    let store = Store::open(&tmp).unwrap();
    let network = Network::new();
    let event_hub = EventHub::new();
    let env = Arc::new(Env::detect());
    let uuid_root = Uuid::random();
    let identity = NodeId::new(uuid_root, 1, None);
    let token = CancellationToken::new();

    let ctx = Context::new(event_hub.clone(), network, store, identity, env, token.clone());

    let cp_port = 19103;
    let cp_config = ControlPlaneConfig {
        bind_addr: format!("127.0.0.1:{cp_port}").parse().unwrap(),
        node_id: Some(1),
        election_timeout_min_ms: 100,
        election_timeout_max_ms: 200,
        heartbeat_interval_ms: 30,
        snapshot_threshold: 50,
    };

    let (cp_svc, cp_handle) = ControlPlaneService::pair_with_config(cp_config);
    let (_shard_svc, shard_handle) = ShardService::coordinator(cp_handle.clone());

    let cp_ctx = ctx.clone();
    tokio::spawn(async move {
        let _ = cp_svc.run(cp_ctx).await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut voters = BTreeMap::new();
    voters.insert(1, ControlPlaneNode::new(format!("127.0.0.1:{cp_port}"), uuid_root));
    let _ = cp_handle.initialize(voters).await;

    for _ in 0..50 {
        if cp_handle.is_leader() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let coord = ShardCoordinator::new(
        ShardConfig {
            total_shards: 4,
            replication_factor: 3,
            is_coordinator: true,
        },
        cp_handle.clone(),
        shard_handle.clone(),
    );

    // Bootstrap two separate services
    let nodes_treasurer = [Uuid::random(), Uuid::random(), Uuid::random()];
    let nodes_bank = [Uuid::random(), Uuid::random(), Uuid::random()];

    coord.bootstrap_service_round_robin("treasurer", &nodes_treasurer, Some(&ctx)).await?;
    coord.bootstrap_service_round_robin("bank", &nodes_bank, Some(&ctx)).await?;

    // Now simulate a brand new node booting up: completely empty ShardHandle
    let fresh_handle = ShardHandle::new(Uuid::random(), 4);
    assert_eq!(fresh_handle.assigned_count().await, 0);
    assert!(!fresh_handle.is_service_bootstrapped("treasurer").await);
    assert!(!fresh_handle.is_service_bootstrapped("bank").await);

    let fresh_coord = ShardCoordinator::new(
        ShardConfig {
            total_shards: 4,
            replication_factor: 3,
            is_coordinator: true,
        },
        cp_handle.clone(),
        fresh_handle.clone(),
    );

    // Run sync_from_raft to rebuild state
    fresh_coord.sync_from_raft(&ctx).await;

    // Verify state was completely and accurately recovered!
    assert_eq!(fresh_handle.assigned_service_count("treasurer").await, 4);
    assert_eq!(fresh_handle.assigned_service_count("bank").await, 4);
    assert_eq!(fresh_handle.assigned_count().await, 8);
    assert!(fresh_handle.is_service_bootstrapped("treasurer").await);
    assert!(fresh_handle.is_service_bootstrapped("bank").await);

    let treasurer_placements = fresh_handle.all_service_placements("treasurer").await;
    assert_eq!(treasurer_placements.len(), 4);
    for p in treasurer_placements {
        assert_eq!(p.service_name, "treasurer");
        assert!(nodes_treasurer.contains(&p.primary));
    }

    token.cancel();
    Ok(())
}

#[tokio::test]
async fn test_multi_service_role_ambiguity_elimination() {
    let local_node = Uuid::random();
    let other_node_1 = Uuid::random();
    let other_node_2 = Uuid::random();

    let handle = ShardHandle::new(local_node, 16);

    // Shard 0 for service "treasurer": local_node is Primary
    let p_treasurer = ShardPlacement::with_service(
        "treasurer",
        0,
        local_node,
        vec![other_node_1, other_node_2],
        10,
    );
    handle.update_placement(p_treasurer).await;

    // Shard 0 for service "bank": local_node is Replica (other_node_1 is Primary)
    let p_bank = ShardPlacement::with_service(
        "bank",
        0,
        other_node_1,
        vec![local_node, other_node_2],
        10,
    );
    handle.update_placement(p_bank).await;

    // Verify my_service_role disambiguates the exact same shard_id (0) across services
    assert_eq!(
        handle.my_service_role("treasurer", 0).await,
        Some(ShardRole::Primary)
    );
    assert!(handle.is_my_service_primary("treasurer", 0).await);
    assert!(!handle.is_my_service_replica("treasurer", 0).await);

    assert_eq!(
        handle.my_service_role("bank", 0).await,
        Some(ShardRole::Replica)
    );
    assert!(handle.is_my_service_replica("bank", 0).await);
    assert!(!handle.is_my_service_primary("bank", 0).await);

    // Verify non-existent service query returns None cleanly
    assert_eq!(handle.my_service_role("non_existent", 0).await, None);
}

#[tokio::test]
async fn test_raft_native_group_assigned_and_role_transitions() {
    let my_node = Uuid::random();
    let peer_1 = Uuid::random();
    let peer_2 = Uuid::random();

    let event_hub = EventHub::new();
    let _handle = ShardHandle::new(my_node, 16);
    let token = CancellationToken::new();

    let mut event_sub = event_hub.subscribe::<ShardEvent>().await;

    // 1. Simulate worker atomic Bootstrap with multiple shards
    let shard_id = 7;
    let members = vec![peer_1, my_node, peer_2];

    event_hub
        .publish(ShardEvent::Bootstrap {
            shards: vec![
                node::ShardGroup {
                    shard_id: 1,
                    members: members.clone(),
                    role: node::MemberRole::Learner,
                },
                node::ShardGroup {
                    shard_id: 2,
                    members: members.clone(),
                    role: node::MemberRole::Voter,
                },
            ],
        })
        .await;

    let event = event_sub.recv().await.expect("Failed to receive ShardEvent");
    match event {
        ShardEvent::Bootstrap { shards } => {
            assert_eq!(shards.len(), 2);
            assert_eq!(shards[0].shard_id, 1);
            assert_eq!(shards[0].role, node::MemberRole::Learner);
            assert_eq!(shards[1].shard_id, 2);
            assert_eq!(shards[1].role, node::MemberRole::Voter);
        }
        _ => panic!("Expected Bootstrap event"),
    }

    // 2. Simulate dynamic runtime Join of a new shard
    event_hub
        .publish(ShardEvent::Join {
            shard_id,
            members: members.clone(),
            role: node::MemberRole::Voter,
        })
        .await;

    // Verify Join event received
    let event = event_sub.recv().await.expect("Failed to receive ShardEvent");
    match event {
        ShardEvent::Join {
            shard_id: sid,
            members: m,
            role,
        } => {
            assert_eq!(sid, 7);
            assert_eq!(m.len(), 3);
            assert_eq!(role, node::MemberRole::Voter);
        }
        _ => panic!("Expected Join event"),
    }

    // Simulate role transition to Leader
    event_hub
        .publish(ShardEvent::RoleChanged {
            shard_id,
            role: node::MemberRole::Leader,
        })
        .await;

    let event = event_sub.recv().await.expect("Failed to receive ShardEvent");
    match event {
        ShardEvent::RoleChanged {
            shard_id: sid,
            role,
        } => {
            assert_eq!(sid, 7);
            assert_eq!(role, node::MemberRole::Leader);
        }
        _ => panic!("Expected RoleChanged event"),
    }

    // Simulate role transition back to Voter
    event_hub
        .publish(ShardEvent::RoleChanged {
            shard_id,
            role: node::MemberRole::Voter,
        })
        .await;

    let event = event_sub.recv().await.expect("Failed to receive ShardEvent");
    match event {
        ShardEvent::RoleChanged {
            shard_id: sid,
            role,
        } => {
            assert_eq!(sid, 7);
            assert_eq!(role, node::MemberRole::Voter);
        }
        _ => panic!("Expected RoleChanged event"),
    }

    // Simulate Leave
    event_hub
        .publish(ShardEvent::Leave {
            shard_id,
        })
        .await;

    let event = event_sub.recv().await.expect("Failed to receive ShardEvent");
    match event {
        ShardEvent::Leave { shard_id: sid } => {
            assert_eq!(sid, 7);
        }
        _ => panic!("Expected Leave event"),
    }

    token.cancel();
}
