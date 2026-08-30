# Aaron Node (`node`)

`node` is the core runtime container and service supervisor of the Aaron distributed framework. It orchestrates service lifecycles, configuration validation, embedded persistence, multi-transport networking, and in-memory event distribution.

---

## Architecture & Subsystems

When an Aaron Node boots, it initializes a shared [`Context`](./src/service/context.rs) that is injected into every supervised [`Service`](./src/service/service_trait.rs):

```
                   ┌──────────────────────────────────────────────┐
                   │                 Aaron Node                   │
                   └──────────────────────┬───────────────────────┘
                                          │
                  1. validate_env() (Fail-Fast Schema Check)
                  2. generate_env_example() / .env.example
                  3. Store::open() (Embedded LSM-Tree Fjall 3.1)
                  4. Assemble Context (Store, Network, EventHub, Identity, Env)
                                          │
                  ┌───────────────────────┼───────────────────────┐
                  ▼                       ▼                       ▼
        ┌───────────────────┐   ┌───────────────────┐   ┌───────────────────┐
        │   Supervised Svc  │   │   Supervised Svc  │   │   Supervised Svc  │
        │ (TracingService)  │   │(MembershipService)│   │  (App Business)   │
        │                   │   │                   │   │                   │
        │ service.run(ctx)  │   │ service.run(ctx)  │   │ service.run(ctx)  │
        └───────────────────┘   └───────────────────┘   └───────────────────┘
```

### The 5 Core Subsystems in `Context`

| Subsystem | Handle | Purpose |
| :--- | :--- | :--- |
| **Event Bus** | `ctx.event_hub` | Lockless in-memory pub/sub queues (`crossfire`) for decoupled service-to-service messaging. |
| **Network Manager** | `ctx.network` | Multi-transport networking: TCP, UDP, and QUIC with automatic P2P TLS connection pooling. |
| **Persistent Store** | `ctx.store` | Embedded LSM-tree storage engine ([Fjall 3.1](https://crates.io/crates/fjall)) partitioned by keyspaces. |
| **Node Identity** | `ctx.identity` | 128-bit cryptographic UUID and monotonic incarnation counter. |
| **Environment** | `ctx.env` | Detected host IPs, hostname, and environment variable access. |
| **Lifecycle Isolation** | `ctx.token` | Local `CancellationToken` for isolating this service instance. |
| **Node Shutdown** | `ctx.shutdown()` | Explicit trigger to initiate a node-wide graceful shutdown across all services. |

---

## How to Build a Service

Every service in Aaron implements the [`Service`](./src/service/service_trait.rs) trait:

```rust
pub trait Service: Send + Sync + 'static {
    type Config: ServiceConfig;

    fn name(&self) -> &str;
    fn run(&self, ctx: Context) -> impl Future<Output = Result<(), BoxError>> + Send;
}
```

### Step-by-Step: Creating a Production-Grade Service

#### 1. Define the Declarative Configuration (`ServiceConfig`)

Services declare their configuration fields explicitly. The supervisor validates this schema on boot before opening ports or databases:

```rust
use node::{BoxError, ConfigError, ConfigField, Context, Env, Node, Service, ServiceConfig, ServiceOpts};
use std::time::Duration;
use tracing::info;

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub poll_interval: Duration,
    pub batch_size: usize,
    pub worker_tag: String,
}

impl ServiceConfig for WorkerConfig {
    fn schema() -> Vec<ConfigField> {
        vec![
            ConfigField::new("WORKER_POLL_INTERVAL_MS", "u64")
                .default("1000")
                .description("Interval between background poll ticks in milliseconds"),
            ConfigField::new("WORKER_BATCH_SIZE", "usize")
                .default("50")
                .description("Maximum batch size processed per iteration"),
            ConfigField::new("WORKER_TAG", "String")
                .required()
                .description("Mandatory worker pool identifier tag"),
        ]
    }

    fn from_env(env: &Env) -> Result<Self, ConfigError> {
        let poll_ms = env.get::<u64>("WORKER_POLL_INTERVAL_MS").unwrap_or(1000);
        let batch_size = env.get::<usize>("WORKER_BATCH_SIZE").unwrap_or(50);
        let worker_tag = env.get::<String>("WORKER_TAG").ok_or_else(|| ConfigError::MissingRequired {
            service: "worker_service".to_string(),
            var_name: "WORKER_TAG".to_string(),
            description: "Mandatory worker pool identifier tag".to_string(),
        })?;

        Ok(Self {
            poll_interval: Duration::from_millis(poll_ms),
            batch_size,
            worker_tag,
        })
    }
}
```

#### 2. Implement the `Service` Logic

```rust
pub struct WorkerService {
    config_override: Option<WorkerConfig>,
}

impl WorkerService {
    pub fn new() -> Self {
        Self { config_override: None }
    }

    pub fn with_config(config: WorkerConfig) -> Self {
        Self { config_override: Some(config) }
    }
}

impl Service for WorkerService {
    type Config = WorkerConfig;

    fn name(&self) -> &str {
        "worker_service"
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        // Resolve configuration (override or environment)
        let config = match &self.config_override {
            Some(c) => c.clone(),
            None => WorkerConfig::from_env(&ctx.env)?,
        };

        info!(
            tag = %config.worker_tag,
            batch = config.batch_size,
            interval = ?config.poll_interval,
            "WorkerService started"
        );

        // Main supervised event loop: react to ctx.token for cancellation
        let mut interval = tokio::time::interval(config.poll_interval);
        loop {
            tokio::select! {
                _ = ctx.token.cancelled() => {
                    info!("WorkerService received cancellation signal, flushing buffers...");
                    break;
                }
                _ = interval.tick() => {
                    // Do work here
                }
            }
        }

        Ok(())
    }
}
```

#### 3. Register and Run with the `Node`

```rust
#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let node = Node::new()
        .with_dir_path("./data")
        .with_opts(
            WorkerService::new(),
            ServiceOpts::new()
                .restart_on_failure()
                .exponential_backoff(Duration::from_secs(1), Duration::from_secs(30), 2.0),
        );

    // Auto-generate .env.example with documentation for all registered services
    node.write_env_example(".env.example")?;

    // Validates schemas (Fail-Fast) and runs supervised services
    node.run().await
}
```

---

## Lightweight Anonymous Services (`service_fn`)

For quick workers, one-off background tasks, or testing, you can use `service_fn` instead of defining a dedicated struct:

```rust
use node::{service_fn, Context, Node};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), node::BoxError> {
    Node::new()
        .with(service_fn("heartbeat", |ctx: Context| async move {
            info!("Heartbeat worker running on node {}", ctx.identity.id());
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            ctx.shutdown(); // Request clean shutdown
            Ok(())
        }))
        .run()
        .await
}
```

---

## Supervision & Fault Tolerance (`ServiceOpts`)

Aaron implements an Erlang/OTP-style supervisor:

### Restart Policies (`RestartPolicy`)

- `RestartPolicy::Never`: Execute once. If the service finishes or errors, do not restart (default).
- `RestartPolicy::Always`: Always restart when the service exits (ideal for persistent daemons).
- `RestartPolicy::OnFailure`: Restart only if `run()` returns `Err` or panics.
- `RestartPolicy::MaxRetries(n)`: Restart up to $n$ times.
- `RestartPolicy::OnFailureMaxRetries(n)`: Restart on failure up to $n$ times.

### Backoff Strategies (`BackoffStrategy`)

- `BackoffStrategy::None`: Restart immediately with 0 delay.
- `BackoffStrategy::Constant(Duration)`: Fixed delay between retry attempts.
- `BackoffStrategy::Linear { initial, step, max }`: Linearly increasing delay: `initial + (step * retry_count)`.
- `BackoffStrategy::Exponential { initial, multiplier, max }`: Exponential backoff: `initial * multiplier^retry_count`.
