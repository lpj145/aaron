use crate::config::ShardConfig;
use crate::coordinator::ShardCoordinator;
use crate::handle::ShardHandle;
use control_plane_service::ControlPlaneHandle;
use node::{BoxError, Context, Service, ServiceConfig};
use tracing::info;

/// Serviço responsável pelo ciclo de vida e roteamento de Shards.
///
/// Possui dois modos de operação:
/// - `Coordinator`: Executado no Control Plane (com `ControlPlaneHandle`), orquestra e persiste atribuições no Raft.
/// - `Worker`: Executado no Data Plane (workers como `inventory`), escuta comandos via QUIC e mantém estado local.
pub enum ShardService {
    Coordinator {
        config_override: Option<ShardConfig>,
        control_plane: ControlPlaneHandle,
        handle: ShardHandle,
    },
    Worker {
        config_override: Option<ShardConfig>,
        handle: ShardHandle,
    },
}

impl ShardService {
    /// Cria o ShardService em modo Coordenador (para nós do Control Plane).
    pub fn coordinator(control_plane: ControlPlaneHandle) -> (Self, ShardHandle) {
        let nil_uuid = node::Uuid::NIL;
        let handle = ShardHandle::new(nil_uuid, 1024);
        (
            Self::Coordinator {
                config_override: None,
                control_plane,
                handle: handle.clone(),
            },
            handle,
        )
    }

    /// Cria o ShardService em modo Worker (para nós do Data Plane).
    pub fn worker() -> (Self, ShardHandle) {
        let nil_uuid = node::Uuid::NIL;
        let handle = ShardHandle::new(nil_uuid, 1024);
        (
            Self::Worker {
                config_override: None,
                handle: handle.clone(),
            },
            handle,
        )
    }

    pub fn with_config(mut self, config: ShardConfig) -> Self {
        match &mut self {
            Self::Coordinator { config_override, .. } => *config_override = Some(config),
            Self::Worker { config_override, .. } => *config_override = Some(config),
        }
        self
    }
}

impl Service for ShardService {
    type Config = ShardConfig;

    fn name(&self) -> &str {
        match self {
            Self::Coordinator { .. } => "shard-coordinator",
            Self::Worker { .. } => "shard-worker",
        }
    }

    fn capabilities(&self) -> Vec<&str> {
        match self {
            Self::Coordinator { .. } => vec!["shard-coordinator"],
            Self::Worker { .. } => vec!["shard", "shard-worker"],
        }
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        let my_id = ctx.identity.id();

        match self {
            Self::Coordinator {
                config_override,
                control_plane,
                handle,
            } => {
                let config = match config_override {
                    Some(c) => c.clone(),
                    None => ShardConfig::from_env(&ctx.env)?,
                };

                handle.set_local_node_id(my_id).await;
                handle.set_total_shards(config.total_shards).await;

                // Sync placement cache from local EventHub
                spawn_event_hub_sync(ctx.token.clone(), ctx.event_hub.clone(), handle.clone());

                // Periodic telemetry self-recording: every 3 seconds
                let telemetry_coord = ctx.telemetry.clone();
                let cp_handle_for_telemetry = control_plane.clone();
                let token_coord = ctx.token.clone();
                let handle_coord = handle.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3));
                    loop {
                        tokio::select! {
                            _ = token_coord.cancelled() => break,
                            _ = interval.tick() => {
                                let local_shard_count = handle_coord.local_shards_count().await as u32;
                                let idle_base = (telemetry_coord.nominal_wps() / 12).max(40);
                                let dynamic_wps = idle_base + local_shard_count * 8;
                                telemetry_coord.set_wps(dynamic_wps);

                                let current_wps = telemetry_coord.current_wps();
                                let error_rate = telemetry_coord.error_rate();
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                cp_handle_for_telemetry
                                    .record_node_telemetry(my_id, current_wps, error_rate, now)
                                    .await;
                            }
                        }
                    }
                });

                let coord = ShardCoordinator::new(config, control_plane.clone(), handle.clone());
                coord.run_loop(ctx).await;
                Ok(())
            }

            Self::Worker {
                config_override,
                handle,
            } => {
                let config = match config_override {
                    Some(c) => c.clone(),
                    None => ShardConfig::from_env(&ctx.env)?,
                };

                handle.set_local_node_id(my_id).await;
                handle.set_total_shards(config.total_shards).await;

                info!(
                    target: "shard_worker",
                    %my_id,
                    total_shards = config.total_shards,
                    "ShardWorker active on Data Plane - listening for incoming Control Plane commands"
                );

                // 1. Sync placement cache from local EventHub
                spawn_event_hub_sync(ctx.token.clone(), ctx.event_hub.clone(), handle.clone());

                // 2. Bind QUIC listener on 0.0.0.0:18946 for incoming ShardCommands from Control Plane
                let bind_addr = ctx
                    .env
                    .get::<String>("SHARD_BIND_ADDR")
                    .or_else(|| ctx.env.get::<String>("CONTROL_PLANE_BIND_ADDR"))
                    .unwrap_or_else(|| "0.0.0.0:18946".to_string());

                let endpoint = ctx.network.quic.listen_for_node(&bind_addr, my_id).await?;
                info!(
                    target: "shard_worker",
                    %bind_addr,
                    "ShardWorker QUIC listener established"
                );

                let event_hub = ctx.event_hub.clone();
                let token = ctx.token.clone();

                tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = token.cancelled() => break,
                            incoming = endpoint.accept() => {
                                let Some(incoming_conn) = incoming else { break };
                                let event_hub_inner = event_hub.clone();
                                let token_inner = token.clone();
                                tokio::spawn(async move {
                                    if let Ok(conn) = incoming_conn.await {
                                        loop {
                                            tokio::select! {
                                                _ = token_inner.cancelled() => break,
                                                stream_res = conn.accept_bi() => {
                                                    let Ok((mut send, mut recv)) = stream_res else { break };
                                                    let hub = event_hub_inner.clone();
                                                    tokio::spawn(async move {
                                                        while let Ok(Some(frame)) = node::read_frame(&mut recv).await {
                                                            if let Ok(control_plane_service::RaftMessage::ShardCommand {
                                                                shard_id,
                                                                role: _,
                                                                primary_high,
                                                                primary_low,
                                                                replicas,
                                                                epoch,
                                                                op_type,
                                                                target_role,
                                                            }) = control_plane_service::RaftMessage::from_bytes(&frame) {
                                                                let primary = node::Uuid::new(primary_high, primary_low);
                                                                let replica_uuids: Vec<node::Uuid> = replicas
                                                                    .into_iter()
                                                                    .map(|(h, l)| node::Uuid::new(h, l))
                                                                    .collect();

                                                                let member_role = match target_role {
                                                                    0 => node::MemberRole::Learner,
                                                                    1 => node::MemberRole::Voter,
                                                                    _ => node::MemberRole::Leader,
                                                                };

                                                                match op_type {
                                                                    1 => {
                                                                        hub.publish(node::ShardEvent::RoleChanged {
                                                                            shard_id,
                                                                            role: member_role,
                                                                        }).await;
                                                                    }
                                                                    2 => {
                                                                        hub.publish(node::ShardEvent::Leave {
                                                                            shard_id,
                                                                        }).await;
                                                                    }
                                                                    _ => {
                                                                        let mut all_members = vec![primary];
                                                                        all_members.extend(&replica_uuids);
                                                                        hub.publish(node::ShardEvent::Join {
                                                                            shard_id,
                                                                            members: all_members,
                                                                            role: member_role,
                                                                        }).await;
                                                                    }
                                                                }

                                                                let resp = control_plane_service::RaftMessage::ShardCommandResp {
                                                                    success: true,
                                                                    shard_id,
                                                                    current_role: target_role,
                                                                    term: epoch,
                                                                    reject_reason: 0,
                                                                };
                                                                let _ = node::write_frame(&mut send, &resp.to_bytes()).await;
                                                            }
                                                        }
                                                    });
                                                }
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    }
                });

                // 3. Periodic telemetry heartbeat ticker: sends telemetry every 3 seconds via QUIC to Control Plane
                let telemetry_worker = ctx.telemetry.clone();
                let my_id_worker = my_id;
                let quic_worker = ctx.network.quic.clone();
                let token_worker = ctx.token.clone();
                let handle_worker = handle.clone();
                let cp_addr = ctx
                    .env
                    .get::<String>("CONTROL_PLANE_ADDR")
                    .unwrap_or_else(|| "127.0.0.1:18946".to_string());

                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3));
                    interval.tick().await;

                    loop {
                        tokio::select! {
                            _ = token_worker.cancelled() => break,
                            _ = interval.tick() => {
                                let local_shard_count = handle_worker.local_shards_count().await as u32;
                                let idle_base = (telemetry_worker.nominal_wps() / 10).max(50);
                                let dynamic_wps = idle_base + local_shard_count * 8;
                                telemetry_worker.set_wps(dynamic_wps);

                                let current_wps = telemetry_worker.current_wps();
                                let error_rate = telemetry_worker.error_rate();
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();

                                let msg = control_plane_service::RaftMessage::TelemetryHeartbeat {
                                    node_id_high: my_id_worker.high,
                                    node_id_low: my_id_worker.low,
                                    current_wps,
                                    error_rate,
                                    timestamp: now,
                                };
                                let bytes = msg.to_bytes();

                                if let Ok(conn) = quic_worker.connect_node(&cp_addr, my_id_worker).await {
                                    if let Ok((mut send, _recv)) = conn.open_bi().await {
                                        let _ = node::write_frame(&mut send, &bytes).await;
                                        let _ = send.finish();
                                    }
                                }
                            }
                        }
                    }
                });

                // Wait for service cancellation
                ctx.token.cancelled().await;
                Ok(())
            }
        }
    }
}

/// Sincroniza em segundo plano os eventos do EventHub para o ShardHandle em memória.
fn spawn_event_hub_sync(
    token: tokio_util::sync::CancellationToken,
    event_hub: node::EventHub,
    handle: ShardHandle,
) {
    tokio::spawn(async move {
        let mut shard_events = event_hub.subscribe::<node::ShardEvent>().await;
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                event_res = shard_events.recv() => {
                    match event_res {
                        Ok(node::ShardEvent::Bootstrap { shards }) => {
                            let my_id = handle.local_node_id().await;
                            for g in shards {
                                let (primary, replicas) = if g.members.is_empty() {
                                    (node::Uuid::NIL, Vec::new())
                                } else if g.role == node::MemberRole::Leader {
                                    (my_id, g.members.iter().filter(|m| **m != my_id).copied().collect())
                                } else {
                                    (g.members[0], g.members[1..].to_vec())
                                };
                                let placement = crate::types::ShardPlacement::new(g.shard_id, primary, replicas, 0);
                                handle.update_placement(placement).await;
                            }
                        }
                        Ok(node::ShardEvent::Join { shard_id, members, role }) => {
                            let my_id = handle.local_node_id().await;
                            let (primary, replicas) = if members.is_empty() {
                                (node::Uuid::NIL, Vec::new())
                            } else if role == node::MemberRole::Leader {
                                (my_id, members.iter().filter(|m| **m != my_id).copied().collect())
                            } else {
                                (members[0], members[1..].to_vec())
                            };
                            let placement = crate::types::ShardPlacement::new(shard_id, primary, replicas, 0);
                            handle.update_placement(placement).await;
                        }
                        Ok(node::ShardEvent::RoleChanged { shard_id, role }) => {
                            handle.announce_role(shard_id, role).await;
                        }
                        Ok(node::ShardEvent::Leave { .. }) => {}
                        Err(_) => break,
                    }
                }
            }
        }
    });
}
