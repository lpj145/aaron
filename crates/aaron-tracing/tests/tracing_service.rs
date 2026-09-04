use aaron_core::{Context, Env, Node, Service, ServiceConfig, service_fn};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use aaron_tracing::{ChangeLogLevel, LogFormat, TracingConfig, TracingService};

#[test]
fn test_tracing_config_parsing_and_defaults() {
    let env = Env::detect();

    // 1. Defaults
    let cfg = TracingConfig::from_env(&env).unwrap();
    assert_eq!(cfg.log_level, "info");
    assert_eq!(cfg.log_format, LogFormat::Json);

    // 2. Custom values
    env.set("LOG_LEVEL", "debug").unwrap();
    env.set("LOG_FORMAT", "pretty").unwrap();

    let cfg_custom = TracingConfig::from_env(&env).unwrap();
    assert_eq!(cfg_custom.log_level, "debug");
    assert_eq!(cfg_custom.log_format, LogFormat::Pretty);

    // 3. Invalid format
    env.set("LOG_FORMAT", "invalid_format_name").unwrap();
    let err = TracingConfig::from_env(&env);
    assert!(err.is_err());
}

#[test]
fn test_tracing_service_schema_and_env_example() {
    let schema = TracingConfig::schema();
    assert_eq!(schema.len(), 2);
    assert_eq!(schema[0].name, "LOG_LEVEL");
    assert_eq!(schema[1].name, "LOG_FORMAT");

    let node = Node::new("tracing-node").with(TracingService::new());
    let example = node.generate_env_example();
    assert!(example.contains("# === [tracing-service] ==="));
    assert!(example.contains("LOG_LEVEL=info"));
    assert!(example.contains("LOG_FORMAT=json"));
}

#[tokio::test]
async fn test_tracing_service_dynamic_reload_via_event_hub() {
    let temp_dir = std::env::temp_dir().join(format!("test_tracing_reload_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let reloaded = Arc::new(AtomicBool::new(false));
    let reloaded_clone = reloaded.clone();

    let cancel_token = tokio_util::sync::CancellationToken::new();
    let cancel_token_clone = cancel_token.clone();

    let node = Node::new("tracing-node")
        .with_dir_path(&temp_dir)
        .with_cancel_token(cancel_token)
        .with(TracingService::new())
        .with(service_fn("log-producer", move |ctx: Context| {
            let reloaded_inner = reloaded_clone.clone();
            let cancel = cancel_token_clone.clone();
            async move {
                // Wait for TracingService to initialize and subscribe
                tokio::time::sleep(Duration::from_millis(50)).await;

                // Publish log level change event to "debug"
                let delivered = ctx.event_hub.publish(ChangeLogLevel::debug()).await;
                assert!(
                    delivered >= 1,
                    "ChangeLogLevel should be delivered to TracingService"
                );

                tokio::time::sleep(Duration::from_millis(20)).await;

                // Publish log level change event to "trace"
                let delivered2 = ctx.event_hub.publish(ChangeLogLevel::trace()).await;
                assert!(delivered2 >= 1);

                reloaded_inner.store(true, Ordering::SeqCst);
                cancel.cancel();
                Ok(())
            }
        }));

    node.run().await.unwrap();

    assert!(
        reloaded.load(Ordering::SeqCst),
        "Log producer successfully published reload events"
    );
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_tracing_service_pretty_and_json_initialization() {
    let svc_json = TracingService::with_config(TracingConfig::new().json().log_level("info"));
    let svc_pretty = TracingService::with_config(TracingConfig::new().pretty().log_level("debug"));

    assert_eq!(svc_json.name(), "tracing-service");
    assert_eq!(svc_pretty.name(), "tracing-service");

    // Test direct reload helper
    svc_json
        .init_subscriber(&TracingConfig::new().json())
        .await
        .unwrap();
    let res = svc_json.reload("trace").await;
    assert!(res.is_ok());
}
