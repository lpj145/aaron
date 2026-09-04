//! Aaron: High-Performance Distributed Actor & Consensus Framework.
//!
//! This crate provides the main facade and re-exports Aaron's modular engine components:
//! - [`aaron_core`]: Node runtime container, supervision tree, event hub, and LSM storage (always included).
//! - [`tracing`]: Dynamic log filtering and distributed telemetry service (feature `tracing`).
//! - [`membership`]: SWIM gossip membership protocol over QUIC FlatBuffers (feature `membership`).
//! - [`control_plane`]: Raft consensus state machine and metadata coordination (feature `control-plane`).
//! - [`shard`]: Partition routing and distributed sharding (feature `shard`).
//! - [`admin`]: Embedded Vue.js administration dashboard and REST/SSE APIs (feature `admin`).

// Re-export core types unconditionally
pub use aaron_core::*;

// Feature-gated flat exports
#[cfg(feature = "tracing")]
pub use aaron_tracing::{
    ChangeLogLevel, LogFormat, ReloadHandle, TracingConfig, TracingError, TracingService,
};

#[cfg(feature = "membership")]
pub use aaron_membership::{
    JoinClusterCommand, Member, MemberStatus, MembershipError, MembershipEvent, MembershipHandle,
    MembershipService, UpdateSwimConfig,
};

#[cfg(feature = "admin")]
pub use aaron_admin::{AdminConfig, AdminError, AdminService};

#[cfg(feature = "control-plane")]
pub use aaron_control_plane::{
    ClientRequest, ClientResponse, ControlPlaneConfig, ControlPlaneHandle, ControlPlaneNode,
    ControlPlaneService, RaftMessage,
};

#[cfg(feature = "shard")]
pub use aaron_shard::{
    ShardConfig, ShardCoordinator, ShardError, ShardEvent, ShardHandle, ShardId, ShardPlacement,
    ShardRole, ShardService, ShardStatus,
};

// Feature-gated namespaced modules
#[cfg(feature = "tracing")]
pub mod tracing {
    pub use aaron_tracing::*;
}

#[cfg(feature = "membership")]
pub mod membership {
    pub use aaron_membership::*;
}

#[cfg(feature = "control-plane")]
pub mod control_plane {
    pub use aaron_control_plane::*;
}

#[cfg(feature = "shard")]
pub mod shard {
    pub use aaron_shard::*;
}

#[cfg(feature = "admin")]
pub mod admin {
    pub use aaron_admin::*;
}