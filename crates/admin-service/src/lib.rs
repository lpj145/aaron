//! Admin Dashboard Service for the Aaron distributed runtime.
//!
//! Serves an embedded Vue.js 3 single-page application (SPA) and REST/SSE management APIs,
//! enabling full cluster topology monitoring, dynamic log filter reloading, supervised services
//! introspection, and LSM-tree key-value storage exploration.

pub mod api;
pub mod config;
pub mod error;
pub mod k8s;
pub mod service;
pub mod state;
pub mod static_files;

pub use config::AdminConfig;
pub use error::AdminError;
pub use service::AdminService;
pub use state::AppState;
