# membership-service

SWIM-based cluster membership, failure detection, and gossip dissemination service for the Aaron Node framework, operating over QUIC with P2P Web-of-Trust TLS and FlatBuffers binary protocol.

## Architecture

`membership-service` organizes cluster membership into decoupled, modular components:

```
crates/aaron-membership/src/
├── config.rs          # ServiceConfig implementation with LAN/WAN presets and Cluster ID validation
├── event.rs           # EventHub lifecycle events (MembershipEvent enum) and commands (JoinClusterCommand)
├── handle.rs          # Thread-safe in-process query handle (MembershipHandle)
├── member.rs          # Member domain model and MemberStatus enum
├── message.rs         # Strongly-typed FlatBuffers message conversions
├── proto.rs           # Planus-generated FlatBuffers declarations
├── service.rs         # Supervised Service lifecycle implementation
├── table.rs           # Thread-safe MembershipTable with SWIM conflict resolution
└── stage/
    ├── egress.rs      # Outbound QUIC connection & stream transport client
    ├── ingress.rs     # Inbound QUIC stream handler (Ping, PingReq, JoinRequest gatekeeper)
    └── probe.rs       # Periodic Failure Detector loop (Direct Ping + Indirect PingReq)
```

## Features

- **Transport over QUIC**: Multiplexed bi-directional streams over QUIC with zero head-of-line blocking and TLS authenticated against node UUIDs.
- **FlatBuffers Serialization**: Zero-copy binary schemas compiled with `planus` (`schemas/membership.fbs`).
- **SWIM Failure Detection**: Direct `Ping` probes backed by indirect `PingReq` across $k$ random intermediaries on probe timeout.
- **Incarnation Conflict Resolution**: Strictly enforces incarnation ordering, status precedence (`Alive` < `Suspect` < `Dead`/`Left`), and automatic refutation of false suspicions against the local node.
- **Cluster Authorization & Security Token**: Enforces strict `cluster_id` validation at the gatekeeper (`JoinRequest`) to reject rogue/foreign nodes.
- **Capability Tags & Hostname Propagation**: Automatically propagates `service:<name>`, `host:<hostname>`, functional roles (`control-plane`, `shard-worker`), and custom tags via gossip without polluting the mesh with internal engine plumbing.
- **Direct Query Handle (`MembershipHandle`)**: Provides sub-microsecond in-memory topology lookups for other services (Admin, RPC, HTTP).
- **Dynamic Join Commands**: Dispatches and handles dynamic cluster join commands (`JoinClusterCommand`) via `EventHub` at runtime.

## Configuration

| Environment Variable | Type | Default | Description |
|----------------------|------|---------|-------------|
| `MEMBERSHIP_BIND_ADDR` | `String` | `"0.0.0.0:7946"` | QUIC listen socket address |
| `MEMBERSHIP_SEEDS` | `String` | `""` | Comma-separated list of seed node socket addresses |
| `MEMBERSHIP_CLUSTER_ID` | `String` | `""` | 128-bit Cluster ID UUID token (required for joiner nodes) |
| `AARON_TAGS` | `String` | `""` | Optional comma-separated custom tags (e.g. `zone:us-east-1,tier:worker`) |

### Cluster Authorization Policy

- **Bootstrap Node (`MEMBERSHIP_SEEDS=""`)**: Initializes the cluster. If `MEMBERSHIP_CLUSTER_ID` is not specified, generates a cryptographically random UUID and acts as the cluster authority.
- **Joining Nodes (`MEMBERSHIP_SEEDS="ip:port,..."`)**: **Must** be provisioned with `MEMBERSHIP_CLUSTER_ID`. Aborts startup immediately (Fail-Fast) if omitted.
- **Join Handshake**: Incoming `JoinRequest` is validated against the local cluster authority. Mismatched cluster tokens are rejected at the gate.

### Protocol Timings (LAN Profile)

| Parameter | LAN Preset | WAN Preset | Description |
|-----------|------------|------------|-------------|
| `probe_interval` | `1000ms` | `5000ms` | Interval between failure detector probe ticks |
| `probe_timeout` | `200ms` | `1000ms` | Timeout before considering direct probe failed and launching indirect probes |
| `suspect_timeout` | `1000ms` | `10000ms` | Window before transitioning a `Suspect` node to `Dead` |
| `indirect_ping_targets` | `3` | `3` | Number of intermediary nodes contacted during `PingReq` |
| `gossip_fanout` | `3` | `4` | Number of member updates piggybacked on each message |

## Usage Examples

### 1. Registering the Service with `MembershipHandle` and Custom Tags

```rust
use std::time::Duration;
use aaron_membership::{MembershipConfig, MembershipService};
use aaron_core::{service_fn, Context, Node, Uuid};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), aaron_core::BoxError> {
    let config = MembershipConfig::lan()
        .bind_addr("127.0.0.1:7946")
        .cluster_id(Uuid::random());

    // 1. Create paired service and query handle
    let (membership, handle) = MembershipService::pair_with_config(config);

    // 2. Register with Node with explicit service identity and tags
    Node::new("worker")
        .with_tag("role:worker")
        .with_tag("tier:data-plane")
        .with(membership)
        .with(service_fn("admin", move |ctx: Context| {
            let handle = handle.clone();
            async move {
                handle.wait_ready().await;

                // Query topology directly from in-memory table
                let active = handle.active_members().await;
                info!("Cluster size: {}", active.len());

                for member in &active {
                    info!("Node {} (host: {:?}) tags: {:?}", member.node_id.id(), member.hostname(), member.tags);
                }

                let cluster_id = handle.cluster_id().await;
                info!("Active Cluster ID: {:?}", cluster_id);

                Ok(())
            }
        }))
        .run()
        .await
}
```

### 2. Subscribing to Topology Events via `EventHub`

```rust
use aaron_core::EventHub;
use aaron_membership::MembershipEvent;

async fn watch_cluster(event_hub: &EventHub) {
    let mut sub = event_hub.subscribe::<MembershipEvent>().await;

    tokio::spawn(async move {
        while let Ok(event) = sub.recv().await {
            match event {
                MembershipEvent::Joined(member) => {
                    println!("Node joined: {} at {}", member.node_id.id(), member.addr);
                }
                MembershipEvent::Suspect(member) => {
                    println!("Node suspect: {} at {}", member.node_id.id(), member.addr);
                }
                MembershipEvent::Dead(member) => {
                    println!("Node dead: {} at {}", member.node_id.id(), member.addr);
                }
                MembershipEvent::Alive(member) => {
                    println!("Node alive: {} at {}", member.node_id.id(), member.addr);
                }
                MembershipEvent::Left(member) => {
                    println!("Node left: {} at {}", member.node_id.id(), member.addr);
                }
                MembershipEvent::Refuted(member) => {
                    println!("Refuted suspicion: inc={}", member.incarnation);
                }
            }
        }
    });
}
```

### 3. Triggering Dynamic Join via `EventHub` or `MembershipHandle`

```rust
use std::net::SocketAddr;
use aaron_core::{EventHub, Uuid};
use aaron_membership::{JoinClusterCommand, MembershipHandle};

// Option A: Publish dynamic command to EventHub
async fn join_via_event(event_hub: &EventHub, seed: SocketAddr, cluster_id: Uuid) {
    event_hub.publish(JoinClusterCommand::new(seed, Some(cluster_id))).await;
}

// Option B: Trigger join directly via MembershipHandle
async fn join_via_handle(handle: &MembershipHandle, seed: SocketAddr) -> Result<(), aaron_core::BoxError> {
    let discovered = handle.join(seed).await?;
    println!("Joined cluster, discovered {} nodes", discovered.len());
    Ok(())
}
```
