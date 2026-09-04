# Shard Service (`shard-service`)

Distributed partition management, shard allocation, worker frame transport, and deterministic data routing engine for the Aaron Node runtime.

---

## Overview

`shard-service` provides linearizable partition orchestration and distributed data sharding across cluster nodes. It separates cluster coordination (consensus) from data processing (workers) through a clean, dual-mode operational model:

1. **Coordinator Mode (`ShardService::coordinator(cp_handle)`)**:
   Runs on Control Plane nodes. Manages authoritative shard assignments, coordinates partition bootstrapping via the Raft state machine per service group (`service_name`), transparently proxies non-leader mutation requests to the active Raft leader, and pushes assignment commands to remote data workers over QUIC bi-directional streams.

2. **Worker Mode (`ShardService::worker()`)**:
   Runs on Data Plane worker nodes (e.g. `treasurer`). Listens for incoming QUIC assignment frames on port `18946` (configured via `SHARD_BIND_ADDR`), validates epochs, persists assignments locally to the embedded LSM store via FlatBuffers (`StoredShardPlacement`), publishes reactive `ShardEvent` lifecycle events (`Join`, `RoleChanged`, `Leave`) to `EventHub`, and transmits periodic telemetry heartbeats (`RaftMessage::TelemetryHeartbeat`) every 3 seconds to the Control Plane.

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
│ 1. Designação (Assignment)      │ Manual & One-Time Raft Bootstrap      │
│ 2. Liderança (Leadership)       │ Monotonic Epoch Handover & Promotion │
│ 3. Remoção (Removal/Eviction)   │ Explicit Node Eviction without Chaos  │
│ 4. Consulta por Ordem (Routing) │ Ring Hashing & [u16 BE] LSM Prefix    │
└─────────────────────────────────────────────────────────────────────────┘
```

### Stage 1: Designação (Assignment) — *Implemented*
- **Raft Bootstrap (`POST /api/shards/bootstrap`)**: Partition allocation persisted directly to the Raft consensus log per service name (`shards/{service_name}/{shard_id}` and `shards/{service_name}/system/bootstrapped`). Once bootstrapped for a given service, subsequent calls are rejected to preserve cluster consistency.
- **Manual Assignment (`POST /api/shards/assign`)**: Authoritative assignment of specific shards to designated Primary and Replica nodes.
- **Topology Rebalancing (`POST /api/shards/rebalance`)**: Calculates target distribution and reallocates shard replicas evenly across available alive worker nodes.
- **Multi-Service Shard Groups**: Shards are isolated and managed per service name (e.g., `treasurer`, `bank`, or `default`), preventing partition interference between different business services.
- **Transparent Leader Proxying**: Coordinator endpoints transparently forward mutation and bootstrap requests to the active Raft leader when received by follower nodes.
- **QUIC Frame RPC**: Bidirectional QUIC streams dispatch `RaftMessage::ShardCommand` binary frames serialized with FlatBuffers (`schemas/control_plane.fbs`).
- **Reactive EventHub Pipeline**: Workers emit `ShardEvent::Join`, `ShardEvent::RoleChanged`, and `ShardEvent::Leave` onto the in-memory bus, allowing application services to react immediately.
- **On-Disk Persistence**: Partition records are persisted to the node's local LSM store using zero-copy FlatBuffers serialization (`StoredShardPlacement` in `schemas/shard.fbs`).
- **Periodic Worker Telemetry**: Workers measure dynamic Workload Performance Score (WPS) and error rate, transmitting `RaftMessage::TelemetryHeartbeat` every 3 seconds to the Control Plane over QUIC.

### Stage 2: Liderança (Leadership) — *Upcoming*
- Promotion of replicas to authoritative primaries per partition.
- Monotonic epoch counters ensuring split-brain resistance and sequential role handovers.
- Local partition lifecycle events (`ShardEvent::RoleChanged`).

### Stage 3: Remoção (Removal / Eviction) — *Upcoming*
- Explicit node eviction from shard replica sets without automatic, uncoordinated rebalancing.

### Stage 4: Consulta por Ordem e Prefixos Big-Endian (Routing & Lookups)
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

### 1. Control Plane Coordinator Node (`bank`)

```rust
use aaron::{
    AdminService, ControlPlaneService, MembershipService, Node, ShardService, TracingService,
};

#[tokio::main]
async fn main() -> Result<(), node::BoxError> {
    let (membership_svc, membership_handle) = MembershipService::pair();
    let (control_svc, control_handle) = ControlPlaneService::pair();
    let (shard_svc, shard_handle) = ShardService::coordinator(control_handle.clone());

    Node::new("bank")
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

### 2. Data Plane Worker Node (`treasurer`)

```rust
use aaron::{Context, MembershipService, Node, ShardEvent, ShardService, TracingService, service_fn};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), node::BoxError> {
    let (membership_svc, _membership_handle) = MembershipService::pair();
    let (shard_svc, _shard_handle) = ShardService::worker();

    Node::new("treasurer")
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
use node::{Context, KeyspaceExt, Store};
use shard_service::{ShardHandle, ShardKey};

async fn handle_user_write(
    shard_handle: &ShardHandle,
    store: &Store,
    account_id: &str,
    payload: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let raw_key = account_id.as_bytes();

    // 1. Resolve target partition and cluster placement
    if let Some(placement) = shard_handle.lookup_route("treasurer", raw_key).await {
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

A suite oficial de micro-benchmarks do roteamento determinístico e prefixação Big-Endian utiliza Criterion:

```bash
cargo bench -p shard-service
```

### Resultados Comparativos de Hashing (FNV-1a vs XXH64 vs WyHash)

| Tamanho da Chave | FNV-1a (Legacy) | XXH64 | WyHash (Padrão Aaron v2) | Vantagem WyHash |
| :--- | :--- | :--- | :--- | :--- |
| **8 bytes** | ~1.50 ns (4.93 GiB/s) | ~1.46 ns (5.07 GiB/s) | **~1.82 ns** (4.08 GiB/s) | Latência sub-2ns (~550M ops/s) |
| **32 bytes** | ~8.15 ns (3.65 GiB/s) | ~4.28 ns (6.96 GiB/s) | **~1.82 ns** (**16.30 GiB/s**) | **4.4x mais rápido** (~547M ops/s) |
| **128 bytes** | ~62.58 ns (1.90 GiB/s) | ~9.03 ns (13.19 GiB/s) | **~4.15 ns** (**28.68 GiB/s**) | **15.0x mais rápido** (~241M ops/s) |
| **1024 bytes** | ~717.34 ns (1.32 GiB/s) | ~51.74 ns (18.42 GiB/s) | **~24.69 ns** (**38.61 GiB/s**) | **29.0x mais rápido** (~40.5M ops/s) |

### Resultados de Roteamento e LSM Prefix

| Operação | Escopo / Configuração | Latência Média | Throughput / Vazão |
| :--- | :--- | :--- | :--- |
| `router.route` (WyHash) | Chave de 22 bytes | ~2.1 ns | **~475M rotas/s** por core |
| `determine_shard` | 8, 64 e 1024 shards | ~2.1 ns (*constante O(1)*) | **~475M rotas/s** por core |
| `ShardKey::encode_u16` | Prefixo Big-Endian | ~7.94 ns | **~126M ops/s** |
| `ShardKey::decode_u16` | Extração Zero-Copy | ~1.18 ns | **~844M ops/s** |
