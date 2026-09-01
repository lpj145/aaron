use crate::config::ShardConfig;
use crate::error::ShardError;
use crate::handle::ShardHandle;
use crate::types::{ShardEvent, ShardId, ShardPlacement, ShardStatus};
use control_plane_service::ControlPlaneHandle;
use node::{Context, Uuid};
use std::collections::BTreeSet;
use std::time::Duration;
use tracing::info;

/// Coordenador do Control Plane para o Estágio 1: Designação de Partições.
pub struct ShardCoordinator {
    config: ShardConfig,
    control_plane: ControlPlaneHandle,
    handle: ShardHandle,
}

impl ShardCoordinator {
    pub fn new(
        config: ShardConfig,
        control_plane: ControlPlaneHandle,
        handle: ShardHandle,
    ) -> Self {
        Self {
            config,
            control_plane,
            handle,
        }
    }

    pub fn handle(&self) -> ShardHandle {
        self.handle.clone()
    }

    /// Loop passivo de sincronização das designações persistidas no Raft.
    pub async fn run_loop(&self, ctx: Context) {
        let my_id = ctx.identity.id();
        info!(
            target: "shard_coordinator",
            %my_id,
            total_shards = self.config.total_shards,
            replication_factor = self.config.replication_factor,
            "ShardCoordinator active on Control Plane (Stage 1: Assignment)"
        );

        self.sync_from_raft(&ctx).await;

        let mut sync_interval = tokio::time::interval(Duration::from_millis(1000));
        loop {
            tokio::select! {
                _ = ctx.token.cancelled() => {
                    info!(target: "shard_coordinator", "ShardCoordinator shutting down");
                    break;
                }
                _ = sync_interval.tick() => {
                    self.sync_from_raft(&ctx).await;
                }
            }
        }
    }

    /// Sincroniza as partições existentes no Raft para a memória local.
    pub async fn sync_from_raft(&self, _ctx: &Context) {
        let all_data = self.control_plane.all_data().await;
        for (k, v) in all_data {
            if let Some(shard_id_str) = k.strip_prefix("shards/") {
                if let Ok(_shard_id) = shard_id_str.parse::<ShardId>() {
                    if let Ok(placement) = serde_json::from_str::<ShardPlacement>(&v) {
                        self.handle.update_placement(placement).await;
                    }
                }
            }
        }
    }

    /// Valida se o Control Plane possui quórum Raft ativo para aceitar operações de designação.
    pub fn check_control_plane_health(&self) -> Result<(), ShardError> {
        // Para alterar a topologia de shards, o nó atual deve estar conectado ao quórum e ser o Líder (ou apto a escrever via Raft)
        if !self.control_plane.is_leader() && self.control_plane.current_leader().is_none() {
            return Err(ShardError::ControlPlaneUnavailable);
        }
        Ok(())
    }

    // =========================================================================
    // ESTÁGIO 1: MODO 1 - ROUND-ROBIN (Bootstrap Inicial de Todas as Partições)
    // =========================================================================
    pub async fn bootstrap_round_robin(
        &self,
        nodes: &[Uuid],
        ctx: Option<&Context>,
    ) -> Result<usize, ShardError> {
        self.check_control_plane_health()?;

        // Validação: Mínimo de 3 nós selecionados para garantir quórum de replicação
        if nodes.len() < 3 {
            return Err(ShardError::InsufficientNodes { count: nodes.len() });
        }

        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let total_shards = self.config.total_shards;
        let rf = self.config.replication_factor.min(nodes.len()).max(3);
        let mut assigned = 0;

        for shard_id in 0..total_shards {
            let primary_idx = (shard_id as usize) % nodes.len();
            let primary = nodes[primary_idx];

            let mut replicas = Vec::with_capacity(rf - 1);
            for i in 1..rf {
                let rep_idx = (primary_idx + i) % nodes.len();
                replicas.push(nodes[rep_idx]);
            }

            let placement = ShardPlacement {
                shard_id,
                primary,
                replicas,
                status: ShardStatus::Healthy,
                epoch,
            };

            let key = format!("shards/{shard_id}");
            let val = serde_json::to_string(&placement)
                .map_err(|source| ShardError::Serialization { source })?;

            self.control_plane
                .set(key, val)
                .await
                .map_err(|e| ShardError::Raft {
                    message: format!("{e}"),
                })?;

            self.handle.update_placement(placement).await;
            assigned += 1;
        }

        info!(
            target: "shard_coordinator",
            total_assigned = assigned,
            total_nodes = nodes.len(),
            epoch,
            "Round-Robin Shard Bootstrap completed successfully via Raft"
        );

        if let Some(c) = ctx {
            let _ = c.event_hub.publish(ShardEvent::BootstrapCompleted {
                total_shards,
                assigned_count: assigned,
                epoch,
            }).await;
        }

        Ok(assigned)
    }

    // =========================================================================
    // ESTÁGIO 1: MODO 2 - DESIGNAÇÃO MANUAL (Seleção com no mínimo 3 nós)
    // =========================================================================
    pub async fn assign_manual(
        &self,
        shard_id: ShardId,
        primary: Uuid,
        replicas: Vec<Uuid>,
        ctx: Option<&Context>,
    ) -> Result<ShardPlacement, ShardError> {
        self.check_control_plane_health()?;

        if shard_id >= self.config.total_shards {
            return Err(ShardError::InvalidShardId {
                shard_id,
                total_shards: self.config.total_shards,
            });
        }

        // Validação 1: O nó Primary não pode ser listado também como réplica
        if replicas.contains(&primary) {
            return Err(ShardError::DuplicateNodeAssignment {
                shard_id,
                node: primary.to_string(),
            });
        }

        // Validação 2: Mínimo de 3 nós distintos selecionados (1 Primary + >= 2 Réplicas)
        let mut distinct_nodes = BTreeSet::new();
        distinct_nodes.insert(primary);
        for r in &replicas {
            distinct_nodes.insert(*r);
        }

        if distinct_nodes.len() < 3 {
            return Err(ShardError::InsufficientNodes {
                count: distinct_nodes.len(),
            });
        }

        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let placement = ShardPlacement {
            shard_id,
            primary,
            replicas: replicas.clone(),
            status: ShardStatus::Healthy,
            epoch,
        };

        let key = format!("shards/{shard_id}");
        let val = serde_json::to_string(&placement)
            .map_err(|source| ShardError::Serialization { source })?;

        self.control_plane
            .set(key, val)
            .await
            .map_err(|e| ShardError::Raft {
                message: format!("{e}"),
            })?;

        self.handle.update_placement(placement.clone()).await;

        info!(
            target: "shard_coordinator",
            shard_id,
            %primary,
            replicas_count = replicas.len(),
            epoch,
            "Manual Shard Assignment committed to Raft"
        );

        if let Some(c) = ctx {
            let _ = c.event_hub.publish(ShardEvent::Assigned {
                shard_id,
                primary,
                replicas,
                epoch,
            }).await;
        }

        Ok(placement)
    }
}
