//! Dynamic, reloadable tracing service for the Aaron Node framework.
//!
//! Provides structured logging with support for `json` and `pretty` formatting,
//! and reacts to [`ChangeLogLevel`] events published on the node's [`aaron_core::EventHub`]
//! to dynamically alter log level filtering at runtime without process restarts.
//!
//! # Example
//!
//! ```rust
//! use aaron_core::Node;
//! use aaron_tracing::{TracingService, ChangeLogLevel};
//!
//! # async fn doc() -> Result<(), aaron_core::BoxError> {
//! let node = Node::new("tracing-node").with(TracingService::new());
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
