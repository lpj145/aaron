//! Shard Service for Aaron Control Plane (Stage 1: Assignment).

pub mod config;
pub mod coordinator;
pub mod error;
pub mod handle;
pub mod service;
pub mod types;

pub use config::ShardConfig;
pub use coordinator::ShardCoordinator;
pub use error::ShardError;
pub use handle::ShardHandle;
pub use service::ShardService;
pub use types::{ShardEvent, ShardId, ShardPlacement, ShardRole, ShardStatus};
