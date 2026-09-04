pub mod config;
pub mod handle;
pub mod message;
pub mod network;
#[allow(clippy::all, clippy::pedantic, clippy::nursery, unused_imports, dead_code)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/control_plane_generated.rs"));
}
pub mod service;
pub mod storage;
pub mod types;

pub use config::ControlPlaneConfig;
pub use handle::{ControlPlaneHandle, NodeTelemetrySnapshot};
pub use message::RaftMessage;
pub use service::ControlPlaneService;
pub use types::{ClientRequest, ClientResponse, ControlPlaneNode, TypeConfig};
