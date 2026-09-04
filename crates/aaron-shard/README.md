# Shard Service (`shard-service`)

Distributed partition management, shard allocation, worker frame transport, and deterministic data routing engine for the Aaron Node runtime.

---

## Overview

`shard-service` provides linearizable partition orchestration and distributed data sharding across cluster nodes. It separates cluster coordination (consensus) from data processing (workers) through a clean, dual-mode operational model:

1. **Coordinator Mode (`ShardService::coordinator(cp_handle)`)**:
   Runs on Control Plane nodes. Manages authoritative shard assignments, coordinates partition bootstrapping via the Raft state machine per service group (`service_name`), transparently proxies non-leader mutation requests to the active Raft leader, and pushes assignment commands to remote data workers over QUIC bi-directional streams.

2. **Worker Mode (`ShardService::worker()`)**:
   Runs on Data Plane worker nodes (e.g. storage workers). Listens for incoming QUIC assignment frames on port `18946` (configured via `SHARD_BIND_ADDR`), validates epochs, persists assignments locally to the embedded LSM store via FlatBuffers (`StoredShardPlacement`), publishes reactive `ShardEvent` lifecycle events (`Join`, `RoleChanged`, `Leave`) to `EventHub`, and transmits periodic telemetry heartbeats (`RaftMessage::TelemetryHeartbeat`) every 3 seconds to the Control Plane.

---

### Framework vs. User-Space Responsibilities in Sharding

The Aaron sharding architecture establishes strict, unambiguous boundaries:

- **Framework Responsibility (`ShardService`)**:
  - Deterministic partition routing (resolving keys to target partition IDs).
  - Cluster-wide topology coordination, partition allocation, epoch tracking, and RPC command dispatch via QUIC.
  - Emitting local lifecycle events (`ShardEvent::{Join, RoleChanged, Leave}`) onto `EventHub`.
- **User-Space Responsibility (Application Layer)**:
  - **Data Replication**: `shard-service` governs topology and placement, **never** data replication. The application chooses and executes its own replication protocol (e.g., partition WAL streaming, mini-Rafts, master-replica sync, or CRDTs).
  - **Role Determination & Feedback**: When application workers elect, promote, or demote partition roles, the user-space is responsible for reporting back to the Control Plane which node holds which role for that shard, keeping the authoritative cluster registry updated.

---

## The 4 Stages of Shard Implementation

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    Aaron Shard Architecture Roadmap                     │
├─────────────────────────────────────────────────────────────────────────┤
│ 1. Assignment                   │ Manual & One-Time Raft Bootstrap      │
│ 2. Leadership                   │ Monotonic Epoch Handover & Promotion │
│ 3. Removal & Eviction           │ Explicit Node Eviction without Chaos  │
│ 4. Deterministic Routing        │ Ring Hashing & [u16 BE] LSM Prefix    │
└─────────────────────────────────────────────────────────────────────────┘
```

### Stage 1: Shard Assignment — *Implemented*
- **Raft Bootstrap (`POST /api/shards/bootstrap`)**: Partition allocation persisted directly to the Raft consensus log per service name (`shards/{service_name}/{shard_id}` and `shards/{service_name}/system/bootstrapped`). Once bootstrapped for a given service, subsequent calls are rejected to preserve cluster consistency.
- **Manual Assignment (`POST /api/shards/assign`)**: Authoritative assignment of specific shards to designated Primary and Replica nodes.
- **Topology Rebalancing (`POST /api/shards/rebalance`)**: Calculates target distribution and reallocates shard replicas evenly across available alive worker nodes.
- **Multi-Service Shard Groups**: Shards are isolated and managed per service name (e.g., `orders`, `inventory`, or `default`), preventing partition interference between different business services.
- **Transparent Leader Proxying**: Coordinator endpoints transparently forward mutation and bootstrap requests to the active Raft leader when received by follower nodes.
- **QUIC Frame RPC**: Bidirectional QUIC streams dispatch `RaftMessage::ShardCommand` binary frames serialized with FlatBuffers (`schemas/control_plane.fbs`).
- **Reactive EventHub Pipeline**: Workers emit `ShardEvent::Join`, `ShardEvent::RoleChanged`, and `ShardEvent::Leave` onto the in-memory bus, allowing application services to react immediately.
- **On-Disk Persistence**: Partition records are persisted to the node's local LSM store using zero-copy FlatBuffers serialization (`StoredShardPlacement` in `schemas/shard.fbs`).
- **Periodic Worker Telemetry**: Workers measure dynamic Workload Performance Score (WPS) and error rate, transmitting `RaftMessage::TelemetryHeartbeat` every 3 seconds to the Control Plane over QUIC.

### Stage 2: Leadership Transitions — *Upcoming*
- Promotion of replicas to authoritative primaries per partition.
- Monotonic epoch counters ensuring split-brain resistance and sequential role handovers.
- Local partition lifecycle events (`ShardEvent::RoleChanged`).

### Stage 3: Shard Removal & Eviction — *Upcoming*
- Explicit node eviction from shard replica sets without automatic, uncoordinated rebalancing.

### Stage 4: Deterministic Routing & Big-Endian Prefixing
- In-memory deterministic WyHash 64-bit hashing (`Router`, `determine_shard`, `wyhash_64`) for key-to-partition mapping with full 64-bit avalanche protection and throughput up to ~38 GiB/s.
- 2-byte and 4-byte Big-Endian LSM key prefixing (`ShardKey::encode_u16`, `ShardKey::prefix_u16`) for partition isolation and contiguous range scans on disk.
- High-level route resolution in `ShardHandle::lookup_route` to find target partition, primary leader, and replicas.
- 5-digit zero-padded Raft keys (`shards/{service}/{shard_id:05}`) guaranteeing byte-level numerical sorting in Control Plane storage.

---

## Binary Schemas & Wire Protocol

### Storage Schema (`schemas/shard.fbs`)

On-disk shard placements are serialized into the embedded LSM store using FlatBuffers:

```flatbuffers
include "node.fbs";

namespace Aaron.Shard;

enum ShardStatus : uint8 {
  Healthy = 0,
  Degraded = 1,
  Unassigned = 2
}

table StoredShardPlacement {
  shard_id: uint32;
  primary: Aaron.Node.Uuid;
  replicas: [Aaron.Node.Uuid];
  status: ShardStatus;
  epoch: uint64;
  service_name: string;
}

root_type StoredShardPlacement;
```

### Network Command Schema (`schemas/control_plane.fbs`)

Inter-node shard assignment and role transitions over QUIC use binary commands:

```flatbuffers
table ShardCommand {
  shard_id: uint32;
  role: uint8; // 0 = Primary/Leader, 1 = Replica/Voter, 2 = Learner
  primary: Aaron.Node.Uuid;
  replicas: [Aaron.Node.Uuid];
  epoch: uint64;
  op_type: uint8; // 0 = AssignGroup, 1 = Promote, 2 = Demote, 3 = Leave
  target_role: uint8; // 0 = Learner, 1 = Voter, 2 = Leader
}

table ShardCommandResponse {
  success: bool;
  shard_id: uint32;
  current_role: uint8;
  term: uint64;
  reject_reason: uint8; // 0 = None, 1 = StaleEpoch, 2 = StaleTerm, 3 = NotAMember, 4 = Busy
}
```

---

## Usage Examples

### 1. Control Plane Coordinator Node (`orders`)

```rust
use aaron::{
    AdminService, ControlPlaneService, MembershipService, Node, ShardService, TracingService,
};

#[tokio::main]
async fn main() -> Result<(), aaron_core::BoxError> {
    let (membership_svc, membership_handle) = MembershipService::pair();
    let (control_svc, control_handle) = ControlPlaneService::pair();
    let (shard_svc, shard_handle) = ShardService::coordinator(control_handle.clone());

    Node::new("orders")
        .with_tag("role:control-plane")
        .with(membership_svc)
        .with(control_svc)
        .with(shard_svc)
        .with(
            AdminService::new()
                .with_membership_handle(membership_handle)
                .with_control_plane_handle(control_handle)
                .with_shard_handle(shard_handle),
        )
        .with(TracingService::new())
        .run()
        .await
}
```

### 2. Data Plane Worker Node (`inventory`)

```rust
use aaron::{Context, MembershipService, Node, ShardEvent, ShardService, TracingService, service_fn};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), aaron_core::BoxError> {
    let (membership_svc, _membership_handle) = MembershipService::pair();
    let (shard_svc, _shard_handle) = ShardService::worker();

    Node::new("inventory")
        .with_tag("role:worker")
        .with(membership_svc)
        .with(TracingService::new())
        .with(shard_svc)
        .with(service_fn("app-data-worker", |ctx: Context| async move {
            let mut sub = ctx.event_hub.subscribe::<ShardEvent>().await;

            while let Ok(event) = sub.recv().await {
                match event {
                    ShardEvent::Join { shard_id, members, role } => {
                        info!(
                            shard_id,
                            ?role,
                            member_count = members.len(),
                            "Partition assigned to local node. Initializing local storage..."
                        );
                    }
                    ShardEvent::RoleChanged { shard_id, role } => {
                        info!(shard_id, ?role, "Partition role transitioned.");
                    }
                    ShardEvent::Leave { shard_id } => {
                        info!(shard_id, "Partition decommissioned from local node.");
                    }
                    ShardEvent::Bootstrap { shards } => {
                        info!(count = shards.len(), "Cluster shards bootstrapped.");
                    }
                }
            }
            Ok(())
        }))
        .run()
        .await
}
```

### 3. User-Space Deterministic Routing & Big-Endian Storage

```rust
use aaron_core::{Context, KeyspaceExt, Store};
use aaron_shard::{ShardHandle, ShardKey};

async fn handle_user_write(
    shard_handle: &ShardHandle,
    store: &Store,
    account_id: &str,
    payload: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
    let raw_key = account_id.as_bytes();

    // 1. Resolve target partition and cluster placement
    if let Some(placement) = shard_handle.lookup_route("inventory", raw_key).await {
        println!(
            "Account {} mapped to Shard #{} (Primary Node: {:?})",
            account_id, placement.shard_id, placement.primary
        );

        // 2. Encode with 2-byte Big-Endian prefix for sequential LSM disk clustering
        let prefixed_key = ShardKey::encode_u16(placement.shard_id as u16, raw_key);
        let data_ks = store.keyspace("data")?;
        data_ks.insert(prefixed_key, payload)?;
    }
    Ok(())
}

async fn scan_shard_partition(
    store: &Store,
    shard_id: u16,
) -> Result<Vec<(String, Vec<u8>)>, Box<dyn std::error::Error>> {
    let data_ks = store.keyspace("data")?;

    // 3. Sequential Range Scan using Big-Endian prefix
    let prefix = ShardKey::prefix_u16(shard_id);
    let page = data_ks.scan_prefix(&prefix, None::<&[u8]>, 1000)?;

    let results = page
        .items
        .into_iter()
        .filter_map(|kv| {
            let (_shard, raw_key) = ShardKey::decode_u16(&kv.key)?;
            let key_str = String::from_utf8(raw_key.to_vec()).ok()?;
            Some((key_str, kv.value.to_vec()))
        })
        .collect();

    Ok(results)
}
```

---

## Admin REST APIs

| Endpoint | Method | Role | Description |
| :--- | :--- | :--- | :--- |
| `/api/shards` | `GET` | Coordinator | Returns active shard assignments, bootstrap status, and leader state |
| `/api/shards/bootstrap` | `POST` | Coordinator | Bootstraps partition distribution for a specified service (or default) across worker nodes |
| `/api/shards/rebalance` | `POST` | Coordinator | Rebalances partitions across currently active nodes |
| `/api/shards/assign` | `POST` | Coordinator | Manually assigns or updates a single partition allocation |

---

## Benchmarks

The official micro-benchmark suite for deterministic routing and Big-Endian prefixing uses Criterion:

```bash
cargo bench -p shard-service
```

### Comparative Hashing Benchmarks (FNV-1a vs. XXH64 vs. WyHash)

| Key Size | FNV-1a (Legacy) | XXH64 | WyHash (Aaron v2 Default) | WyHash Advantage |
| :--- | :--- | :--- | :--- | :--- |
| **8 bytes** | ~1.50 ns (4.93 GiB/s) | ~1.46 ns (5.07 GiB/s) | **~1.82 ns** (4.08 GiB/s) | Sub-2ns latency (~550M ops/s) |
| **32 bytes** | ~8.15 ns (3.65 GiB/s) | ~4.28 ns (6.96 GiB/s) | **~1.82 ns** (**16.30 GiB/s**) | **4.4x faster** (~547M ops/s) |
| **128 bytes** | ~62.58 ns (1.90 GiB/s) | ~9.03 ns (13.19 GiB/s) | **~4.15 ns** (**28.68 GiB/s**) | **15.0x faster** (~241M ops/s) |
| **1024 bytes** | ~717.34 ns (1.32 GiB/s) | ~51.74 ns (18.42 GiB/s) | **~24.69 ns** (**38.61 GiB/s**) | **29.0x faster** (~40.5M ops/s) |

### Routing & LSM Prefixing Benchmarks

| Operation | Scope / Configuration | Mean Latency | Throughput |
| :--- | :--- | :--- | :--- |
| `router.route` (WyHash) | 22-byte key | ~2.1 ns | **~475M routes/s** per core |
| `determine_shard` | 8, 64, and 1024 shards | ~2.1 ns (*constant O(1)*) | **~475M routes/s** per core |
| `ShardKey::encode_u16` | Big-Endian Prefix | ~7.94 ns | **~126M ops/s** |
| `ShardKey::decode_u16` | Zero-Copy Extraction | ~1.18 ns | **~844M ops/s** |
