# Aaron

An opinionated, high-performance distributed systems runtime and actor-service framework in Rust, designed for resilient peer-to-peer (P2P) networking, embedded LSM-tree persistence, linearizable Raft consensus, lockless event-driven architecture, and supervised service lifecycles.

> **Origins & Vision**:
> Aaron represents the culmination of **5 years of research, architectural experiments, and distributed systems engineering**. Not 5 years of experience but a decade of trying to build system that really work's as expected and scale well. I've written many docs, hand papers, diagrams on many tools, a lot of loc's trying make this work's very closest to the vision that I have in mind, to be honest maybe this version is a 52 version of the project but work's and look's very closest to what I thought. Bringing this such system is very difficult for a solo developer and that's why I used Gemini to build and maintain those pieces, but I try hard to architect it and put strong pattern decisions, perfect? not even close, but if you have something please open issue or contact me.

---

## 1. The Opinionated Philosophy of Aaron

Aaron is built upon core architectural principles that dictate how distributed services should be composed, configured, and run:

1. **Decoupled, Composable Services (`Service` Trait)**:
   Every feature in Aaron (tracing, cluster membership, consensus, discovery, storage, RPC) is a first-class, standalone, supervised `Service`. The `Node` acts as a runtime host and supervisor.

2. **Fail-Fast Declarative Configuration (`ServiceConfig`)**:
   Services declare their environment variables, types, defaults, and descriptions explicitly. The node inspects and validates the entire configuration schema *before* initializing network listeners or disk storage. If a mandatory variable is missing or malformed, startup aborts immediately with actionable error reports.

3. **In-Memory Lockless Event Mesh (`EventHub`)**:
   Services communicate in-process using strongly-typed pub/sub events over lockless ring buffers ([`crossfire 3`](https://crates.io/crates/crossfire)), achieving multi-million events/sec throughput with zero global lock contention across distinct topics.

4. **Embedded Zero-External-Dependency Storage (`Store`)**:
   Every Aaron node comes with an embedded, ACID-compliant LSM-tree storage engine ([Fjall 3.1](https://crates.io/crates/fjall)) partitioned into isolated keyspace namespaces (`node`, `membership`, `control-plane`, `app`). Features striped atomic RMW locks and explicit maintenance mode during snapshot swaps.

5. **Linearizable Distributed Consensus (`OpenRaft 0.9`)**:
   Coordinated cluster metadata and linearizable state machine replication powered by OpenRaft, supporting dynamic joint consensus membership changes, leader elections, non-voting learners, and automatic follower-to-leader HTTP proxying.

6. **OpenDAL-Style Enriched Error Architecture (`Error`, `ErrorKind`, `snafu`)**:
   Strongly-typed error domains with context selectors powered by [`snafu 0.8`], unified into an OpenDAL-inspired `Error` struct providing structured `ErrorKind` classification, operation labeling, and key-value diagnostic context.

7. **Multiplexed Transport over QUIC (`Network`)**:
   All inter-node traffic travels over QUIC with native Web-of-Trust P2P TLS certificates authenticated against 128-bit node UUIDs, eliminating head-of-line blocking and connection handshake overhead.

8. **Erlang/OTP-Style Supervision Tree (`ServiceOpts`)**:
   Each service is isolated in its own task hierarchy with dedicated cancellation tokens, configurable restart policies (`Never`, `Always`, `OnFailure`, `MaxRetries`), and backoff strategies (`Constant`, `Linear`, `Exponential`).

---

## 2. Runtime Architecture & Management

### Comprehensive Multi-Service Runtime Architecture

<p align="center">
  <img src="./assets/aaron_full_architecture.jpg" alt="Aaron Runtime Architecture" width="100%" />
</p>

### Embedded Web Admin Dashboard & Live Topology

<p align="center">
  <img src="./assets/admin_panel_overview.png" alt="Aaron Admin Dashboard" width="100%" />
</p>

---

## 3. Expressive Cluster Composition in Rust

Spinning up a secure, supervised node with embedded storage, dynamic logging, SWIM gossip, Raft consensus, and an embedded Vue.js dashboard takes just a few lines of idiomatic Rust:

```rust
use aaron::{
    Node, Context, service_fn,
    TracingService, MembershipService, ControlPlaneService, AdminService
};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), node::Error> {
    // 1. Initialize services and operational handles
    let (membership_svc, membership_handle) = MembershipService::new_with_handle();
    let (control_plane_svc, cp_handle) = ControlPlaneService::new_with_handle();
    let tracing_svc = TracingService::new();

    let admin_svc = AdminService::new()
        .with_membership_handle(membership_handle.clone())
        .with_control_plane_handle(cp_handle.clone())
        .with_service_schema(&membership_svc)
        .with_service_schema(&control_plane_svc)
        .with_service_schema(&tracing_svc);

    // 2. Compose the Node runtime with supervised services
    let node = Node::new()
        .with(tracing_svc)
        .with(membership_svc)
        .with(control_plane_svc)
        .with(admin_svc);

    // 3. Connect to the cluster seed asynchronously
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let seed_addr = "127.0.0.1:7946".parse().unwrap();
        match membership_handle.join(seed_addr).await {
            Ok(peers) => println!("Joined cluster! Discovered {} peer(s)", peers.len()),
            Err(err) => eprintln!("Failed to join cluster: {err}"),
        }
    });

    // 4. Run the supervised node runtime
    node.run().await
}
```

---

## 4. Workspace Structure

```
aaron/
├── crates/
│   ├── node/                   # Core runtime host, Context, Supervision, Network, Store, EventHub, Error
│   ├── tracing-service/        # Structured JSON/Pretty logging with dynamic runtime level reload
│   ├── membership-service/     # SWIM cluster membership & failure detection over QUIC FlatBuffers
│   ├── control-plane-service/  # Linearizable OpenRaft 0.9 consensus engine backed by Fjall LSM store
│   └── admin-service/          # Supervised HTTP dashboard serving embedded Vue.js SPA & REST APIs
├── assets/                     # Architecture diagrams and UI preview screenshots
├── schemas/
│   └── membership.fbs          # FlatBuffers binary protocol definitions
└── Cargo.toml                  # Workspace manifest
```

### Crate Highlights

- **[`node`](./crates/node/README.md)**: Core runtime container providing `Node`, `Context`, `Service`, `ServiceConfig`, `EventHub`, `Network`, `Store`, and unified `Error`/`ErrorKind`.
- **[`membership-service`](./crates/membership-service/README.md)**: SWIM-based cluster membership, failure detection (Ping + PingReq), and gossip dissemination over QUIC with strict `cluster_id` authorization.
- **[`control-plane-service`](./crates/control-plane-service/README.md)**: OpenRaft 0.9 distributed consensus, linearizable key/value state machine, dynamic membership, and leader election.
- **[`admin-service`](./crates/admin-service/README.md)**: Embedded Vue.js 3 single-page application, Canvas 2D topology ring, and REST/SSE management interface.
- **[`tracing-service`](./crates/tracing-service/README.md)**: Dynamic observability service reacting to `ChangeLogLevel` events via `EventHub`.

---

## 5. Quick Start

### Running Examples

```bash
# 1. Minimal Worker Node
cargo run --example basic_node

# 2. Dynamic Log Level Reloading with TracingService
cargo run --example tracing_node

# 3. Cluster Membership with Admin Handle & SWIM Gossip
cargo run --example cluster_admin

# 4. Full Node with Embedded Vue.js Admin Dashboard & Raft Control Plane (http://127.0.0.1:8080)
cargo run --example admin_node
```

### Running Tests and Benchmarks

```bash
# Run all workspace unit, integration, chaos, and fuzz tests
cargo test --all-targets --release

# Run EventHub Criterion 0.8 benchmarks
cargo bench -p node --bench event_hub_bench

# Run linter
cargo clippy --all-targets --release
```

---

## 6. Documentation Index

- [Node Architecture & Service Development Guide](./crates/node/README.md)
- [SWIM Membership Service & Protocol Specification](./crates/membership-service/README.md)
- [Control Plane Consensus Service Guide](./crates/control-plane-service/README.md)
- [Admin Service & Vue.js Dashboard Guide](./crates/admin-service/README.md)
- [Tracing Service & Dynamic Reloading](./crates/tracing-service/README.md)
- [Architectural Conventions & Directives](./CONVENTIONS.md)
