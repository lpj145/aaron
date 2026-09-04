use crate::config::ShardConfig;
use crate::error::ShardError;
use crate::handle::ShardHandle;
use crate::types::{ShardId, ShardPlacement};
use aaron_control_plane::ControlPlaneHandle;
use aaron_core::{Context, Uuid};
use std::collections::BTreeSet;
use std::time::Duration;
use tracing::info;

/// Coordenador do Control Plane para o Estágio 1: Designação de Partições.
pub struct ShardCoordinator {
    config: ShardConfig,
    control_plane: ControlPlaneHandle,
    handle: ShardHandle,
    service_name: String,
}

impl ShardCoordinator {
    pub fn new(
        config: ShardConfig,
        control_plane: ControlPlaneHandle,
        handle: ShardHandle,
    ) -> Self {
        Self::with_service("default", config, control_plane, handle)
    }

    pub fn with_service(
        service_name: impl Into<String>,
        config: ShardConfig,
        control_plane: ControlPlaneHandle,
        handle: ShardHandle,
    ) -> Self {
        Self {
            config,
            control_plane,
            handle,
            service_name: service_name.into(),
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

    /// Sincroniza as partições existentes no Raft para a memória local do handle.
    pub async fn sync_from_raft(&self, _ctx: &Context) {
        let shard_data = self.control_plane.prefix_data("shards/").await;

        for (k, v_bytes) in shard_data {
            let Some(suffix) = k.strip_prefix("shards/") else {
                continue;
            };

            // Case 1: Global or per-service bootstrap flags
            if suffix == "system/bootstrapped" {
                self.handle.set_bootstrapped(true).await;
                continue;
            }
            if let Some(service) = suffix.strip_suffix("/system/bootstrapped") {
                self.handle.set_service_bootstrapped(service, true).await;
                continue;
            }

            // Case 2: Structured service partitions (e.g. "shards/inventory/0")
            let parts: Vec<&str> = suffix.split('/').collect();
            if parts.len() == 2 && parts[1].parse::<ShardId>().is_ok()
                && let Ok(mut placement) = ShardPlacement::from_bytes(&v_bytes) {
                    if placement.service_name == "default" || placement.service_name.is_empty() {
                        placement.service_name = parts[0].to_string();
                    }
                    self.handle.update_placement(placement).await;
                    continue;
                }

            // Case 3: Flat legacy partitions (e.g. "shards/0")
            if suffix.parse::<ShardId>().is_ok()
                && let Ok(placement) = ShardPlacement::from_bytes(&v_bytes) {
                    self.handle.update_placement(placement).await;
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

    /// Filtra nós elegíveis para o serviço a partir da tabela de membros SWIM,
    /// excluindo estritamente nós com a tag de Control Plane.
    pub fn filter_service_nodes(
        &self,
        service_name: &str,
        members: &[aaron_membership::Member],
    ) -> Vec<Uuid> {
        members
            .iter()
            .filter(|m| {
                m.status == aaron_membership::MemberStatus::Alive
                    && !m
                        .tags
                        .iter()
                        .any(|t| t == "role:control-plane" || t.starts_with("role:control-plane"))
                    && (service_name == "default"
                        || m.tags
                            .iter()
                            .any(|t| t == service_name || t == &format!("service:{service_name}")))
            })
            .map(|m| m.node_id.id())
            .collect()
    }

    // =========================================================================
    // ESTÁGIO 1: MODO 1 - ROUND-ROBIN (Bootstrap Inicial de Todas as Partições)
    // =========================================================================
    pub async fn bootstrap_round_robin(
        &self,
        nodes: &[Uuid],
        ctx: Option<&Context>,
    ) -> Result<usize, ShardError> {
        self.bootstrap_service_round_robin(&self.service_name, nodes, ctx).await
    }

    /// Executa o bootstrap Round-Robin isolado para um grupo de serviço específico.
    pub async fn bootstrap_service_round_robin(
        &self,
        service_name: &str,
        nodes: &[Uuid],
        _ctx: Option<&Context>,
    ) -> Result<usize, ShardError> {
        self.check_control_plane_health()?;

        let bootstrap_key = if service_name == "default" {
            "shards/system/bootstrapped".to_string()
        } else {
            format!("shards/{service_name}/system/bootstrapped")
        };

        // Validação: Bootstrap só pode ocorrer UMA ÚNICA VEZ por grupo de serviço
        if self.handle.is_service_bootstrapped(service_name).await
            || self.control_plane.get(&bootstrap_key).await.is_some()
        {
            return Err(ShardError::AlreadyBootstrapped);
        }

        // Validação: Mínimo de 3 nós selecionados para garantir quórum de replicação
        if nodes.len() < 3 {
            return Err(ShardError::InsufficientNodes { count: nodes.len() });
        }

        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let total_shards = self.config.total_shards;
        self.handle.set_total_shards(total_shards).await;
        let rf = self.config.replication_factor.min(nodes.len()).max(3);
        let mut assigned = 0;
        let mut batch_entries = Vec::with_capacity(total_shards as usize * 2 + 2);

        for shard_id in 0..total_shards {
            let primary_idx = (shard_id as usize) % nodes.len();
            let primary = nodes[primary_idx];

            let mut replicas = Vec::with_capacity(rf - 1);
            for i in 1..rf {
                let rep_idx = (primary_idx + i) % nodes.len();
                replicas.push(nodes[rep_idx]);
            }

            let placement = ShardPlacement::with_service(
                service_name,
                shard_id,
                primary,
                replicas,
                epoch,
            );

            let bytes = placement.to_bytes();
            if service_name == "default" {
                batch_entries.push((format!("shards/{shard_id:05}"), bytes.clone()));
            }
            batch_entries.push((format!("shards/{service_name}/{shard_id:05}"), bytes));
            self.handle.update_placement(placement).await;
            assigned += 1;
        }

        // Grava no consenso do Control Plane todas as atribuições e o flag de bootstrap
        batch_entries.push((bootstrap_key, b"true".to_vec()));
        if service_name == "default" {
            batch_entries.push(("shards/system/bootstrapped".to_string(), b"true".to_vec()));
        }

        self.control_plane
            .set_batch(batch_entries)
            .await
            .map_err(|e| ShardError::Raft {
                message: format!("{e}"),
            })?;

        self.handle.set_service_bootstrapped(service_name, true).await;
        if service_name == "default" {
            self.handle.set_bootstrapped(true).await;
        }

        // Dispatches partition commands via Control Plane channel to Primary and Replica nodes
        for shard_id in 0..total_shards {
            if let Some(p) = self.handle.get_service_placement(service_name, shard_id).await {
                let _ = self
                    .control_plane
                    .dispatch_shard_command(p.primary, shard_id, 0, p.primary, &p.replicas, epoch)
                    .await;
                for rep in &p.replicas {
                    let _ = self
                        .control_plane
                        .dispatch_shard_command(*rep, shard_id, 1, p.primary, &p.replicas, epoch)
                        .await;
                }
            }
        }

        info!(
            target: "shard_coordinator",
            service_name,
            total_assigned = assigned,
            total_nodes = nodes.len(),
            epoch,
            "Round-Robin Shard Bootstrap completed successfully via Raft and dispatched to nodes"
        );

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
        self.assign_service_manual(&self.service_name, shard_id, primary, replicas, ctx).await
    }

    /// Executa a designação manual para um grupo de serviço específico.
    pub async fn assign_service_manual(
        &self,
        service_name: &str,
        shard_id: ShardId,
        primary: Uuid,
        replicas: Vec<Uuid>,
        _ctx: Option<&Context>,
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

        let placement = ShardPlacement::with_service(
            service_name,
            shard_id,
            primary,
            replicas.clone(),
            epoch,
        );

        let bytes = placement.to_bytes();
        let key = if service_name == "default" {
            format!("shards/{shard_id:05}")
        } else {
            format!("shards/{service_name}/{shard_id:05}")
        };

        self.control_plane
            .set(key, bytes)
            .await
            .map_err(|e| ShardError::Raft {
                message: format!("{e}"),
            })?;

        self.handle.update_placement(placement.clone()).await;

        // Dispara comando via canal do Control Plane para o Primary
        let _ = self
            .control_plane
            .dispatch_shard_command(primary, shard_id, 0, primary, &replicas, epoch)
            .await;

        // Dispara comando via canal do Control Plane para cada Réplica
        for rep in &replicas {
            let _ = self
                .control_plane
                .dispatch_shard_command(*rep, shard_id, 1, primary, &replicas, epoch)
                .await;
        }

        info!(
            target: "shard_coordinator",
            service_name,
            shard_id,
            %primary,
            replicas_count = replicas.len(),
            epoch,
            "Manual Shard Assignment committed to Raft and dispatched to nodes"
        );

        Ok(placement)
    }
}
