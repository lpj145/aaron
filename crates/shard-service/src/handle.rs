use crate::types::{ShardId, ShardPlacement};
use node::Uuid;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

struct Inner {
    local_node_id: Uuid,
    total_shards: u32,
    bootstrapped: bool,
    placements: BTreeMap<ShardId, ShardPlacement>,
}

/// Handle thread-safe para consultas do estado das partições.
#[derive(Clone)]
pub struct ShardHandle {
    inner: Arc<RwLock<Inner>>,
}

impl ShardHandle {
    pub fn new(local_node_id: Uuid, total_shards: u32) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                local_node_id,
                total_shards,
                bootstrapped: false,
                placements: BTreeMap::new(),
            })),
        }
    }

    pub async fn local_node_id(&self) -> Uuid {
        self.inner.read().await.local_node_id
    }

    pub async fn set_local_node_id(&self, id: Uuid) {
        self.inner.write().await.local_node_id = id;
    }

    pub async fn total_shards(&self) -> u32 {
        self.inner.read().await.total_shards
    }

    pub async fn set_total_shards(&self, total: u32) {
        self.inner.write().await.total_shards = total;
    }

    pub async fn is_bootstrapped(&self) -> bool {
        let inner = self.inner.read().await;
        inner.bootstrapped || !inner.placements.is_empty()
    }

    pub async fn set_bootstrapped(&self, val: bool) {
        self.inner.write().await.bootstrapped = val;
    }

    pub async fn get_placement(&self, shard_id: ShardId) -> Option<ShardPlacement> {
        self.inner.read().await.placements.get(&shard_id).cloned()
    }

    pub async fn all_placements(&self) -> Vec<ShardPlacement> {
        self.inner.read().await.placements.values().cloned().collect()
    }

    pub async fn assigned_count(&self) -> usize {
        self.inner.read().await.placements.len()
    }

    pub async fn update_placement(&self, placement: ShardPlacement) {
        self.inner.write().await.placements.insert(placement.shard_id, placement);
    }

    /// Retorna todas as partições em que o nó local participa (como Primary ou Réplica).
    pub async fn my_shards(&self) -> Vec<(ShardId, crate::types::ShardRole, ShardPlacement)> {
        let inner = self.inner.read().await;
        let local_id = inner.local_node_id;
        let mut result = Vec::new();
        for (id, p) in &inner.placements {
            if p.primary == local_id {
                result.push((*id, crate::types::ShardRole::Primary, p.clone()));
            } else if p.replicas.contains(&local_id) {
                result.push((*id, crate::types::ShardRole::Replica, p.clone()));
            }
        }
        result
    }

    /// Retorna o papel do nó local na partição informada (`Primary`, `Replica` ou `None`).
    pub async fn my_role(&self, shard_id: ShardId) -> Option<crate::types::ShardRole> {
        let inner = self.inner.read().await;
        let local_id = inner.local_node_id;
        if let Some(p) = inner.placements.get(&shard_id) {
            if p.primary == local_id {
                Some(crate::types::ShardRole::Primary)
            } else if p.replicas.contains(&local_id) {
                Some(crate::types::ShardRole::Replica)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub async fn is_my_primary(&self, shard_id: ShardId) -> bool {
        matches!(self.my_role(shard_id).await, Some(crate::types::ShardRole::Primary))
    }

    pub async fn is_my_replica(&self, shard_id: ShardId) -> bool {
        matches!(self.my_role(shard_id).await, Some(crate::types::ShardRole::Replica))
    }
}
