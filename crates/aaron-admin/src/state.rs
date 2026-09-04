use aaron_control_plane::ControlPlaneHandle;
use aaron_membership::MembershipHandle;
use aaron_core::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use aaron_shard::ShardHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFieldMetadata {
    pub name: String,
    pub type_name: String,
    pub required: bool,
    pub default: Option<String>,
    pub description: String,
    pub current_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceMetadata {
    pub name: String,
    pub schema: Vec<ConfigFieldMetadata>,
}

#[derive(Clone)]
pub struct AppState {
    pub ctx: Context,
    pub membership: Option<MembershipHandle>,
    pub control_plane: Option<ControlPlaneHandle>,
    pub shard: Option<ShardHandle>,
    pub start_time: Instant,
    pub static_dir: Option<PathBuf>,
    pub services: Arc<Vec<ServiceMetadata>>,
}
