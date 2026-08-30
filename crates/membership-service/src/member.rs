use node::NodeId;
use std::fmt;
use std::net::SocketAddr;

/// Lifecycle state of a cluster member according to the SWIM failure detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum MemberStatus {
    /// The node is active and responding to direct probes.
    #[default]
    Alive,
    /// The node failed direct probe and is undergoing indirect probing/suspect timer.
    Suspect,
    /// The node has been confirmed unreachable and declared dead.
    Dead,
    /// The node voluntarily and gracefully left the cluster.
    Left,
}

impl fmt::Display for MemberStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Alive => write!(f, "alive"),
            Self::Suspect => write!(f, "suspect"),
            Self::Dead => write!(f, "dead"),
            Self::Left => write!(f, "left"),
        }
    }
}

/// Representation of a known peer node in the cluster membership table.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Member {
    /// Unique identity and incarnation of the node.
    pub node_id: NodeId,
    /// Network address (IP:port) where the node's membership UDP listener is bound.
    pub addr: SocketAddr,
    /// Current membership status.
    pub status: MemberStatus,
    /// Incarnation counter used for state conflict resolution (monotonic per node).
    pub incarnation: u64,
}

impl Member {
    /// Creates a new `Member` in the `Alive` state with the node's current incarnation.
    pub fn new(node_id: NodeId, addr: SocketAddr) -> Self {
        let incarnation = node_id.incarnation;
        Self {
            node_id,
            addr,
            status: MemberStatus::Alive,
            incarnation,
        }
    }

    /// Creates a `Member` with explicit status and incarnation.
    pub fn with_status(
        node_id: NodeId,
        addr: SocketAddr,
        status: MemberStatus,
        incarnation: u64,
    ) -> Self {
        Self {
            node_id,
            addr,
            status,
            incarnation,
        }
    }

    /// Returns `true` if the member is in the `Alive` state.
    pub fn is_alive(&self) -> bool {
        self.status == MemberStatus::Alive
    }

    /// Returns `true` if the member is in the `Suspect` state.
    pub fn is_suspect(&self) -> bool {
        self.status == MemberStatus::Suspect
    }

    /// Returns `true` if the member is in the `Dead` state.
    pub fn is_dead(&self) -> bool {
        self.status == MemberStatus::Dead
    }

    /// Returns `true` if the member has voluntarily `Left`.
    pub fn is_left(&self) -> bool {
        self.status == MemberStatus::Left
    }
}

impl fmt::Display for Member {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Member(id={}, addr={}, status={}, incarnation={})",
            self.node_id.id(),
            self.addr,
            self.status,
            self.incarnation
        )
    }
}
