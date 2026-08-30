use crate::member::Member;
use std::fmt;

/// Unified lifecycle events emitted by the SWIM membership service onto [`node::EventHub`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembershipEvent {
    /// A new node has joined the cluster.
    Joined(Member),
    /// A node was confirmed or reaffirmed Alive.
    Alive(Member),
    /// A node failed health probes and entered Suspect state.
    Suspect(Member),
    /// A node was declared Dead after the suspect timer expired.
    Dead(Member),
    /// A node gracefully left the cluster.
    Left(Member),
    /// The local node refuted a false suspicion against itself by incrementing its incarnation.
    Refuted(Member),
}

impl MembershipEvent {
    /// Returns a reference to the affected member.
    pub fn member(&self) -> &Member {
        match self {
            Self::Joined(m)
            | Self::Alive(m)
            | Self::Suspect(m)
            | Self::Dead(m)
            | Self::Left(m)
            | Self::Refuted(m) => m,
        }
    }
}

impl fmt::Display for MembershipEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Joined(m) => write!(f, "MembershipEvent::Joined({m})"),
            Self::Alive(m) => write!(f, "MembershipEvent::Alive({m})"),
            Self::Suspect(m) => write!(f, "MembershipEvent::Suspect({m})"),
            Self::Dead(m) => write!(f, "MembershipEvent::Dead({m})"),
            Self::Left(m) => write!(f, "MembershipEvent::Left({m})"),
            Self::Refuted(m) => write!(f, "MembershipEvent::Refuted({m})"),
        }
    }
}

/// Command published to `EventHub` to dynamically trigger a cluster join at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinClusterCommand {
    /// Seed node socket address to contact.
    pub seed_addr: std::net::SocketAddr,
    /// Expected cluster ID (must match or establish cluster).
    pub cluster_id: Option<node::Uuid>,
}

impl JoinClusterCommand {
    /// Creates a new `JoinClusterCommand`.
    pub fn new(seed_addr: std::net::SocketAddr, cluster_id: Option<node::Uuid>) -> Self {
        Self {
            seed_addr,
            cluster_id,
        }
    }
}
