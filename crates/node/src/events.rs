use crate::Uuid;

/// Unified domain event enum for Node lifecycle, configuration updates, and service supervision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeEvent {
    /// Request to supervise and start an instance of a registered service.
    StartService {
        name: String,
    },
    /// Command to associate or update this node's cluster identity.
    BindClusterId {
        cluster_id: Uuid,
    },
    /// Event indicating that a cluster node should be started/orchestrated.
    StartNode {
        service_name: String,
        node_id: Uuid,
        addr: Option<String>,
    },
    /// Event indicating that a cluster node should be removed/stopped.
    RemoveNode {
        node_id: Uuid,
    },
    /// Dynamic runtime update of an environment variable.
    SetEnvVar {
        key: String,
        value: String,
    },
}

/// Backwards compatibility alias for `NodeEvent`.
pub type NodeEvents = NodeEvent;

/// Struct representation kept for standalone event bus publishing if desired.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetEnvVar {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MemberRole {
    Learner,
    Voter,
    Leader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ShardRole {
    Primary,
    Replica,
    Learner,
    Voter,
    Leader,
}

impl From<MemberRole> for ShardRole {
    fn from(r: MemberRole) -> Self {
        match r {
            MemberRole::Learner => ShardRole::Learner,
            MemberRole::Voter => ShardRole::Voter,
            MemberRole::Leader => ShardRole::Leader,
        }
    }
}

impl From<ShardRole> for MemberRole {
    fn from(r: ShardRole) -> Self {
        match r {
            ShardRole::Primary | ShardRole::Leader => MemberRole::Leader,
            ShardRole::Replica | ShardRole::Voter => MemberRole::Voter,
            ShardRole::Learner => MemberRole::Learner,
        }
    }
}

/// Representa a atribuição de um shard ao nó com seu quórum e papel inicial.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShardGroup {
    pub shard_id: u32,
    pub members: Vec<Uuid>,
    pub role: MemberRole,
}

/// Unified domain event for Shard partitions lifecycle and assignments.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ShardEvent {
    /// Inicialização atômica do Worker com todas as suas partições de uma só vez.
    Bootstrap {
        shards: Vec<ShardGroup>,
    },
    /// Adição dinâmica de uma partição em tempo de execução (rebalanceamento / escala).
    Join {
        shard_id: u32,
        members: Vec<Uuid>,
        role: MemberRole,
    },
    /// Mudança de papel no quórum (Learner, Voter ou Leader).
    RoleChanged {
        shard_id: u32,
        role: MemberRole,
    },
    /// Remoção / desligamento da réplica do shard (saída do quórum).
    Leave {
        shard_id: u32,
    },
}
