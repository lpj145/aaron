# Aaron - Architectural Conventions, Directives & Project Memories

This document records the foundational architectural principles, design decisions, codebase conventions, and historical insights established for the Aaron project.

---

## 1. General Directives & Communication Guidelines
- **No emojis**: Keep communications, documentation, and commit messages clean, concise, and professional without emojis.
- **Incremental & Tested**: Implement changes step-by-step. Never dump large untested code blocks.
- **Fail-Fast & Resilient**: Software must panic/abort on illegal misconfiguration at startup, but recover gracefully and self-heal at runtime under network chaos or peer failure.
- **Strict Testing Discipline**: The integration and chaos test suites under `tests/` are strictly read-only specifications. Never modify test files to bypass failures or hide race conditions; all bug fixes and concurrency linearizations must happen exclusively in `src/`.

---

## 2. Core Architectural Philosophy of Aaron

### A. The `Service` Trait & Actor-Service Model
- Every capability in Aaron (Membership, Tracing, Storage, RPC, Pipelines) is an isolated, supervised `Service`.
- Services implement `Service` and declare their strongly-typed configuration via `ServiceConfig`.
- The `Node` acts as the supervisor host, managing the lifecycle, configuration validation, task spawning, and cancellation propagation.

### B. Decoupling via `EventHub` (In-Process Lockless Bus)
- Services **must not** couple tightly to each other or have direct references to internal tables of other services.
- Powered by [`crossfire 3`](https://crates.io/crates/crossfire) lockless bounded MPSC channels, delivering multi-million events/sec throughput with zero global lock contention across distinct event types.
- Inter-service communication and core node lifecycle management happen over `EventHub`:
  - `BindClusterIdCommand`: Published when cluster authority is resolved; `Node` listens and persists `cluster_id` into the `"node"` keyspace.
  - `StartServiceCommand`: Published to dynamically spawn new supervised instances of a registered service.
  - `ChangeLogLevel`: Dynamic runtime log level reloads for `TracingService`.
  - `MembershipEvent`: Published when peers join, become alive, suspect, dead, or leave.

### C. Registration vs. Execution
- **Registration (`Node::with` / `Node::with_opts`)**:
  - Each service name must be unique. Registering the same service name twice is strictly forbidden and panics/aborts immediately at setup time.
- **Execution (`StartServiceCommand`)**:
  - A registered service can have multiple concurrent running instances spawned dynamically at runtime by sending `StartServiceCommand` over `EventHub`.

---

## 3. Storage Engine Conventions (`Store` & LSM)
- Powered by embedded [Fjall 3.1](https://crates.io/crates/fjall) LSM-tree with zero external C/C++ dependencies.
- **Keyspace Namespacing**: Partitioned into dedicated keyspaces (`"node"`, `"default"`, `"membership"`, etc.).
- **Atomic Read-Modify-Write (RMW)**:
  - `KeyspaceExt::update` and `Store::update` use a striped key lock table (256 stripes) to guarantee linearizable atomic updates across real OS threads without lost updates.
- **Snapshots & Maintenance Mode**:
  - `Store` state is held in `Arc<RwLock<StoreState>>` and `Arc<AtomicBool>` (`maintenance`).
  - During `install_snapshot`, the store enters maintenance mode (`Store::is_maintenance()`), holding exclusive write state across the directory wipe and reopening.
  - Mutating operations (`set`, `remove`, `update`) explicitly reject writes with `StoreError::LockedForMaintenance` during snapshot installation instead of letting writes vanish into discarded temporary handles.
  - `Store::restore` cleans target directory remnants prior to unpacking snapshots.

---

## 4. Distributed Networking & Security (`Network` & QUIC)
- **Web of Trust P2P TLS**:
  - QUIC listeners present self-signed certificates with Subject Alternative Names (SAN) bound to the node's 128-bit `Uuid`.
  - `P2pServerCertVerifier` performs cryptographic signature checks and rejects any incoming/outgoing connection if the certificate does not match the target `NodeId` UUID (mitigating rogue/impostor MITM spoofing).
- **Connection Pooling**:
  - Outbound connections use atomic `get_or_insert` to eliminate race conditions under thundering herd connections, closing redundant handshakes immediately.

---

## 5. Cluster Membership Protocol (`membership-service` / SWIM)
- **Cluster Isolation**: Strict `cluster_id` validation. Any frame or ping from a foreign cluster is rejected at ingress before entering the table.
- **Incarnation Precedence & Self-Refutation**:
  - If a gossip message falsely claims the local node is `Suspect` or `Dead` with incarnation $\ge$ local incarnation, the local node automatically refutes with `local.incarnation = update.incarnation.saturating_add(1)`.
  - Arithmetic operations on incarnations must use `saturating_add` to prevent overflow on `u64::MAX`.
- **Indirect Probing (`PingReq`)**: If direct probe fails within `probe_timeout`, $k$ random alive peers are selected to probe the target.
- **Tombstones & Retention**: Dead member records are retained for 24h before being pruned by a slow GC loop (every 5 min).

---

## 6. Error Handling Architecture (OpenDAL + Snafu)
- **Unified `Error` & `ErrorKind`**:
  - Inspired by OpenDAL, all subsystem errors convert into `node::Error` providing programmatic `ErrorKind` matching, `.operation()` labeling, `.context(key, value)` key-value metadata, and complete `.source()` causal chains.
- **Domain-Specific Errors with `snafu 0.8`**:
  - `StoreError`: `LockedForMaintenance`, `KeyspaceNotFound`, `SnapshotSameDir`, `Fjall`, `Io`, `Utf8`.
  - `NetworkError`: `FrameTooLarge`, `UnexpectedDisconnect`, `AddressResolution`, `QuicConnect`, `QuicConnection`, `QuicWrite`, `QuicRead`, `Tls`, `Io`.
  - `ConfigError`: `MissingRequired`, `InvalidValue`, `Custom`.
  - `EventHubError`: `Disconnected`.
  - `MembershipError`: `NotRunning`, `ClusterMismatch`, `InvalidJoinResponse`, `ProbeTimeout`, `MalformedMessage`, `Node`, `Io`.
  - `TracingError`: `EmptyFilterDirective`, `InvalidFilterDirective`, `ReloadHandleNotInitialized`, `ReloadFailed`, `SubscriberInit`.

---

## 7. Testing Philosophy & Benchmarking Standards
- **Test Suites**:
  1. **Real OS Threads (`std::thread` + `Barrier`)**: For validating concurrency invariants, atomic RMW, and lock contention.
  2. **Chaos & Corrupted Frames**: Blasting malformed FlatBuffers, truncated frames, random binary noise, and SSRF attempts.
  3. **Fuzzing**: Boundary tests on UTF-8, multi-byte characters, UUID parsing, arithmetic extremes (`u64::MAX`), and degenerate floats (`NaN`, `+inf`).
  4. **Multi-Node Cluster Convergence**: Multi-node meshes verifying gossip convergence, crash-restart, and rejoin under higher incarnation.
- **Benchmarks**:
  - Criterion 0.8 microbenchmarks under `benches/` for measuring hot-path throughput, fanout scaling, and multi-producer contention.
