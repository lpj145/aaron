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
pub enum ShardRole {
    Primary,
    Replica,
}

/// Unified domain event for Shard partitions lifecycle and assignments.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ShardEvent {
    /// Uma partição foi designada (Primary ou Replica) para o nó.
    Assigned {
        shard_id: u32,
        role: ShardRole,
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
