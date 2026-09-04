use crate::types::{ShardId, ShardPlacement};
use node::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tokio::sync::RwLock;

struct Inner {
    local_node_id: Uuid,
    total_shards: u32,
    bootstrapped: bool,
    placements: BTreeMap<(String, ShardId), ShardPlacement>,
    bootstrapped_services: BTreeSet<String>,
}

/// Handle thread-safe para consultas do estado das partições e comunicação do worker.
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
                bootstrapped_services: BTreeSet::new(),
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

    pub async fn is_service_bootstrapped(&self, service_name: &str) -> bool {
        let inner = self.inner.read().await;
        inner.bootstrapped_services.contains(service_name)
            || inner.placements.keys().any(|(s, _)| s == service_name)
    }

    pub async fn set_bootstrapped(&self, val: bool) {
        self.inner.write().await.bootstrapped = val;
    }

    pub async fn set_service_bootstrapped(&self, service_name: &str, val: bool) {
        let mut inner = self.inner.write().await;
        if val {
            inner.bootstrapped_services.insert(service_name.to_string());
        } else {
            inner.bootstrapped_services.remove(service_name);
        }
    }

    pub async fn get_placement(&self, shard_id: ShardId) -> Option<ShardPlacement> {
        let inner = self.inner.read().await;
        if let Some(p) = inner.placements.get(&("default".to_string(), shard_id)) {
            return Some(p.clone());
        }
        inner
            .placements
            .iter()
            .find(|((_, id), _)| *id == shard_id)
            .map(|(_, p)| p.clone())
    }

    pub async fn get_service_placement(
        &self,
        service_name: &str,
        shard_id: ShardId,
    ) -> Option<ShardPlacement> {
        self.inner
            .read()
            .await
            .placements
            .get(&(service_name.to_string(), shard_id))
            .cloned()
    }

    pub async fn all_placements(&self) -> Vec<ShardPlacement> {
        self.inner.read().await.placements.values().cloned().collect()
    }

    pub async fn all_service_placements(&self, service_name: &str) -> Vec<ShardPlacement> {
        self.inner
            .read()
            .await
            .placements
            .iter()
            .filter(|((s, _), _)| s == service_name)
            .map(|(_, p)| p.clone())
            .collect()
    }

    pub async fn assigned_count(&self) -> usize {
        self.inner.read().await.placements.len()
    }

    pub async fn assigned_service_count(&self, service_name: &str) -> usize {
        self.inner
            .read()
            .await
            .placements
            .keys()
            .filter(|(s, _)| s == service_name)
            .count()
    }

    pub async fn local_shards_count(&self) -> usize {
        let inner = self.inner.read().await;
        let my_id = inner.local_node_id;
        inner
            .placements
            .values()
            .filter(|p| p.primary == my_id || p.replicas.contains(&my_id))
            .count()
    }

    pub async fn update_placement(&self, placement: ShardPlacement) {
        self.inner.write().await.placements.insert(
            (placement.service_name.clone(), placement.shard_id),
            placement,
        );
    }

    /// Sets or updates the primary leader for a partition.
    pub async fn set_service_leader(&self, service_name: &str, shard_id: ShardId, leader_id: Uuid) {
        let mut inner = self.inner.write().await;
        let target_key = if inner.placements.contains_key(&(service_name.to_string(), shard_id)) {
            Some((service_name.to_string(), shard_id))
        } else {
            inner
                .placements
                .keys()
                .find(|(_, id)| *id == shard_id)
                .cloned()
        };

        if let Some(k) = target_key
            && let Some(placement) = inner.placements.get_mut(&k) {
                let old_primary = placement.primary;
                if old_primary != leader_id {
                    placement.primary = leader_id;
                    placement.replicas.retain(|r| *r != leader_id);
                    if !placement.replicas.contains(&old_primary) && old_primary != Uuid::NIL {
                        placement.replicas.push(old_primary);
                    }
                }
            }
    }

    pub async fn set_leader(&self, shard_id: ShardId, leader_id: Uuid) {
        self.set_service_leader("", shard_id, leader_id).await;
    }

    /// Alias para retrocompatibilidade
    pub async fn announce_leader(
        &self,
        service_name: &str,
        shard_id: ShardId,
        leader_id: Uuid,
        _term: u64,
    ) {
        self.set_service_leader(service_name, shard_id, leader_id).await;
    }

    /// Anuncia o papel do nó local no quórum do shard (Leader, Voter ou Learner).
    pub async fn announce_role(
        &self,
        shard_id: ShardId,
        role: node::MemberRole,
    ) {
        let mut inner = self.inner.write().await;
        let local_id = inner.local_node_id;

        let target_key = inner
            .placements
            .keys()
            .find(|(_, id)| *id == shard_id)
            .cloned();

        if let Some(k) = target_key
            && let Some(placement) = inner.placements.get_mut(&k) {
                match role {
                    node::MemberRole::Leader => {
                        let old_primary = placement.primary;
                        if old_primary != local_id {
                            placement.primary = local_id;
                            placement.replicas.retain(|r| *r != local_id);
                            if !placement.replicas.contains(&old_primary) && old_primary != Uuid::NIL {
                                placement.replicas.push(old_primary);
                            }
                        }
                    }
                    node::MemberRole::Voter => {
                        if placement.primary == local_id {
                            placement.primary = Uuid::NIL;
                        }
                        if !placement.replicas.contains(&local_id) {
                            placement.replicas.push(local_id);
                        }
                    }
                    node::MemberRole::Learner => {
                        if placement.primary == local_id {
                            placement.primary = Uuid::NIL;
                        }
                        placement.replicas.retain(|r| *r != local_id);
                    }
                }
            }
    }

    /// Retorna todas as partições em que o nó local participa (como Primary ou Réplica).
    pub async fn my_shards(&self) -> Vec<(ShardId, crate::types::ShardRole, ShardPlacement)> {
        let inner = self.inner.read().await;
        let local_id = inner.local_node_id;
        let mut result = Vec::new();
        for ((_, id), p) in &inner.placements {
            if p.primary == local_id {
                result.push((*id, crate::types::ShardRole::Primary, p.clone()));
            } else if p.replicas.contains(&local_id) {
                result.push((*id, crate::types::ShardRole::Replica, p.clone()));
            }
        }
        result
    }

    /// Returns the local node's role in the specified partition (`Primary`, `Replica`, or `None`).
    pub async fn my_role(&self, shard_id: ShardId) -> Option<crate::types::ShardRole> {
        let inner = self.inner.read().await;
        let local_id = inner.local_node_id;
        let maybe_p = inner
            .placements
            .iter()
            .find(|((_, id), _)| *id == shard_id)
            .map(|(_, p)| p);
        if let Some(p) = maybe_p {
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

    pub async fn my_service_shards(
        &self,
        service_name: &str,
    ) -> Vec<(ShardId, crate::types::ShardRole, ShardPlacement)> {
        self.my_shards()
            .await
            .into_iter()
            .filter(|(_, _, p)| p.service_name == service_name)
            .collect()
    }

    pub async fn my_service_role(
        &self,
        service_name: &str,
        shard_id: ShardId,
    ) -> Option<crate::types::ShardRole> {
        let inner = self.inner.read().await;
        let local_id = inner.local_node_id;
        if let Some(p) = inner.placements.get(&(service_name.to_string(), shard_id)) {
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

    pub async fn is_my_service_primary(&self, service_name: &str, shard_id: ShardId) -> bool {
        matches!(self.my_service_role(service_name, shard_id).await, Some(crate::types::ShardRole::Primary))
    }

    pub async fn is_my_service_replica(&self, service_name: &str, shard_id: ShardId) -> bool {
        matches!(self.my_service_role(service_name, shard_id).await, Some(crate::types::ShardRole::Replica))
    }

    /// Returns a deterministic [`Router`](crate::route::Router) configured with the total number of shards.
    pub async fn router(&self) -> crate::route::Router {
        crate::route::Router::new(self.total_shards().await.max(1))
    }

    /// Deterministically computes the target [`ShardId`] for the given raw binary key.
    pub async fn route_key(&self, key: &[u8]) -> ShardId {
        self.router().await.route(key)
    }

    /// Deterministically computes the target [`ShardId`] for the given UTF-8 key.
    pub async fn route_key_str(&self, key: &str) -> ShardId {
        self.router().await.route_str(key)
    }

    /// Deterministically resolves the target [`ShardPlacement`] (Primary and Replicas)
    /// for a given service and key.
    pub async fn lookup_route(&self, service_name: &str, key: &[u8]) -> Option<ShardPlacement> {
        let shard_id = self.route_key(key).await;
        self.get_service_placement(service_name, shard_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shard_handle_multi_service_isolation_and_leader_announcement() {
        let node_a = Uuid::random();
        let node_b = Uuid::random();
        let node_c = Uuid::random();

        let handle = ShardHandle::new(node_a, 16);

        // Placement for service "inventory"
        let p_inventory = ShardPlacement::with_service(
            "inventory",
            0,
            node_a,
            vec![node_b, node_c],
            100,
        );
        handle.update_placement(p_inventory).await;

        // Placement for service "orders"
        let p_orders = ShardPlacement::with_service(
            "orders",
            0,
            node_b,
            vec![node_a, node_c],
            100,
        );
        handle.update_placement(p_orders).await;

        // Check isolation by service
        let inventory_shards = handle.my_service_shards("inventory").await;
        assert_eq!(inventory_shards.len(), 1);
        assert_eq!(inventory_shards[0].1, crate::types::ShardRole::Primary);

        let orders_shards = handle.my_service_shards("orders").await;
        assert_eq!(orders_shards.len(), 1);
        assert_eq!(orders_shards[0].1, crate::types::ShardRole::Replica);

        // Check counts
        assert_eq!(handle.assigned_service_count("inventory").await, 1);
        assert_eq!(handle.assigned_service_count("orders").await, 1);
        assert_eq!(handle.assigned_count().await, 2);

        // Test leader announcement: node_b becomes leader of inventory shard 0
        handle.announce_leader("inventory", 0, node_b, 2).await;
        let updated = handle.get_service_placement("inventory", 0).await.unwrap();
        assert_eq!(updated.primary, node_b);
        assert!(updated.replicas.contains(&node_a));
        assert!(!updated.replicas.contains(&node_b));

        // Node A now sees itself as Replica for inventory shard 0
        assert_eq!(handle.my_service_role("inventory", 0).await, Some(crate::types::ShardRole::Replica));
        assert!(handle.is_my_service_replica("inventory", 0).await);
        assert!(!handle.is_my_service_primary("inventory", 0).await);
    }

    #[tokio::test]
    async fn test_shard_handle_announce_role() {
        let node_a = Uuid::random();
        let node_b = Uuid::random();
        let handle_a = ShardHandle::new(node_a, 16);

        // Initial placement with node_b as primary, node_a as replica
        let p = ShardPlacement::with_service("orders", 1, node_b, vec![node_a], 1);
        handle_a.update_placement(p).await;

        // 1. Node A announces it became Leader
        handle_a.announce_role(1, node::MemberRole::Leader).await;
        let p = handle_a.get_service_placement("orders", 1).await.unwrap();
        assert_eq!(p.primary, node_a);
        assert!(p.replicas.contains(&node_b));
        assert!(!p.replicas.contains(&node_a));

        // 2. Node A announces it stepped down to Voter
        handle_a.announce_role(1, node::MemberRole::Voter).await;
        let p = handle_a.get_service_placement("orders", 1).await.unwrap();
        assert_eq!(p.primary, Uuid::NIL);
        assert!(p.replicas.contains(&node_a));

        // 3. Node A announces it became Learner
        handle_a.announce_role(1, node::MemberRole::Learner).await;
        let p = handle_a.get_service_placement("orders", 1).await.unwrap();
        assert!(!p.replicas.contains(&node_a));
    }
}
