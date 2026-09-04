use super::config::{ConfigField, ServiceConfig};
use super::context::Context;
use super::{BoxError, BoxFuture};
use std::sync::Arc;

/// Contract for services managed and supervised by the node daemon.
pub trait Service: Send + Sync + 'static {
    /// Strongly-typed configuration schema for this service.
    type Config: ServiceConfig;

    /// Name identifier of the service (used in logs, errors, and .env.example sections).
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    /// Explicit cluster capabilities and roles provided by this service (e.g. "control-plane", "shard", "shard-worker").
    fn capabilities(&self) -> Vec<&str> {
        vec![]
    }

    /// Supervised execution loop.
    ///
    /// Runs as an asynchronous task under supervisor monitoring.
    fn run(&self, ctx: Context) -> impl Future<Output = Result<(), BoxError>> + Send;
}

impl<S: Service> Service for Arc<S> {
    type Config = S::Config;

    fn name(&self) -> &str {
        (**self).name()
    }

    fn capabilities(&self) -> Vec<&str> {
        (**self).capabilities()
    }

    fn run(&self, ctx: Context) -> impl Future<Output = Result<(), BoxError>> + Send {
        (**self).run(ctx)
    }
}

pub(crate) trait DynService: Send + Sync + 'static {
    fn dyn_name(&self) -> &str;
    fn dyn_capabilities(&self) -> Vec<String>;
    fn dyn_schema(&self) -> Vec<ConfigField>;
    fn dyn_run<'a>(&'a self, ctx: Context) -> BoxFuture<'a, Result<(), BoxError>>;
}

impl<S: Service> DynService for S {
    fn dyn_name(&self) -> &str {
        Service::name(self)
    }

    fn dyn_capabilities(&self) -> Vec<String> {
        Service::capabilities(self)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    fn dyn_schema(&self) -> Vec<ConfigField> {
        S::Config::schema()
    }

    fn dyn_run<'a>(&'a self, ctx: Context) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(Service::run(self, ctx))
    }
}
