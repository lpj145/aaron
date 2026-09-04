//! Chaos/exploratory tests for `TracingService`.
//!
//! The service owns process-global state (the `tracing` subscriber) and mutates it in
//! response to events arriving on the `EventHub`, so it is exposed to two hazards ordinary
//! services are not: a *global* singleton that only the first initializer really owns, and
//! a hot path driven by untrusted-ish event payloads (filter directives).
//!
//! Because the global subscriber is per-process, tests that observe it must not race each
//! other; `GLOBAL_LOCK` serialises them, and `primary_service()` guarantees the process's
//! global subscriber always comes from one known service regardless of test order.
//!
//! Nothing in `src/` is modified — these only explore and document behavior.

use aaron_core::{Context, Env, EventHub, Network, NodeId, Service, Store, Uuid};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::level_filters::LevelFilter;
use aaron_tracing::{ChangeLogLevel, LogFormat, TracingConfig, TracingService};

static GLOBAL_LOCK: Mutex<()> = Mutex::const_new(());
static PRIMARY: OnceLock<TracingService> = OnceLock::new();

/// Returns the one service that owns this process's global tracing subscriber, installing
/// it on first use so the "who won `try_init`" question has a deterministic answer no
/// matter which test runs first.
async fn primary_service() -> &'static TracingService {
    if PRIMARY.get().is_none() {
        let svc = TracingService::with_config(TracingConfig::new().json().log_level("info"));
        svc.init_subscriber(&TracingConfig::new().json().log_level("info"))
            .await
            .unwrap();
        let _ = PRIMARY.set(svc);
    }
    PRIMARY.get().unwrap()
}

fn test_context(token: CancellationToken) -> (Context, tempfile::TempDir) {
    static CTR: AtomicUsize = AtomicUsize::new(0);
    let n = CTR.fetch_add(1, Ordering::SeqCst);
    let tmp = tempfile::Builder::new()
        .prefix(&format!("tracing_chaos_{}_{n}_", std::process::id()))
        .tempdir()
        .unwrap();
    let store = Store::open(&tmp).unwrap();
    let identity = NodeId::with_current_incarnation(Uuid::random(), None);
    let env = Arc::new(Env::detect());
    (
        Context::new(EventHub::new(), Network::new(), store, identity, env, token),
        tmp,
    )
}

/// A second `TracingService` in the same process loses the race for the global subscriber
/// (`try_init` fails and its result is discarded with `let _ =`), yet `init_subscriber`
/// still stores its reload handle and returns `Ok(())`. This pins down what the losing
/// service can and cannot do afterwards: its `reload()` calls fail (the reload layer they
/// point at was never installed), and the effective global level never moves — so a node
/// that registers two tracing services has one that looks healthy at startup and then
/// reports a runtime error on every single `ChangeLogLevel` event it receives.
#[tokio::test]
async fn test_second_tracing_service_cannot_control_the_global_log_level() {
    let _guard = GLOBAL_LOCK.lock().await;
    let primary = primary_service().await;

    // Sanity: the primary really does drive the global level.
    primary.reload("info").await.unwrap();
    assert_eq!(LevelFilter::current(), LevelFilter::INFO);
    primary.reload("trace").await.unwrap();
    assert_eq!(
        LevelFilter::current(),
        LevelFilter::TRACE,
        "the service that won try_init() must be able to move the global level"
    );

    // A second service initialises; its try_init() silently fails.
    let secondary = TracingService::with_config(TracingConfig::new().pretty().log_level("error"));
    secondary
        .init_subscriber(&TracingConfig::new().pretty().log_level("error"))
        .await
        .unwrap();

    let result = secondary.reload("error").await;
    let effective = LevelFilter::current();
    // Restore before asserting so a failure doesn't leave the process at TRACE.
    primary.reload("info").await.unwrap();

    assert!(
        result.is_err(),
        "the losing service's reload() reported success; if it truly took effect the global \
         level would now be ERROR (it is {effective:?})"
    );
    assert_eq!(
        effective,
        LevelFilter::TRACE,
        "a second TracingService managed to move the global level even though it never \
         installed the reload layer"
    );
}

/// A malformed filter directive arriving as an event must be reported and skipped without
/// taking the service down or corrupting the currently active level.
#[tokio::test]
async fn test_invalid_filter_directives_do_not_change_or_break_the_active_level() {
    let _guard = GLOBAL_LOCK.lock().await;
    let primary = primary_service().await;

    primary.reload("warn").await.unwrap();
    assert_eq!(LevelFilter::current(), LevelFilter::WARN);

    let hostile = [
        "",
        "   ",
        "=",
        "not_a_level",
        "node=notalevel",
        "node=debug,,,,",
        "🔥=trace",
        &"a=debug,".repeat(5_000),
        &"x".repeat(1_000_000),
        "node=debug\0trace",
    ];

    // Collect every problem rather than stopping at the first, so one run reports the full
    // picture of which hostile directives are dangerous.
    let mut silenced = Vec::new();
    let mut changed_despite_rejection = Vec::new();

    for directive in hostile {
        let res = primary.reload(directive).await;
        let effective = LevelFilter::current();
        let short = directive[..directive.len().min(40)].to_string();
        if res.is_ok() {
            // If it was accepted, it must have actually meant something sane.
            if effective == LevelFilter::OFF {
                silenced.push(short);
            }
            primary.reload("warn").await.unwrap();
        } else if effective != LevelFilter::WARN {
            changed_despite_rejection.push((short, effective));
        }
    }

    assert!(
        changed_despite_rejection.is_empty(),
        "rejected filter directives still changed the effective level: {changed_despite_rejection:?}"
    );
    assert!(
        silenced.is_empty(),
        "these filter directives were accepted by reload() and turned logging completely OFF \
         for the whole process: {silenced:?}. A `ChangeLogLevel` event carrying an empty or \
         blank filter is treated as a valid directive, so any publisher — including a \
         mis-set LOG_LEVEL env var routed through the same path — can silently blind the \
         node's entire observability stack, with the success path logging \
         \"Log level dynamically reloaded\"."
    );

    // The service is still fully functional after the hostile batch.
    primary.reload("trace").await.unwrap();
    assert_eq!(LevelFilter::current(), LevelFilter::TRACE);
    primary.reload("info").await.unwrap();
}

/// Many tasks publishing `ChangeLogLevel` at the same instant. The service processes them
/// serially off one channel, so the *last* one to be delivered decides the final level —
/// this checks that the burst neither wedges the service nor leaves the level at something
/// that was never requested.
#[tokio::test]
async fn test_concurrent_log_level_changes_converge_to_a_requested_level() {
    let _guard = GLOBAL_LOCK.lock().await;
    let primary = primary_service().await;
    primary.reload("info").await.unwrap();

    const TASKS: usize = 16;
    const PER_TASK: usize = 20;
    let levels = ["trace", "debug", "info", "warn", "error"];

    let mut handles = Vec::new();
    for t in 0..TASKS {
        let svc = primary;
        handles.push(tokio::spawn(async move {
            for i in 0..PER_TASK {
                let level = levels[(t + i) % levels.len()];
                svc.reload(level).await.unwrap();
                tokio::task::yield_now().await;
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let final_level = LevelFilter::current();
    assert!(
        [
            LevelFilter::TRACE,
            LevelFilter::DEBUG,
            LevelFilter::INFO,
            LevelFilter::WARN,
            LevelFilter::ERROR
        ]
        .contains(&final_level),
        "after {} concurrent reloads the global level is {final_level:?}, which is not any \
         of the levels that were requested",
        TASKS * PER_TASK
    );

    // And it is still reloadable afterwards — no poisoned lock, no wedged handle.
    primary.reload("info").await.unwrap();
    assert_eq!(LevelFilter::current(), LevelFilter::INFO);
}

/// The `EventHub` gives every subscriber a bounded (128 item) queue and `publish` drops the
/// event for a subscriber that can't accept it within 15ms. A burst of level changes far
/// larger than that queue therefore silently loses events. This documents how many of a
/// 1000-event burst actually reach a live `TracingService`, and asserts the service is
/// still responsive to a *subsequent* event afterwards (the important property: a flood
/// must not permanently deafen the service).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_event_flood_drops_events_but_never_deafens_the_service() {
    let _guard = GLOBAL_LOCK.lock().await;
    let token = CancellationToken::new();
    let (ctx, _tmp) = test_context(token.clone());

    let service = TracingService::with_config(TracingConfig::new().json().log_level("info"));
    let ctx_task = ctx.clone();
    let svc_task = tokio::spawn(async move { service.run(ctx_task).await });

    // Wait until the service has subscribed.
    for _ in 0..200 {
        if ctx.event_hub.subscriber_count::<ChangeLogLevel>().await > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        ctx.event_hub.subscriber_count::<ChangeLogLevel>().await > 0,
        "TracingService never subscribed to ChangeLogLevel"
    );

    let mut delivered = 0usize;
    for i in 0..1_000 {
        delivered += ctx
            .event_hub
            .publish(ChangeLogLevel::new(if i % 2 == 0 {
                "debug"
            } else {
                "trace"
            }))
            .await;
    }

    // Give the service a moment to drain whatever it accepted.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // The service must still accept a fresh event after the flood.
    let post_flood = ctx.event_hub.publish(ChangeLogLevel::info()).await;
    assert!(
        post_flood >= 1,
        "after a 1000-event flood ({delivered} accepted at publish time) the TracingService \
         no longer accepts ChangeLogLevel events — its subscriber queue is permanently \
         saturated or its event loop exited"
    );

    token.cancel();
    let result = tokio::time::timeout(Duration::from_secs(5), svc_task).await;
    assert!(
        result.is_ok(),
        "TracingService did not exit within 5s of cancellation after the event flood"
    );
    assert!(result.unwrap().unwrap().is_ok());
}

/// Cancellation racing in-flight events: the token is cancelled while a publisher is still
/// pushing `ChangeLogLevel` events. `run()` must return promptly (its `select!` prefers
/// neither branch, so a saturated event stream must not starve the cancellation branch)
/// and must not panic on the events it never got to process.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_shutdown_while_level_changes_are_in_flight_is_prompt() {
    let _guard = GLOBAL_LOCK.lock().await;
    let token = CancellationToken::new();
    let (ctx, _tmp) = test_context(token.clone());

    let service = TracingService::with_config(TracingConfig::new().json().log_level("info"));
    let ctx_task = ctx.clone();
    let svc_task = tokio::spawn(async move { service.run(ctx_task).await });

    for _ in 0..200 {
        if ctx.event_hub.subscriber_count::<ChangeLogLevel>().await > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let pub_ctx = ctx.clone();
    let pub_token = token.clone();
    let publisher = tokio::spawn(async move {
        let mut n = 0u64;
        while !pub_token.is_cancelled() {
            pub_ctx
                .event_hub
                .publish(ChangeLogLevel::new("debug"))
                .await;
            n += 1;
            tokio::task::yield_now().await;
        }
        n
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    let started = std::time::Instant::now();
    token.cancel();

    let result = tokio::time::timeout(Duration::from_secs(5), svc_task).await;
    let published = publisher.await.unwrap();

    assert!(
        result.is_ok(),
        "TracingService::run() did not return within 5s of cancellation while {published} \
         ChangeLogLevel events were being published concurrently — a saturated event stream \
         can starve the cancellation branch of its select!"
    );
    assert!(
        result.unwrap().unwrap().is_ok(),
        "run() returned an error on graceful shutdown"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "shutdown took {:?} under concurrent event load",
        started.elapsed()
    );
}

/// Two `TracingService` instances configured with *different* formats (json vs pretty) both
/// running under one node. Only the first can install a formatter, so the second's format
/// choice is silently ignored — and nothing anywhere reports the conflict.
#[tokio::test]
async fn test_conflicting_output_formats_resolve_silently_to_the_first_initializer() {
    let _guard = GLOBAL_LOCK.lock().await;
    let _primary = primary_service().await; // json wins the process

    let pretty = TracingService::with_config(TracingConfig::new().pretty().log_level("info"));
    let cfg = TracingConfig::new().pretty().log_level("info");
    assert_eq!(cfg.log_format, LogFormat::Pretty);

    // Initialising a second formatter reports Ok without replacing global registry,
    // but the second handle cannot reload global filters.
    let res = pretty.init_subscriber(&cfg).await;
    assert!(res.is_ok());
    assert!(pretty.reload("debug").await.is_err());
}
