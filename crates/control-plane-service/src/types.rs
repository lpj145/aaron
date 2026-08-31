use node::Uuid;
use openraft::declare_raft_types;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Application state machine write command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientRequest {
    Set { key: String, value: String },
    Delete { key: String },
}

/// Response returned when a command is applied to the state machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientResponse {
    pub success: bool,
    pub value: Option<String>,
}

/// Node representation within the Control Plane Raft cluster.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ControlPlaneNode {
    /// Remote socket address (host:port) of the node's Control Plane QUIC listener.
    pub addr: String,
    /// 128-bit Node UUID high bits.
    pub node_uuid_high: u64,
    /// 128-bit Node UUID low bits.
    pub node_uuid_low: u64,
}

impl ControlPlaneNode {
    pub fn new(addr: impl Into<String>, uuid: Uuid) -> Self {
        Self {
            addr: addr.into(),
            node_uuid_high: uuid.high,
            node_uuid_low: uuid.low,
        }
    }

    pub fn node_uuid(&self) -> Uuid {
        Uuid::new(self.node_uuid_high, self.node_uuid_low)
    }
}

impl fmt::Display for ControlPlaneNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.node_uuid(), self.addr)
    }
}

// Declare OpenRaft types
declare_raft_types!(
    pub TypeConfig:
        D = ClientRequest,
        R = ClientResponse,
        NodeId = u64,
        Node = ControlPlaneNode,
        Entry = openraft::Entry<TypeConfig>,
        SnapshotData = std::io::Cursor<Vec<u8>>,
);

pub type Raft = openraft::Raft<TypeConfig>;
pub type LogId = openraft::LogId<u64>;
pub type Vote = openraft::Vote<u64>;
pub type StoredMembership = openraft::StoredMembership<u64, ControlPlaneNode>;
pub type Membership = openraft::Membership<u64, ControlPlaneNode>;
pub type Entry = openraft::Entry<TypeConfig>;
pub type EntryPayload = openraft::EntryPayload<TypeConfig>;
pub type Snapshot = openraft::Snapshot<TypeConfig>;
pub type SnapshotMeta = openraft::SnapshotMeta<u64, ControlPlaneNode>;
