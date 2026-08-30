//! Dynamic, reloadable tracing service for the Aaron Node framework.
//!
//! Provides structured logging with support for `json` and `pretty` formatting,
//! and reacts to [`ChangeLogLevel`] events published on the node's [`node::EventHub`]
//! to dynamically alter log level filtering at runtime without process restarts.
//!
//! # Example
//!
//! ```rust
//! use node::Node;
//! use tracing_service::{TracingService, ChangeLogLevel};
//!
//! # async fn doc() -> Result<(), node::BoxError> {
//! let node = Node::new().with(TracingService::new());
//! # Ok(())
//! # }
//! ```

pub mod config;
pub mod error;
pub mod event;
pub mod service;

pub use config::{LogFormat, TracingConfig};
pub use error::TracingError;
pub use event::ChangeLogLevel;
pub use service::{ReloadHandle, TracingService};
