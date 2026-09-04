# Aaron: High-Performance Distributed Actor & Consensus Framework

[![crates.io](https://img.shields.io/crates/v/aaron.svg)](https://crates.io/crates/aaron)
[![docs.rs](https://docs.rs/aaron/badge.svg)](https://docs.rs/aaron)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

`aaron` is the umbrella facade crate for the Aaron distributed actor and consensus framework. It aggregates all modular subsystems behind granular Cargo feature flags, allowing users to import only what they need or pull in the complete runtime with a single dependency.

---

## Features

By default, `aaron` enables the `full` feature suite. You can disable default features and opt in to specific capabilities:

```toml
[dependencies]
# Full framework (all services enabled)
aaron = "0.1.0"

# Minimal: core runtime only (Node, Context, Supervision, Network, Store, EventHub)
aaron = { version = "0.1.0", default-features = false }

# Core + Membership & Tracing only
aaron = { version = "0.1.0", default-features = false, features = ["membership", "tracing"] }
```

### Feature Matrix

| Feature | Subsystem | Description |
| :--- | :--- | :--- |
| *(default)* | [`aaron-core`](../aaron-core) | Core runtime, supervision tree, event hub, LSM storage, hardware benchmark, error types (unconditionally included). |
| `tracing` | [`aaron-tracing`](../aaron-tracing) | Dynamic log level filtering and distributed telemetry without process restarts. |
| `membership` | [`aaron-membership`](../aaron-membership) | SWIM gossip membership protocol over QUIC FlatBuffers with failure detection. |
| `control-plane` | [`aaron-control-plane`](../aaron-control-plane) | OpenRaft 0.9 linearizable consensus engine and metadata state machine. |
| `shard` | [`aaron-shard`](../aaron-shard) | Virtual partition management, WyHash routing, and LSM prefix key partitioning. |
| `admin` | [`aaron-admin`](../aaron-admin) | Embedded Vue.js administration dashboard and REST/SSE management APIs. |
| `full` | *All Above* | Activates all features: `tracing`, `membership`, `control-plane`, `shard`, and `admin`. |

---

## Quick Example: Minimal Worker Node

```rust
use aaron::{service_fn, Context, Node};

#[tokio::main]
async fn main() -> Result<(), aaron::BoxError> {
    Node::new("worker-node")
        .with(service_fn("worker", |ctx: Context| async move {
            println!("Node started with ID: {}", ctx.identity.id());
            Ok(())
        }))
        .run()
        .await?;

    Ok(())
}
```

---

## Quick Example: Cluster Node with Embedded Admin Console

```rust
use std::time::Duration;
use aaron::{
    admin::{AdminConfig, AdminService},
    membership::{MembershipConfig, MembershipService},
    tracing::TracingService,
    Context, Node, Uuid, service_fn,
};

#[tokio::main]
async fn main() -> Result<(), aaron::BoxError> {
    let cluster_id = Uuid::new(0x1234_5678, 0x9ABC_DEF0);

    let mem_config = MembershipConfig {
        bind_addr: "127.0.0.1:17946".to_string(),
        seeds: vec![],
        cluster_id: Some(cluster_id),
        probe_interval: Duration::from_millis(500),
        probe_timeout: Duration::from_millis(150),
        suspect_timeout: Duration::from_millis(1000),
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    };

    let (membership, handle) = MembershipService::pair_with_config(mem_config);

    let admin_config = AdminConfig {
        bind_addr: "127.0.0.1:8080".parse().unwrap(),
        enabled: true,
        static_dir: None,
    };

    let admin_svc = AdminService::with_config(admin_config)
        .with_membership_handle(handle);

    Node::new("cluster-node")
        .with(TracingService::new())
        .with(membership)
        .with(admin_svc)
        .run()
        .await?;

    Ok(())
}
```

---

## Workspace Crates

For granular microservice or embedded architectures, individual crates can be consumed directly:

- [`aaron-core`](../aaron-core): Fundamental runtime abstractions, lifecycle, and embedded LSM storage.
- [`aaron-tracing`](../aaron-tracing): Observability with dynamic filter reloading.
- [`aaron-membership`](../aaron-membership): High-availability cluster membership via SWIM protocol.
- [`aaron-control-plane`](../aaron-control-plane): Distributed Raft consensus state machine.
- [`aaron-shard`](../aaron-shard): Partition placement and hash routing.
- [`aaron-admin`](../aaron-admin): Embedded administrative UI and REST APIs.
- [`aaron-build`](../aaron-build): Build-time FlatBuffers compiler utility for custom schemas.

---

## License

Licensed under the Apache License, Version 2.0 ([LICENSE](https://www.apache.org/licenses/LICENSE-2.0)).
