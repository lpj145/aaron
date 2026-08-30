use super::BoxError;
use super::context::Context;
use super::service_trait::Service;

/// Trait for closures and functions that can handle service execution.
pub trait ServiceHandler: Send + Sync + 'static {
    fn call(&self, ctx: Context) -> impl Future<Output = Result<(), BoxError>> + Send;
}

impl<F, Fut> ServiceHandler for F
where
    F: Fn(Context) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), BoxError>> + Send + 'static,
{
    fn call(&self, ctx: Context) -> impl Future<Output = Result<(), BoxError>> + Send {
        (self)(ctx)
    }
}

/// An anonymous service created from a name and an async handler.
pub struct AnonymousService<H> {
    name: &'static str,
    handler: H,
}

impl<H: ServiceHandler> Service for AnonymousService<H> {
    type Config = ();

    fn name(&self) -> &str {
        self.name
    }

    fn run(&self, ctx: Context) -> impl Future<Output = Result<(), BoxError>> + Send {
        self.handler.call(ctx)
    }
}

/// Creates an anonymous service providing an async handler function receiving `Context`.
///
/// # Example
///
/// ```rust
/// use node::{service_fn, Context, BoxError};
///
/// let svc = service_fn("worker", |ctx: Context| async move {
///     println!("Running node {}", ctx.identity.id());
///     Ok(())
/// });
/// ```
pub fn service_fn<H: ServiceHandler>(name: &'static str, handler: H) -> AnonymousService<H> {
    AnonymousService { name, handler }
}
