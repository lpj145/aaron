use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};

use tokio::task::{JoinError, JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{BoxError, Context, ServiceOpts, service::service_trait::DynService};

#[derive(Clone)]
pub struct SupervisedService {
    pub(crate) service: Arc<dyn DynService>,
    pub(crate) opts: Arc<ServiceOpts>,
}

#[cfg(feature = "test-util")]
impl SupervisedService {
    /// Wraps a service and its options for direct use with [`supervise`].
    ///
    /// Only available under the `test-util` feature — this exists so integration
    /// tests can exercise `supervise()` directly instead of only through [`crate::Node`].
    #[allow(dead_code)]
    pub fn new<S: crate::Service>(service: S, opts: ServiceOpts) -> Self {
        Self {
            service: Arc::new(service),
            opts: Arc::new(opts),
        }
    }
}

/// Wraps a `JoinHandle` and aborts the underlying task if this future is
/// dropped before completing. Without this, dropping a `JoinHandle` (e.g.
/// when the *outer* supervising task is itself aborted on shutdown) merely
/// detaches the inner task instead of stopping it, leaving it running on
/// the runtime forever.
struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl<T> Future for AbortOnDrop<T> {
    type Output = Result<T, JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().0).poll(cx)
    }
}

pub async fn supervise(
    svc: SupervisedService,
    ctx: Context,
    token: CancellationToken,
) -> TaskResult {
    let mut restarts = 0;
    loop {
        let service = svc.service.clone();
        let mut run_ctx = ctx.clone();
        run_ctx.token = token.child_token();

        info!("[{}] Initializing...", svc.service.dyn_name());
        let handle = AbortOnDrop(tokio::spawn(async move { service.dyn_run(run_ctx).await }));
        let result = match handle.await {
            Ok(result) => result,
            Err(err) => {
                // A panic (or the task being aborted) is treated as a failed
                // run so `restart_policy` still gets a say, instead of
                // unconditionally giving up here.
                error!("{err}");
                Err(Box::new(err) as BoxError)
            }
        };

        if token.is_cancelled() || !svc.opts.should_restart(&result, restarts) {
            return result.into();
        }

        let wait_to_restart = svc.opts.retry_delay(restarts);
        tokio::select! {
            _ = tokio::time::sleep(wait_to_restart) => {}
            _ = token.cancelled() => {
                return result.into();
            }
        }

        restarts += 1;
    }
}

pub enum TaskResult {
    Error,
    Success,
}

impl std::fmt::Display for TaskResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskResult::Error => f.write_str("Error"),
            TaskResult::Success => f.write_str("Success"),
        }
    }
}

impl From<Result<(), BoxError>> for TaskResult {
    fn from(value: Result<(), BoxError>) -> Self {
        match value {
            Ok(_) => Self::Success,
            Err(_err) => Self::Error,
        }
    }
}
