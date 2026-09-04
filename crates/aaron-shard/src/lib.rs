//! Shard Service for Aaron Control Plane (Stage 1: Assignment).

pub mod config;
pub mod coordinator;
pub mod error;
pub mod handle;
pub mod proto;
pub mod route;
pub mod service;
pub mod types;

pub use config::ShardConfig;
pub use coordinator::ShardCoordinator;
pub use error::ShardError;
pub use handle::ShardHandle;
pub use route::{
    decode_shard_key_u16, decode_shard_key_u32, determine_shard, encode_shard_key_u16,
    encode_shard_key_u32, fnv1a_64, shard_prefix_u16, shard_prefix_u32, wyhash_64, Router,
    ShardKey, WYHASH_CLUSTER_SEED,
};
pub use service::ShardService;
pub use types::{MemberRole, ShardEvent, ShardGroup, ShardId, ShardPlacement, ShardRole, ShardStatus};
