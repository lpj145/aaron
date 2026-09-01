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

/// Handle for controlling and querying the Raft Control Plane.
#[derive(Clone, Default)]
pub struct ControlPlaneHandle {
    inner: Arc<OnceCell<Inner>>,
}

impl ControlPlaneHandle {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(OnceCell::new()),
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
            let shard_role = if role == 0 {
                node::ShardRole::Primary
            } else {
                node::ShardRole::Replica
            };
            let event = node::ShardEvent::Assigned {
                shard_id,
                role: shard_role,
                primary,
                replicas: replicas.to_vec(),
                epoch,
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
        };
        let bytes = msg.to_bytes();
        node::write_frame(&mut send, &bytes).await?;
        let _ = send.finish();

        let _resp_bytes = node::read_frame(&mut recv).await?;
        Ok(())
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

    /// Executes a linearizable write (`Set`) through the Raft leader.
    pub async fn set(
        &self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<ClientResponse, RaftError<u64, ClientWriteError<u64, ControlPlaneNode>>> {
        let inner = self.inner.get().ok_or({
            RaftError::Fatal(openraft::error::Fatal::Panicked)
        })?;
        let req = ClientRequest::Set {
            key: key.into(),
            value: value.into(),
        };
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

    /// Performs a local read from the replicated state machine.
    pub async fn get(&self, key: &str) -> Option<String> {
        if let Some(inner) = self.inner.get() {
            inner.storage.get_data(key).await
        } else {
            None
        }
    }

    /// Returns a map of all key-value entries in the replicated state machine.
    pub async fn all_data(&self) -> BTreeMap<String, String> {
        if let Some(inner) = self.inner.get() {
            inner.storage.all_data().await
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
}
