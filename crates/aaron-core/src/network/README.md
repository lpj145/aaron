# Network Module

A unified, multi-transport network manager for distributed node communication with automatic connection pooling and listener management.

---

## Table of Contents

- [Overview](#overview)
- [Architecture & Modules](#architecture--modules)
- [Quick Start](#quick-start)
- [Feature Guide](#feature-guide)
  - [1. TCP Inbound & Outbound with Connection Pooling](#1-tcp-inbound--outbound-with-connection-pooling)
  - [2. UDP Socket Binding & Datagrams](#2-udp-socket-binding--datagrams)
  - [3. QUIC with Web-of-Trust P2P TLS](#3-quic-with-web-of-trust-p2p-tls)
- [Thread Safety & Concurrency](#thread-safety--concurrency)

---

## Overview

The `Network` module provides a central facade (`network.tcp`, `network.udp`, `network.quic`) managing:
- **Inbound Listeners**: Listening on TCP ports, binding UDP sockets, or hosting QUIC endpoints.
- **Outbound Connection Pooling**: Transparently reusing active TCP/QUIC connections to the same target peer without duplicate handshakes.
- **Web-of-Trust P2P TLS**: Self-signed certificate generation and custom TLS 1.3 verification for decentralized peer authentication without Web PKI CAs.
- **Thread-Safe Handles**: Safe asynchronous read/write and multiplexed bi-directional stream operations across concurrent Tokio tasks.

---

## Architecture & Modules

```
crates/aaron-core/src/network/
├── mod.rs             # Network facade struct and re-exports
├── tcp/
│   ├── mod.rs         # TcpManager (listen, connect, disconnect)
│   ├── pool.rs        # Concurrent TcpPool keyed by SocketAddr
│   └── connection.rs  # Full-duplex managed TcpConnection handle (split reader/writer)
├── udp/
│   └── mod.rs         # UdpManager (bind, get_or_bind, unbind)
├── quic/
│   ├── mod.rs         # QuicManager (listen, connect, pool)
│   ├── tls.rs         # Web-of-Trust P2P TLS (self-signed cert generation & P2pServerCertVerifier)
│   └── pool.rs        # QuicPool (multiplexed QUIC connection pool)
└── README.md          # Module documentation
```

---

## Quick Start

```rust
use aaron_core::Network;

#[tokio::main]
async fn main() -> Result<(), aaron_core::BoxError> {
    let network = Network::new();

    // 1. TCP Inbound & Outbound
    let listener = network.tcp.listen("0.0.0.0:8080").await?;
    let conn = network.tcp.connect("192.168.1.50:8080").await?;
    conn.write_all(b"PING").await?;

    // 2. QUIC with P2P TLS
    let quic_server = network.quic.listen("0.0.0.0:9000").await?;
    let quic_conn = network.quic.connect("192.168.1.50:9000", "localhost").await?;
    let (mut send, mut recv) = quic_conn.open_bi().await?;
    send.write_all(b"QUIC PING").await?;

    Ok(())
}
```

---

## Feature Guide

### 1. TCP Inbound & Outbound with Connection Pooling

#### Inbound Listener
```rust
let listener = network.tcp.listen("127.0.0.1:9000").await?;
while let Ok((stream, peer_addr)) = listener.accept().await {
    println!("Incoming connection from: {peer_addr}");
}
```

#### Outbound Connection Pooling & Full-Duplex Splitting
```rust
// Connect: registers in pool
let conn1 = network.tcp.connect("127.0.0.1:9001").await?;

// Second connect to the same address: returns existing connection from pool
let conn2 = network.tcp.connect("127.0.0.1:9001").await?;
assert_eq!(network.tcp.pool().count().await, 1);

// Split into independent reader and writer handles (no locking contention)
let (reader, writer) = conn1.split();
```

---

### 2. UDP Socket Binding & Datagrams

```rust
let udp_socket = network.udp.bind("0.0.0.0:9002").await?;

// Send datagram to peer
udp_socket.send_to(b"discovery_ping", "192.168.1.100:9002").await?;

// Receive datagram
let mut buf = [0u8; 1024];
let (len, sender) = udp_socket.recv_from(&mut buf).await?;
```

---

### 3. QUIC with Web-of-Trust P2P TLS

Quinn-powered QUIC transport with automatic self-signed TLS certificate generation and custom peer certificate verification.

#### Server Endpoint (Inbound)
```rust
// Automatically generates self-signed P2P TLS certificates
let server_endpoint = network.quic.listen("127.0.0.1:9003").await?;
let incoming = server_endpoint.accept().await.unwrap();
let connection = incoming.await?;

// Accept multiplexed bi-directional stream
let (mut send, mut recv) = connection.accept_bi().await?;
```

#### Client with Pooling & Stream Multiplexing (Outbound)
```rust
let conn = network.quic.connect("127.0.0.1:9003", "localhost").await?;

// Reuses the single physical QUIC connection to open hundreds of concurrent streams
let (mut send1, mut recv1) = conn.open_bi().await?;
let (mut send2, mut recv2) = conn.open_bi().await?;
```

---

## Thread Safety & Concurrency

- `Network`, `TcpManager`, `TcpConnection`, `UdpManager`, and `QuicManager` are fully `Clone + Send + Sync + 'static`.
- Outbound pools are protected by non-blocking asynchronous `RwLock` primitives from Tokio.
