//! Chaos/exploratory tests for the supervision restart loop (`supervise`) and its
//! backoff arithmetic (`BackoffStrategy`/`ServiceOpts`).
//!
//! Focus areas not covered by `tests/supervise.rs` (which tests the happy-path restart
//! semantics): hostile/extreme backoff parameters, deep retry counts, panic-loops racing
//! cancellation, and many services with different policies being torn down at once.
//!
//! Nothing in `src/` is modified — these only explore and document behavior.

use aaron_core::{
    BackoffStrategy, BoxError, Context, Env, EventHub, Network, NodeId, RestartPolicy, Service,
    ServiceOpts, Store, SupervisedService, TaskResult, Uuid, supervise,
};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

fn test_context() -> Context {
    static CTR: AtomicUsize = AtomicUsize::new(0);
    let id = CTR.fetch_add(1, Ordering::SeqCst);
    let temp_dir = std::env::temp_dir().join(format!(
        "test_supervise_chaos_{}_{}_{}",
        std::process::id(),
        id,
        Uuid::random()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let store = Store::open(&temp_dir).unwrap();
    let identity = NodeId::with_current_incarnation(Uuid::random(), None);
    let env = Arc::new(Env::detect());
    Context::new(
        EventHub::new(),
        Network::new(),
        store,
        identity,
        env,
        CancellationToken::new(),
    )
}

/// Always fails immediately. Used to drive a supervisor into its backoff path as fast as
/// the runtime allows (a "crash loop").
struct AlwaysFailingService {
    runs: Arc<AtomicU32>,
}

impl Service for AlwaysFailingService {
    type Config = ();

    fn name(&self) -> &str {
        "always-failing-service"
    }

    async fn run(&self, _ctx: Context) -> Result<(), BoxError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Err("boom".into())
    }
}

/// Panics immediately on every single run — a true panic-loop, not a one-shot panic.
struct AlwaysPanickingService {
    runs: Arc<AtomicU32>,
}

impl Service for AlwaysPanickingService {
    type Config = ();

    fn name(&self) -> &str {
        "always-panicking-service"
    }

    async fn run(&self, _ctx: Context) -> Result<(), BoxError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        panic!("panic-loop");
    }
}

/// Waits for its own child token, then reports it observed cancellation.
struct CooperativeService {
    name: &'static str,
    stopped: Arc<AtomicBool>,
}

impl Service for CooperativeService {
    type Config = ();

    fn name(&self) -> &str {
        self.name
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        ctx.token.cancelled().await;
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }
}

/// `BackoffStrategy::Exponential` computes `Duration::from_secs_f64(initial * multiplier^n)`.
/// `Duration::from_secs_f64` *panics* when the value overflows a `Duration`, and the guard
/// in front of it only rejects non-finite/negative values — a finite-but-too-large product
/// slips through. A long-lived crash-looping service with an unbounded exponential backoff
/// walks its retry counter straight into that range.
#[test]
fn test_exponential_backoff_never_panics_at_any_retry_count() {
    let strategies = [
        (
            "initial=1s, x2, no max",
            BackoffStrategy::exponential(Duration::from_secs(1), None, 2.0),
        ),
        (
            "initial=100ms, x10, no max",
            BackoffStrategy::exponential(Duration::from_millis(100), None, 10.0),
        ),
        (
            "initial=1s, x2, max=60s",
            BackoffStrategy::exponential(
                Duration::from_secs(1),
                Some(Duration::from_secs(60)),
                2.0,
            ),
        ),
    ];

    let mut failures = Vec::new();
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    for (label, strategy) in &strategies {
        for retry in 0u32..=256 {
            let res =
                std::panic::catch_unwind(AssertUnwindSafe(|| strategy.calculate_delay(retry)));
            if res.is_err() {
                failures.push(format!("{label} @ retry_count={retry}"));
                break;
            }
        }
    }
    std::panic::set_hook(prev_hook);

    assert!(
        failures.is_empty(),
        "BackoffStrategy::calculate_delay panicked for {} strategy/retry combinations: {:?} \
         (`Duration::from_secs_f64` panics on values that overflow a Duration; the \
         `secs.is_finite() && secs >= 0.0` guard lets those through). A service crash-looping \
         long enough under exponential backoff would take the whole supervisor task down.",
        failures.len(),
        failures
    );
}

/// Hostile/degenerate multipliers a caller can legally pass to `exponential_backoff`.
/// None of these should panic, and none should silently produce a *shorter* delay than
/// requested by `max` semantics.
#[test]
fn test_exponential_backoff_with_degenerate_multipliers_is_sane() {
    let cases: [(&str, f64); 6] = [
        ("NaN", f64::NAN),
        ("+inf", f64::INFINITY),
        ("-inf", f64::NEG_INFINITY),
        ("negative", -2.0),
        ("zero", 0.0),
        ("subnormal", f64::MIN_POSITIVE),
    ];

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut panicked = Vec::new();
    let mut observed = Vec::new();
    for (label, multiplier) in cases {
        let strategy = BackoffStrategy::exponential(Duration::from_secs(1), None, multiplier);
        for retry in [0u32, 1, 5, 32] {
            match std::panic::catch_unwind(AssertUnwindSafe(|| strategy.calculate_delay(retry))) {
                Ok(d) => observed.push((label, retry, d)),
                Err(_) => panicked.push(format!("{label} @ retry={retry}")),
            }
        }
    }
    std::panic::set_hook(prev_hook);

    assert!(
        panicked.is_empty(),
        "calculate_delay panicked for degenerate multipliers: {panicked:?}"
    );

    // A NaN multiplier degrades to `Duration::MAX` — effectively "never restart again",
    // silently, with no error surfaced to the operator. Documented here, not asserted as
    // a failure, so the observed contract is explicit.
    let nan_delays: Vec<_> = observed
        .iter()
        .filter(|(l, _, _)| *l == "NaN")
        .map(|(_, r, d)| (*r, *d))
        .collect();
    assert!(
        nan_delays.iter().all(|(_, d)| *d == Duration::MAX),
        "expected NaN multiplier to collapse to Duration::MAX (an effectively infinite \
         backoff); got {nan_delays:?}"
    );
}

/// A `Duration::MAX` backoff (reachable via a NaN/negative multiplier, or set directly)
/// must still be interruptible: `supervise` selects on `token.cancelled()` alongside the
/// sleep, so cancellation should win immediately rather than hanging until the heat death
/// of the universe (or panicking inside `tokio::time::sleep`).
#[tokio::test]
async fn test_cancel_during_effectively_infinite_backoff_returns_promptly() {
    let runs = Arc::new(AtomicU32::new(0));
    let svc = SupervisedService::new(
        AlwaysFailingService { runs: runs.clone() },
        ServiceOpts::new()
            .restart_always()
            .backoff(BackoffStrategy::Constant(Duration::MAX)),
    );
    let token = CancellationToken::new();
    let handle = tokio::spawn(supervise(svc, test_context(), token.clone()));

    // Let the first run fail and the supervisor enter its (infinite) backoff.
    for _ in 0..100 {
        if runs.load(Ordering::SeqCst) >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "service should have run exactly once"
    );

    let started = Instant::now();
    token.cancel();
    let result = tokio::time::timeout(Duration::from_secs(3), handle).await;

    assert!(
        result.is_ok(),
        "supervise() did not return within 3s of cancellation while sleeping on a \
         Duration::MAX backoff — cancellation is not preempting the backoff sleep"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "cancellation took {:?} to unwind the infinite backoff",
        started.elapsed()
    );
    assert!(matches!(result.unwrap().unwrap(), TaskResult::Error));
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "no extra run after cancellation"
    );
}

/// A service panicking as fast as the runtime can respawn it, with zero backoff, racing a
/// cancellation that lands at an arbitrary point in the loop. The supervisor must stop
/// promptly and must not spin forever after the token is cancelled.
#[tokio::test]
async fn test_panic_loop_with_zero_backoff_stops_on_concurrent_cancel() {
    let runs = Arc::new(AtomicU32::new(0));
    let svc = SupervisedService::new(
        AlwaysPanickingService { runs: runs.clone() },
        ServiceOpts::new()
            .restart_always()
            .backoff(BackoffStrategy::none()),
    );
    let token = CancellationToken::new();

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {})); // silence the flood of expected panics
    let handle = tokio::spawn(supervise(svc, test_context(), token.clone()));

    // Let the panic-loop spin freely for a moment, then cancel mid-flight.
    tokio::time::sleep(Duration::from_millis(60)).await;
    let runs_at_cancel = runs.load(Ordering::SeqCst);
    token.cancel();

    let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
    std::panic::set_hook(prev_hook);

    assert!(
        result.is_ok(),
        "supervise() never returned after cancelling a zero-backoff panic-loop \
         (ran {runs_at_cancel} times before cancel) — the restart loop can outlive its token"
    );

    let runs_after = runs.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        runs.load(Ordering::SeqCst),
        runs_after,
        "the panicking service was still being restarted after supervise() returned"
    );
}

/// Ten services with five different restart policies and backoff strategies, all sharing a
/// single root token that is cancelled at one instant. Every supervisor must return, and
/// every cooperative service must have observed its child token firing.
#[tokio::test]
async fn test_simultaneous_cancel_of_many_policies_tears_everything_down() {
    const NAMES: [&str; 10] = [
        "svc-0", "svc-1", "svc-2", "svc-3", "svc-4", "svc-5", "svc-6", "svc-7", "svc-8", "svc-9",
    ];
    let policies = [
        (RestartPolicy::Never, BackoffStrategy::none()),
        (
            RestartPolicy::Always,
            BackoffStrategy::constant(Duration::from_millis(20)),
        ),
        (
            RestartPolicy::OnFailure,
            BackoffStrategy::linear(Duration::from_millis(5), Duration::from_millis(5), None),
        ),
        (
            RestartPolicy::MaxRetries(3),
            BackoffStrategy::exponential(
                Duration::from_millis(10),
                Some(Duration::from_secs(1)),
                2.0,
            ),
        ),
        (
            RestartPolicy::OnFailureMaxRetries(2),
            BackoffStrategy::constant(Duration::from_secs(30)),
        ),
    ];

    let token = CancellationToken::new();
    let mut flags = Vec::new();
    let mut handles = Vec::new();

    for (i, name) in NAMES.iter().enumerate() {
        let (policy, backoff) = policies[i % policies.len()].clone();
        let stopped = Arc::new(AtomicBool::new(false));
        flags.push((name, stopped.clone()));
        let svc = SupervisedService::new(
            CooperativeService { name, stopped },
            ServiceOpts::new().restart_policy(policy).backoff(backoff),
        );
        handles.push(tokio::spawn(supervise(svc, test_context(), token.clone())));
    }

    // Give every supervisor a chance to actually start its inner service.
    tokio::time::sleep(Duration::from_millis(80)).await;
    token.cancel();

    let all = tokio::time::timeout(Duration::from_secs(5), async {
        for h in handles {
            let _ = h.await;
        }
    })
    .await;

    assert!(
        all.is_ok(),
        "not every supervisor returned within 5s of a simultaneous cancellation across \
         mixed restart policies/backoffs"
    );

    let never_stopped: Vec<_> = flags
        .iter()
        .filter(|(_, f)| !f.load(Ordering::SeqCst))
        .map(|(n, _)| *n)
        .collect();
    assert!(
        never_stopped.is_empty(),
        "these services never observed their child cancellation token firing: {never_stopped:?} \
         (the root token's cancellation must propagate to every per-run child token)"
    );
}

/// `supervise` tracks retries in a `u32` and hands it straight to `should_restart`.
/// This checks the policy boundary arithmetic at the extremes of that counter — an
/// off-by-one or an overflow here turns "restart at most N times" into "never restart"
/// or "restart forever".
#[test]
fn test_retry_policy_boundaries_at_counter_extremes() {
    let ok: Result<(), BoxError> = Ok(());
    let err: Result<(), BoxError> = Err("x".into());

    let opts = ServiceOpts::new().max_retries(u32::MAX);
    assert!(
        opts.should_restart(&err, u32::MAX - 1),
        "MaxRetries(u32::MAX) must still allow a restart one below the ceiling"
    );
    assert!(
        !opts.should_restart(&err, u32::MAX),
        "MaxRetries(u32::MAX) must stop once retry_count reaches the ceiling — otherwise \
         supervise()'s `restarts += 1` would overflow and panic in debug builds"
    );

    let zero = ServiceOpts::new().max_retries(0);
    assert!(
        !zero.should_restart(&err, 0),
        "MaxRetries(0) must never restart"
    );
    assert!(
        !zero.should_restart(&ok, 0),
        "MaxRetries(0) must never restart on success either"
    );

    let on_fail_zero = ServiceOpts::new().on_failure_max_retries(0);
    assert!(!on_fail_zero.should_restart(&err, 0));
    assert!(!on_fail_zero.should_restart(&ok, 0));
}
