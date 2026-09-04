use aaron_control_plane::{ControlPlaneConfig, ControlPlaneNode, ControlPlaneService};
use aaron_core::{Context, Env, EventHub, Network, NodeId, Service, Store, Uuid};
use aaron_shard::{ShardConfig, ShardCoordinator, ShardError, ShardEvent, ShardRole, ShardService};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

#[tokio::main]
#[test]
async fn test_stage1_round_robin_and_manual_assignment() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir().unwrap();
    let store = Store::open(&tmp).unwrap();
    let network = Network::new();
    let event_hub = EventHub::new();
    let env = Arc::new(Env::detect());
    let uuid_a = Uuid::random();
    let identity = NodeId::new(uuid_a, 1, None);
    let token = CancellationToken::new();

    let ctx = Context::new(event_hub.clone(), network, store, identity, env, token.clone());

    let cp_port = 18995;
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
        total_shards: 16,
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

    let coord = ShardCoordinator::new(
        ShardConfig {
            total_shards: 16,
            replication_factor: 3,
            is_coordinator: true,
        },
        cp_handle.clone(),
        shard_handle.clone(),
    );

    let uuid_b = Uuid::random();
    let uuid_c = Uuid::random();

    // 1. Antes do Raft estar ativo, qualquer tentativa de designação DEVE falhar
    let pre_raft_res = coord.bootstrap_round_robin(&[uuid_a, uuid_b, uuid_c], Some(&ctx)).await;
    assert!(matches!(pre_raft_res, Err(ShardError::ControlPlaneUnavailable)));

    // 2. Inicializa o quórum do Control Plane
    let mut voters = BTreeMap::new();
    voters.insert(1, ControlPlaneNode::new(format!("127.0.0.1:{cp_port}"), uuid_a));
    let init_res = cp_handle.initialize(voters).await;
    eprintln!("DEBUG TEST: init_res = {:?}", init_res);
    assert!(init_res.is_ok(), "Bootstrap should succeed: {:?}", init_res);

    let mut elected = false;
    for _ in 0..50 {
        if cp_handle.is_leader() {
            elected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(elected, "Control plane leader should be elected");

    // 3. Validação: Round-Robin com menos de 3 nós DEVE falhar
    let few_nodes_res = coord.bootstrap_round_robin(&[uuid_a, uuid_b], Some(&ctx)).await;
    eprintln!("DEBUG TEST: few_nodes_res = {:?}", few_nodes_res);
    assert!(matches!(few_nodes_res, Err(ShardError::InsufficientNodes { count: 2 })));

    // 4. MODO 1: Bootstrap Round-Robin com 3 nós (16 shards)
    let assigned_count = coord
        .bootstrap_round_robin(&[uuid_a, uuid_b, uuid_c], Some(&ctx))
        .await?;
    assert_eq!(assigned_count, 16);

    // Tentativa de executar Bootstrap uma 2ª vez DEVE ser estritamente bloqueada
    let second_bootstrap = coord
        .bootstrap_round_robin(&[uuid_a, uuid_b, uuid_c], Some(&ctx))
        .await;
    assert!(matches!(second_bootstrap, Err(ShardError::AlreadyBootstrapped)));

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verifica que todos os 16 shards foram designados e sincronizados
    assert_eq!(shard_handle.assigned_count().await, 16);
    let shard_0 = shard_handle.get_placement(0).await.unwrap();
    assert_eq!(shard_0.primary, uuid_a);
    assert_eq!(shard_0.replicas, vec![uuid_b, uuid_c]);
    assert_eq!(shard_0.node_count(), 3);

    // 5. MODO 2: Validações da Designação Manual
    // a) Menos de 3 nós distintos -> Falha
    let manual_few = coord.assign_manual(0, uuid_a, vec![uuid_b], Some(&ctx)).await;
    assert!(matches!(manual_few, Err(ShardError::InsufficientNodes { count: 2 })));

    // b) Primary duplicado na lista de réplicas -> Falha
    let manual_dup = coord.assign_manual(0, uuid_a, vec![uuid_a, uuid_b], Some(&ctx)).await;
    assert!(matches!(manual_dup, Err(ShardError::DuplicateNodeAssignment { .. })));

    // c) Designação Manual válida (1 Primary + 2 Réplicas)
    let mut shard_sub = ctx.event_hub.subscribe::<ShardEvent>().await;
    let manual_ok = coord.assign_manual(0, uuid_a, vec![uuid_b, uuid_c], Some(&ctx)).await?;
    assert_eq!(manual_ok.shard_id, 0);
    assert_eq!(manual_ok.primary, uuid_a);
    assert_eq!(manual_ok.replicas, vec![uuid_b, uuid_c]);

    // Valida que o evento reativo foi emitido no EventHub do nó local (uuid_a)
    let event = tokio::time::timeout(Duration::from_millis(500), shard_sub.recv())
        .await
        .expect("Should receive ShardEvent on EventHub")
        .expect("Event receive failed");
    match event {
        ShardEvent::Join { shard_id, members, role } => {
            assert_eq!(shard_id, 0);
            assert_eq!(role, aaron_shard::MemberRole::Leader);
            assert_eq!(members, vec![uuid_a, uuid_b, uuid_c]);
        }
        _ => panic!("Expected ShardEvent::Join"),
    }

    // 6. Validação dos Métodos de Consulta do Data-Plane (ShardHandle)
    assert!(shard_handle.is_my_primary(0).await);
    assert!(!shard_handle.is_my_replica(0).await);
    assert_eq!(shard_handle.my_role(0).await, Some(ShardRole::Primary));
    let my_shards = shard_handle.my_shards().await;
    assert!(!my_shards.is_empty());

    token.cancel();
    Ok(())
}
