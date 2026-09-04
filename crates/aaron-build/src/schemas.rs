//! Embedded standard Aaron FlatBuffers schemas (.fbs).

/// Standard 128-bit Node and UUID schema (`schemas/node.fbs`).
pub const NODE_FBS: &str = include_str!("../schemas/node.fbs");

/// Standard cluster membership schema (`schemas/membership.fbs`).
pub const MEMBERSHIP_FBS: &str = include_str!("../schemas/membership.fbs");

/// Standard Control Plane Raft storage and command schema (`schemas/control_plane.fbs`).
pub const CONTROL_PLANE_FBS: &str = include_str!("../schemas/control_plane.fbs");

/// Standard multi-service shard placement storage schema (`schemas/shard.fbs`).
pub const SHARD_FBS: &str = include_str!("../schemas/shard.fbs");
