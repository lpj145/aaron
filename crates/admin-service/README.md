# Admin Service (`admin-service`)

A supervised HTTP management service for the Aaron distributed runtime that embeds and serves a high-performance Vue.js 3 single-page application (SPA) alongside REST and Server-Sent Events (SSE) administration APIs.

---

## 1. Overview & Capabilities

- **Zero-External-Asset Runtime**: Compiles and embeds the Vue.js 3 SPA directly into the Rust binary with `rust-embed`. Aaron nodes can serve the complete visual dashboard without requiring external frontend servers or static directories.
- **SWIM Cluster Topology Management**: Real-time visual monitoring of cluster members (Alive, Suspect, Dead, Left), join seed connections, and graceful leave triggers.
- **LSM-Tree Key-Value Explorer (Fjall 3.1)**: Browse partitioned keyspaces (`"node"`, `"membership"`, `"app"`), scan with prefix filtering, inspect formatted JSON and binary hex payloads, insert/update keys, and delete entries.
- **Dynamic Log Filter Reloading**: Apply new `EnvFilter` tracing directives dynamically on-the-fly (via `EventHub`), with real-time log and event streaming over Server-Sent Events (SSE).
- **Supervised Services Introspection**: Inspect registered services, declared configuration schemas (`ServiceConfig`), expected types, defaults, and currently resolved environment variables.
- **Environment & Secret Detection**: Inspect active environment variables with automated masking of sensitive secrets (tokens, keys, passwords).

---

## 2. Configuration (`AdminConfig`)

Declared configuration variables validated before startup:

| Environment Variable | Type | Default | Description |
|----------------------|------|---------|-------------|
| `ADMIN_BIND_ADDR` | `SocketAddr` | `127.0.0.1:8080` | HTTP address and port for the dashboard and REST API |
| `ADMIN_ENABLED` | `bool` | `true` | Enables or disables the HTTP admin dashboard service |
| `ADMIN_STATIC_DIR` | `String` | `None` | Optional filesystem directory for external static frontend assets |

---

## 3. REST & Streaming API Reference

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/health` | Health check endpoint returning node status and uptime |
| `GET` | `/api/node` | Local node identity, hardware, and network interfaces |
| `GET` | `/api/cluster` | Cluster authority ID, local member, and member topology list |
| `POST` | `/api/cluster/join` | Connect to a cluster seed (`{ "seed": "127.0.0.1:17946" }`) |
| `POST` | `/api/cluster/leave` | Gracefully broadcast voluntary `Left` status |
| `GET` | `/api/services` | Supervised services and their `ServiceConfig` schemas |
| `GET` | `/api/store` | LSM store filesystem path, keyspaces, and maintenance status |
| `POST` | `/api/store/keyspaces` | Initialize a new keyspace (`{ "name": "app" }`) |
| `GET` | `/api/store/:keyspace/scan` | Paginated prefix scan (`?prefix=...&limit=50`) |
| `GET` | `/api/store/:keyspace/get` | Retrieve key value (`?key=...`) |
| `POST` | `/api/store/:keyspace/set` | Insert or update key value (`{ "key": "...", "value": "..." }`) |
| `DELETE` | `/api/store/:keyspace/delete` | Delete key (`?key=...`) |
| `GET` | `/api/tracing` | Retrieve active log level filter directive |
| `POST` | `/api/tracing/level` | Dynamically update log filter (`{ "filter": "debug" }`) |
| `GET` | `/api/env` | Active environment variables with secret masking |
| `GET` | `/api/events/stream` | Real-time Server-Sent Events (SSE) pub/sub stream |

---

## 4. Usage Example

```rust
use aaron::{AdminService, MembershipService, TracingService, Node};

#[tokio::main]
async fn main() -> Result<(), node::Error> {
    let (membership, handle) = MembershipService::pair();
    let tracing = TracingService::new();

    let admin = AdminService::new()
        .with_membership_handle(handle)
        .with_service_schema(&membership)
        .with_service_schema(&tracing);

    Node::new()
        .with(tracing)
        .with(membership)
        .with(admin)
        .run()
        .await
        .map_err(Into::into)
}
```
