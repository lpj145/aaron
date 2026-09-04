//! Integration tests for the `supervise()` restart loop.
//!
//! These reach `supervise`/`SupervisedService`/`TaskResult` — internal plumbing not
//! part of the normal public API — via the `test-util` feature, enabled for this
//! crate's own tests through the self dev-dependency in `Cargo.toml`.

use aaron_core::{
    BoxError, Context, Env, EventHub, Network, NodeId, Service, ServiceOpts, Store,
    SupervisedService, TaskResult, Uuid, supervise,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn test_context() -> Context {
    static CTR: AtomicUsize = AtomicUsize::new(0);
    let id = CTR.fetch_add(1, Ordering::SeqCst);
    let temp_dir = std::env::temp_dir().join(format!(
        "test_supervise_{}_{}_{}",
        std::process::id(),
        id,
        Uuid::random()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let store = Store::open(&temp_dir).unwrap();
    let node_uuid = Uuid::new(1, 2);
    let identity = NodeId::with_current_incarnation(node_uuid, None);
    let env = Arc::new(Env::detect());
    let token = CancellationToken::new();
    Context::new(EventHub::new(), Network::new(), store, identity, env, token)
}

/// Polls `pred` until it returns `true`, or panics after ~500ms.
/// Used instead of a fixed `sleep` to avoid flaky timing assumptions
/// about exactly when a spawned task has made progress.
async fn wait_until(mut pred: impl FnMut() -> bool) {
    for _ in 0..100 {
        if pred() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition not met in time");
}

/// Fails on its first `fail_until` calls, then succeeds.
struct CountingService {
    runs: Arc<AtomicU32>,
    fail_until: u32,
}

impl Service for CountingService {
    type Config = ();

    fn name(&self) -> &str {
        "counting-service"
    }

    async fn run(&self, _ctx: Context) -> Result<(), BoxError> {
        let n = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        if n <= self.fail_until {
            Err("boom".into())
        } else {
            Ok(())
        }
    }
}

/// Panics on its first call, then succeeds.
struct PanicOnceService {
    runs: Arc<AtomicU32>,
}

impl Service for PanicOnceService {
    type Config = ();

    fn name(&self) -> &str {
        "panic-once-service"
    }

    async fn run(&self, _ctx: Context) -> Result<(), BoxError> {
        let n = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        if n == 1 {
            panic!("boom");
        }
        Ok(())
    }
}

/// Never returns; ticks a counter so tests can observe whether it's still running.
struct HeartbeatService {
    ticks: Arc<AtomicU32>,
}

impl Service for HeartbeatService {
    type Config = ();

    fn name(&self) -> &str {
        "heartbeat-service"
    }

    async fn run(&self, _ctx: Context) -> Result<(), BoxError> {
        loop {
            self.ticks.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

struct TokenWatchingService {
    saw_cancelled: Arc<AtomicBool>,
}

impl Service for TokenWatchingService {
    type Config = ();

    fn name(&self) -> &str {
        "token-watching-service"
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        ctx.token.cancelled().await;
        self.saw_cancelled.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn restarts_on_failure_until_policy_stops() {
    let runs = Arc::new(AtomicU32::new(0));
    let svc = SupervisedService::new(
        CountingService {
            runs: runs.clone(),
            fail_until: 2,
        },
        ServiceOpts::new().restart_on_failure(),
    );

    let result = supervise(svc, test_context(), CancellationToken::new()).await;

    assert_eq!(
        runs.load(Ordering::SeqCst),
        3,
        "should fail twice then succeed on the 3rd run"
    );
    assert!(matches!(result, TaskResult::Success));
}

#[tokio::test]
async fn never_policy_does_not_restart_after_failure() {
    let runs = Arc::new(AtomicU32::new(0));
    let svc = SupervisedService::new(
        CountingService {
            runs: runs.clone(),
            fail_until: u32::MAX,
        },
        ServiceOpts::default(), // RestartPolicy::Never
    );

    let result = supervise(svc, test_context(), CancellationToken::new()).await;

    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(matches!(result, TaskResult::Error));
}

#[tokio::test]
async fn panic_is_treated_as_failure_and_respects_restart_policy() {
    // Regression test: a panic used to unconditionally end supervision
    // (`return TaskResult::Error`) regardless of `restart_policy`.
    let runs = Arc::new(AtomicU32::new(0));
    let svc = SupervisedService::new(
        PanicOnceService { runs: runs.clone() },
        ServiceOpts::new().restart_on_failure(),
    );

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {})); // silence the expected panic's default log
    let result = supervise(svc, test_context(), CancellationToken::new()).await;
    std::panic::set_hook(prev_hook);

    assert_eq!(
        runs.load(Ordering::SeqCst),
        2,
        "should restart once after the panic, then succeed"
    );
    assert!(matches!(result, TaskResult::Success));
}

#[tokio::test]
async fn cancelling_during_backoff_stops_without_an_extra_run() {
    // Regression test: cancellation used to only be checked after a run
    // completed, so a service cancelled mid-backoff would still run once more.
    let runs = Arc::new(AtomicU32::new(0));
    let svc = SupervisedService::new(
        CountingService {
            runs: runs.clone(),
            fail_until: u32::MAX,
        },
        ServiceOpts::new()
            .restart_always()
            .constant_backoff(Duration::from_secs(10)),
    );
    let token = CancellationToken::new();
    let handle = tokio::spawn(supervise(svc, test_context(), token.clone()));

    wait_until(|| runs.load(Ordering::SeqCst) >= 1).await;
    tokio::time::sleep(Duration::from_millis(20)).await; // let it reach the backoff sleep
    token.cancel();

    let result = tokio::time::timeout(Duration::from_millis(500), handle)
        .await
        .expect("supervise should stop promptly once cancelled, not wait out the 10s backoff")
        .expect("supervise task should not panic");

    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "cancelling during backoff must not trigger another run"
    );
    assert!(matches!(result, TaskResult::Error));
}

#[tokio::test]
async fn aborting_supervise_also_aborts_the_running_service_task() {
    // Regression test: the service used to run in a nested `tokio::spawn`
    // untracked by the outer supervised task, so aborting/dropping the
    // outer task (as `Node::run()`'s shutdown timeout does) merely
    // detached the inner task instead of stopping it.
    let ticks = Arc::new(AtomicU32::new(0));
    let svc = SupervisedService::new(
        HeartbeatService {
            ticks: ticks.clone(),
        },
        ServiceOpts::default(),
    );
    let outer = tokio::spawn(supervise(svc, test_context(), CancellationToken::new()));

    wait_until(|| ticks.load(Ordering::SeqCst) > 0).await;
    outer.abort();

    tokio::time::sleep(Duration::from_millis(100)).await;
    let after_abort = ticks.load(Ordering::SeqCst);

    tokio::time::sleep(Duration::from_millis(200)).await;
    let later = ticks.load(Ordering::SeqCst);

    assert_eq!(
        after_abort, later,
        "service task kept running after its supervisor was aborted (leaked/detached task)"
    );
}

#[tokio::test]
async fn max_retries_stops_after_configured_limit() {
    let runs = Arc::new(AtomicU32::new(0));
    let svc = SupervisedService::new(
        CountingService {
            runs: runs.clone(),
            fail_until: u32::MAX,
        },
        ServiceOpts::new().max_retries(3),
    );

    let result = supervise(svc, test_context(), CancellationToken::new()).await;

    // Initial run (0) + 3 retries (1, 2, 3) = 4 total runs
    assert_eq!(runs.load(Ordering::SeqCst), 4);
    assert!(matches!(result, TaskResult::Error));
}

#[tokio::test]
async fn on_failure_max_retries_stops_after_configured_failures() {
    let runs = Arc::new(AtomicU32::new(0));
    let svc = SupervisedService::new(
        CountingService {
            runs: runs.clone(),
            fail_until: u32::MAX,
        },
        ServiceOpts::new().on_failure_max_retries(2),
    );

    let result = supervise(svc, test_context(), CancellationToken::new()).await;

    // Initial run (0) + 2 retries (1, 2) = 3 total runs
    assert_eq!(runs.load(Ordering::SeqCst), 3);
    assert!(matches!(result, TaskResult::Error));
}

#[tokio::test]
async fn test_supervise_child_cancellation_token_propagation() {
    let saw_cancelled = Arc::new(AtomicBool::new(false));
    let svc = SupervisedService::new(
        TokenWatchingService {
            saw_cancelled: saw_cancelled.clone(),
        },
        ServiceOpts::default(),
    );

    let parent_token = CancellationToken::new();
    let supervise_handle = tokio::spawn(supervise(svc, test_context(), parent_token.clone()));

    tokio::time::sleep(Duration::from_millis(20)).await;
    parent_token.cancel();

    let res = supervise_handle.await.unwrap();
    assert!(matches!(res, TaskResult::Success));
    assert!(
        saw_cancelled.load(Ordering::SeqCst),
        "service should observe ctx.token cancellation"
    );
}
