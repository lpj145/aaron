use crate::message::RaftMessage;
use crate::storage::ControlPlaneStorage;
use crate::types::{ClientRequest, ClientResponse, ControlPlaneNode, Raft};
use node::{EventHub, QuicManager, Uuid};
use openraft::error::{ClientWriteError, InitializeError, RaftError};
use openraft::RaftMetrics;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{watch, OnceCell, RwLock};

#[derive(Clone)]
struct Inner {
    raft: Raft,
    storage: ControlPlaneStorage,
    metrics_rx: watch::Receiver<RaftMetrics<u64, ControlPlaneNode>>,
    node_id: u64,
    local_uuid: Uuid,
    quic: QuicManager,
    event_hub: EventHub,
    routing_table: Arc<RwLock<HashMap<u64, SocketAddr>>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeTelemetrySnapshot {
    pub node_id: Uuid,
    pub current_wps: u32,
    pub error_rate: u32,
    pub updated_at: u64,
}

/// Handle for controlling and querying the Raft Control Plane.
#[derive(Clone)]
pub struct ControlPlaneHandle {
    inner: Arc<OnceCell<Inner>>,
    telemetry_cache: Arc<RwLock<HashMap<Uuid, NodeTelemetrySnapshot>>>,
}

impl Default for ControlPlaneHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlPlaneHandle {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(OnceCell::new()),
            telemetry_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(crate) fn init(
        &self,
        raft: Raft,
        storage: ControlPlaneStorage,
        metrics_rx: watch::Receiver<RaftMetrics<u64, ControlPlaneNode>>,
        node_id: u64,
        local_uuid: Uuid,
        quic: QuicManager,
        event_hub: EventHub,
        routing_table: Arc<RwLock<HashMap<u64, SocketAddr>>>,
    ) {
        let _ = self.inner.set(Inner {
            raft,
            storage,
            metrics_rx,
            node_id,
            local_uuid,
            quic,
            event_hub,
            routing_table,
        });
    }

    /// Dispatches a shard assignment command to a target node.
    /// If target is the local node, it immediately emits `ShardEvent::Assigned` to `EventHub`.
    /// Otherwise, it transmits a `ShardCommand` frame over the Control Plane QUIC connection.
    pub async fn dispatch_shard_command(
        &self,
        target_uuid: Uuid,
        shard_id: u32,
        role: u8,
        primary: Uuid,
        replicas: &[Uuid],
        epoch: u64,
    ) -> Result<(), node::BoxError> {
        let inner = self.inner.get().ok_or("Control plane not initialized")?;

        if target_uuid == inner.local_uuid {
            let member_role = if role == 0 {
                node::MemberRole::Leader
            } else {
                node::MemberRole::Voter
            };
            let mut members = vec![primary];
            members.extend_from_slice(replicas);
            let event = node::ShardEvent::Join {
                shard_id,
                members,
                role: member_role,
            };
            inner.event_hub.publish(event).await;
            return Ok(());
        }

        // Resolves target address
        let target_id_u64 = target_uuid.low;
        let maybe_addr = {
            let table = inner.routing_table.read().await;
            table.get(&target_id_u64).map(|a| a.to_string())
        };

        let target_addr = if let Some(a) = maybe_addr {
            a
        } else {
            let metrics = inner.metrics_rx.borrow().clone();
            metrics
                .membership_config
                .membership()
                .nodes()
                .find(|(nid, _)| **nid == target_id_u64)
                .map(|(_, node)| node.addr.clone())
                .ok_or_else(|| format!("Target node {target_uuid} not found in routing table or Raft membership"))?
        };

        let conn = inner.quic.connect_node(&target_addr, target_uuid).await?;
        let (mut send, mut recv) = conn.open_bi().await?;
        let replicas_proto = replicas.iter().map(|u| (u.high, u.low)).collect();
        let msg = RaftMessage::ShardCommand {
            shard_id,
            role,
            primary_high: primary.high,
            primary_low: primary.low,
            replicas: replicas_proto,
            epoch,
            op_type: 0,
            target_role: role,
        };
        let bytes = msg.to_bytes();
        node::write_frame(&mut send, &bytes).await?;
        let _ = send.finish();

        let _resp_bytes = node::read_frame(&mut recv).await?;
        Ok(())
    }

    /// Envia um comando estruturado de transição Raft para um nó alvo e aguarda a resposta tipada.
    pub async fn send_raft_shard_command(
        &self,
        target_uuid: Uuid,
        shard_id: u32,
        op_type: u8, // 0 = AssignGroup, 1 = SetRole, 2 = Leave
        target_role: u8, // 0 = Learner, 1 = Voter, 2 = Leader
        members: &[Uuid],
    ) -> Result<RaftMessage, Box<dyn std::error::Error + Send + Sync>> {
        let inner = self.inner.get().ok_or("Control Plane not initialized")?;

        let target_id_u64 = target_uuid.high;
        let maybe_addr = {
            let table = inner.routing_table.read().await;
            table.get(&target_id_u64).map(|a| a.to_string())
        };

        let target_addr = if let Some(a) = maybe_addr {
            a
        } else {
            let metrics = inner.metrics_rx.borrow().clone();
            metrics
                .membership_config
                .membership()
                .nodes()
                .find(|(nid, _)| **nid == target_id_u64)
                .map(|(_, node)| node.addr.clone())
                .ok_or_else(|| format!("Target node {target_uuid} not found in routing table or Raft membership"))?
        };

        let conn = inner.quic.connect_node(&target_addr, target_uuid).await?;
        let (mut send, mut recv) = conn.open_bi().await?;

        let (primary, replicas) = if members.is_empty() {
            (target_uuid, Vec::new())
        } else {
            (members[0], members[1..].to_vec())
        };

        let replicas_proto = replicas.iter().map(|u| (u.high, u.low)).collect();
        let msg = RaftMessage::ShardCommand {
            shard_id,
            role: target_role,
            primary_high: primary.high,
            primary_low: primary.low,
            replicas: replicas_proto,
            epoch: 0,
            op_type,
            target_role,
        };
        let bytes = msg.to_bytes();
        node::write_frame(&mut send, &bytes).await?;
        let _ = send.finish();

        if let Some(resp_bytes) = node::read_frame(&mut recv).await? {
            let resp = RaftMessage::from_bytes(&resp_bytes)?;
            Ok(resp)
        } else {
            Err("No response received for ShardCommand".into())
        }
    }

    /// Returns the local Raft node ID.
    pub fn node_id(&self) -> Option<u64> {
        self.inner.get().map(|i| i.node_id)
    }

    /// Initializes the Raft cluster with the specified initial voters.
    pub async fn initialize(
        &self,
        voters: BTreeMap<u64, ControlPlaneNode>,
    ) -> Result<(), RaftError<u64, InitializeError<u64, ControlPlaneNode>>> {
        let inner = self.inner.get().ok_or({
            RaftError::Fatal(openraft::error::Fatal::Panicked)
        })?;
        inner.raft.initialize(voters).await
    }

    /// Adds a remote node as a non-voting Learner.
    pub async fn add_learner(
        &self,
        id: u64,
        node: ControlPlaneNode,
        blocking: bool,
    ) -> Result<ClientResponse, RaftError<u64, ClientWriteError<u64, ControlPlaneNode>>> {
        let inner = self.inner.get().ok_or({
            RaftError::Fatal(openraft::error::Fatal::Panicked)
        })?;
        let resp = inner.raft.add_learner(id, node, blocking).await?;
        Ok(resp.data)
    }

    /// Changes the cluster voter membership.
    pub async fn change_membership(
        &self,
        voter_ids: BTreeSet<u64>,
        retain: bool,
    ) -> Result<ClientResponse, RaftError<u64, ClientWriteError<u64, ControlPlaneNode>>> {
        let inner = self.inner.get().ok_or({
            RaftError::Fatal(openraft::error::Fatal::Panicked)
        })?;
        let resp = inner.raft.change_membership(voter_ids, retain).await?;
        Ok(resp.data)
    }

    /// Removes a node (learner or voter) completely from the Raft consensus group.
    pub async fn remove_node_from_raft(
        &self,
        node_id: u64,
    ) -> Result<ClientResponse, RaftError<u64, ClientWriteError<u64, ControlPlaneNode>>> {
        let inner = self.inner.get().ok_or({
            RaftError::Fatal(openraft::error::Fatal::Panicked)
        })?;

        // 1. Check if node is currently a voter; if so, remove from voters first with retain = false
        let metrics = inner.metrics_rx.borrow().clone();
        if metrics.membership_config.membership().voter_ids().any(|v| v == node_id) {
            let mut voters: BTreeSet<u64> = metrics.membership_config.membership().voter_ids().collect();
            voters.remove(&node_id);
            if !voters.is_empty() {
                let _ = inner.raft.change_membership(voters, false).await?;
            }
        }

        // 2. Remove node from learners / nodes list
        let mut ids = BTreeSet::new();
        ids.insert(node_id);
        let resp = inner
            .raft
            .change_membership(openraft::ChangeMembers::RemoveNodes(ids), false)
            .await?;
        Ok(resp.data)
    }

    /// Executes a linearizable write (`Set`) through the Raft leader with raw bytes or string.
    pub async fn set(
        &self,
        key: impl Into<String>,
        value: impl AsRef<[u8]>,
    ) -> Result<ClientResponse, RaftError<u64, ClientWriteError<u64, ControlPlaneNode>>> {
        let inner = self.inner.get().ok_or({
            RaftError::Fatal(openraft::error::Fatal::Panicked)
        })?;
        let req = ClientRequest::Set {
            key: key.into(),
            value: value.as_ref().to_vec(),
        };
        let resp = inner.raft.client_write(req).await?;
        Ok(resp.data)
    }

    /// Executes an atomic batch write (`SetBatch`) through the Raft leader.
    pub async fn set_batch(
        &self,
        entries: Vec<(String, Vec<u8>)>,
    ) -> Result<ClientResponse, RaftError<u64, ClientWriteError<u64, ControlPlaneNode>>> {
        let inner = self.inner.get().ok_or({
            RaftError::Fatal(openraft::error::Fatal::Panicked)
        })?;
        let req = ClientRequest::SetBatch { entries };
        let resp = inner.raft.client_write(req).await?;
        Ok(resp.data)
    }

    /// Executes a linearizable delete through the Raft leader.
    pub async fn delete(
        &self,
        key: impl Into<String>,
    ) -> Result<ClientResponse, RaftError<u64, ClientWriteError<u64, ControlPlaneNode>>> {
        let inner = self.inner.get().ok_or({
            RaftError::Fatal(openraft::error::Fatal::Panicked)
        })?;
        let req = ClientRequest::Delete { key: key.into() };
        let resp = inner.raft.client_write(req).await?;
        Ok(resp.data)
    }

    /// Performs a local read from the replicated state machine returning raw bytes.
    pub async fn get(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(inner) = self.inner.get() {
            inner.storage.get_data(key).await
        } else {
            None
        }
    }

    /// Performs a local read from the replicated state machine returning a String.
    pub async fn get_string(&self, key: &str) -> Option<String> {
        self.get(key).await.and_then(|b| String::from_utf8(b).ok())
    }

    /// Returns a map of all key-value entries in raw binary format.
    pub async fn all_data(&self) -> BTreeMap<String, Vec<u8>> {
        if let Some(inner) = self.inner.get() {
            inner.storage.all_data().await
        } else {
            BTreeMap::new()
        }
    }

    /// Returns a map of key-value entries matching a specific prefix.
    pub async fn prefix_data(&self, prefix: &str) -> BTreeMap<String, Vec<u8>> {
        if let Some(inner) = self.inner.get() {
            inner.storage.prefix_data(prefix).await
        } else {
            BTreeMap::new()
        }
    }

    /// Returns a map of all key-value entries as string (for dashboard and inspection).
    pub async fn all_data_strings(&self) -> BTreeMap<String, String> {
        if let Some(inner) = self.inner.get() {
            inner.storage.all_data_strings().await
        } else {
            BTreeMap::new()
        }
    }

    /// Returns the latest metrics snapshot of the Raft node.
    pub fn metrics(&self) -> Option<RaftMetrics<u64, ControlPlaneNode>> {
        self.inner.get().map(|i| i.metrics_rx.borrow().clone())
    }

    /// Returns `true` if the local node is currently the elected Raft Leader.
    pub fn is_leader(&self) -> bool {
        if let Some(inner) = self.inner.get() {
            inner.metrics_rx.borrow().current_leader == Some(inner.node_id)
        } else {
            false
        }
    }

    /// Returns the node ID of the current Raft leader, if one is elected.
    pub fn current_leader(&self) -> Option<u64> {
        self.inner.get().and_then(|i| i.metrics_rx.borrow().current_leader)
    }

    /// Records or updates the dynamic telemetry snapshot (WPS, Error Rate) for a specific node.
    pub async fn record_node_telemetry(&self, node_id: Uuid, current_wps: u32, error_rate: u32, updated_at: u64) {
        self.telemetry_cache.write().await.insert(node_id, NodeTelemetrySnapshot {
            node_id,
            current_wps,
            error_rate,
            updated_at,
        });
    }

    /// Returns the latest telemetry snapshot for a given node ID.
    pub async fn get_node_telemetry(&self, node_id: Uuid) -> Option<NodeTelemetrySnapshot> {
        self.telemetry_cache.read().await.get(&node_id).cloned()
    }

    /// Returns all cached node telemetry snapshots.
    pub async fn all_node_telemetry(&self) -> HashMap<Uuid, NodeTelemetrySnapshot> {
        self.telemetry_cache.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_control_plane_telemetry_cache() {
        let handle = ControlPlaneHandle::new();
        let test_node = Uuid::random();

        assert!(handle.get_node_telemetry(test_node).await.is_none());
        assert!(handle.all_node_telemetry().await.is_empty());

        handle.record_node_telemetry(test_node, 720, 2, 1000).await;

        let snap = handle.get_node_telemetry(test_node).await.expect("snapshot not found");
        assert_eq!(snap.node_id, test_node);
        assert_eq!(snap.current_wps, 720);
        assert_eq!(snap.error_rate, 2);
        assert_eq!(snap.updated_at, 1000);

        let all = handle.all_node_telemetry().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all.get(&test_node).unwrap().current_wps, 720);
    }
}
