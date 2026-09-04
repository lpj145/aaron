# Tracing Service (`tracing-service`)

The `tracing-service` is a managed service for the **Aaron Node** runtime responsible for structured logging, observability, and dynamic telemetry configuration.

It provides:
1. **Structured Log Formatting**: Native support for `json` formatting (optimized for production/log collectors) and `pretty` formatting (human-readable, colored output for development).
2. **Declarative Configuration Schema**: Fail-fast environment variable validation (`LOG_LEVEL`, `LOG_FORMAT`) and automated `.env.example` template generation.
3. **Dynamic Hot-Reloading**: Ability to change log level filtering directives on-the-fly without restarting the process by publishing [`ChangeLogLevel`](./src/event.rs) events to the node's [`EventHub`](../node/src/event_hub/mod.rs).
4. **Graceful & Cooperative Shutdown**: Continuous monitoring of runtime cancellation signals via `ctx.token`.

---

## Quick Start

### Registering in `Node`

Add `TracingService` to your node pipeline:

```rust
use aaron_core::Node;
use aaron_tracing::TracingService;

#[tokio::main]
async fn main() -> Result<(), aaron_core::BoxError> {
    Node::new()
        .with(TracingService::new())
        .run()
        .await
}
```

---

## Environment Variables & Declarative Schema

The service implements the [`ServiceConfig`](../node/src/service/config.rs) trait, exposing its schema to the node runtime:

| Variable | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `LOG_LEVEL` | `String` | `"info"` | `EnvFilter` directive (e.g., `info`, `debug`, `trace`, `node=debug,tracing_service=trace`). |
| `LOG_FORMAT` | `String` | `"json"` | Output format: `"json"` (default) or `"pretty"`. |

### Example generated in `.env.example`
```ini
# === [tracing-service] ===
# Initial tracing log level filter directive (e.g. info, debug, trace, crate=debug)
LOG_LEVEL=info

# Log output format: 'json' (default) or 'pretty'
LOG_FORMAT=json
```

---

## Dynamic Hot-Reloading at Runtime

Any service registered within the node can dispatch a [`ChangeLogLevel`](./src/event.rs) event through the shared `EventHub` in `Context`. The `TracingService` intercepts this event and reconfigures the subscriber's active filter layer dynamically:

```rust
use aaron_core::{service_fn, Context, Node};
use tracing::{debug, info};
use aaron_tracing::{ChangeLogLevel, TracingService};

let node = Node::new()
    .with(TracingService::new())
    .with(service_fn("worker", |ctx: Context| async move {
        // Initially, only 'info' and higher are emitted
        info!("Running background routine with default log level (info)...");
        debug!("This debug message is NOT visible yet");

        // Publish event to lower log level to DEBUG at runtime
        ctx.event_hub.publish(ChangeLogLevel::debug()).await;

        // Debug log messages are now visible immediately
        debug!("DEBUG log message is now visible after dynamic reload!");

        // Available convenience helpers:
        // ChangeLogLevel::trace()
        // ChangeLogLevel::debug()
        // ChangeLogLevel::info()
        // ChangeLogLevel::warn()
        // ChangeLogLevel::error()
        // ChangeLogLevel::new("my_crate=trace,tokio=warn")

        Ok(())
    }));
```

---

## Programmatic Configuration

If you prefer configuring the service without environment variables:

```rust
use aaron_tracing::{LogFormat, TracingConfig, TracingService};

let config = TracingConfig::new()
    .pretty()
    .log_level("debug");

let service = TracingService::with_config(config);
```

---

## Running Tests

The test suite is located in `tests/tracing_service.rs`:

```bash
cargo test -p tracing-service
```
