use node::Uuid;
pub use node::{ShardEvent, ShardRole};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub type ShardId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardStatus {
    Healthy,
    Degraded,
    Unassigned,
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

/// Comandos disparados pelo Control Plane para nós de dados (Data-Plane).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardCommand {
    /// Atribuição de partição enviada via canal de controle de frame.
    Assign {
        shard_id: ShardId,
        role: ShardRole,
        primary: Uuid,
        replicas: Vec<Uuid>,
        epoch: u64,
    },
}
