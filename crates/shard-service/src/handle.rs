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
}
