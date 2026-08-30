use std::pin::Pin;

pub mod anon_service;
pub mod command;
pub mod config;
pub mod context;
pub mod service_opts;
pub mod service_trait;

pub use anon_service::{AnonymousService, ServiceHandler, service_fn};
pub use command::{BindClusterIdCommand, StartServiceCommand};
pub use config::{ConfigError, ConfigField, ServiceConfig};
pub use context::Context;
pub use service_opts::{BackoffStrategy, RestartPolicy, ServiceOpts};
pub use service_trait::Service;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
