use node::Uuid;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub type ShardId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardStatus {
    Healthy,
    Degraded,
    Unassigned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardRole {
    Primary,
    Replica,
}

/// Registro de designação de uma partição (Estágio 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardPlacement {
    pub shard_id: ShardId,
    pub primary: Uuid,
    pub replicas: Vec<Uuid>,
    pub status: ShardStatus,
    pub epoch: u64,
}

impl ShardPlacement {
    pub fn new(shard_id: ShardId, primary: Uuid, replicas: Vec<Uuid>, epoch: u64) -> Self {
        Self {
            shard_id,
            primary,
            replicas,
            status: ShardStatus::Healthy,
            epoch,
        }
    }

    /// Retorna o conjunto de todos os nós participantes (Primary + Réplicas).
    pub fn all_nodes(&self) -> BTreeSet<Uuid> {
        let mut set = BTreeSet::new();
        set.insert(self.primary);
        for r in &self.replicas {
            set.insert(*r);
        }
        set
    }

    /// Retorna a contagem total de nós atribuídos a esta partição.
    pub fn node_count(&self) -> usize {
        self.all_nodes().len()
    }
}

/// Eventos reativos emitidos para o barramento `EventHub`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardEvent {
    /// Uma partição foi designada (Round-Robin ou Manual).
    Assigned {
        shard_id: ShardId,
        primary: Uuid,
        replicas: Vec<Uuid>,
        epoch: u64,
    },
    /// O bootstrap inicial de todas as partições foi concluído.
    BootstrapCompleted {
        total_shards: u32,
        assigned_count: usize,
        epoch: u64,
    },
}
