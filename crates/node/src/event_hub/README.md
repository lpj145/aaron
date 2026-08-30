# EventHub Module

A high-performance, strongly-typed in-memory Pub/Sub event bus powered by lockless `crossfire` channels.

---

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Quick Start](#quick-start)
- [Features](#features)
  - [1. Strongly-Typed Events](#1-strongly-typed-events)
  - [2. Multiple Subscribers (Fan-Out)](#2-multiple-subscribers-fan-out)
  - [3. Lockless Crossfire Performance](#3-lockless-crossfire-performance)
  - [4. Automatic Dead Subscriber Pruning](#4-automatic-dead-subscriber-pruning)
- [Thread Safety](#thread-safety)

---

## Overview

The `EventHub` enables asynchronous decoupling across services, background workers, and network handlers by providing a type-driven publish-subscribe bus:
- **No String Topic Magic**: The Rust type `E` *is* the topic.
- **Lock-Free Buffering**: Each subscriber receives its own lockless ring-buffer queue via `crossfire`.
- **Fan-out Multicast**: Every published event is cloned and dispatched to all active subscribers of that type.

---

## Architecture

```
crates/node/src/event_hub/
├── mod.rs          # EventHub struct (publish, subscribe, count, clear)
├── subscriber.rs   # Subscriber<E> handle (recv, try_recv)
├── topic.rs        # Type-erased TopicDispatcher<E>
└── README.md       # Module documentation
```

---

## Quick Start

```rust
use node::EventHub;

// 1. Declare any event type
#[derive(Clone, Debug, PartialEq)]
pub struct PeerJoined {
    pub peer_id: String,
}

#[tokio::main]
async fn main() -> Result<(), node::BoxError> {
    let hub = EventHub::new();

    // 2. Subscribe
    let mut sub = hub.subscribe::<PeerJoined>().await;

    // 3. Publish
    hub.publish(PeerJoined {
        peer_id: "node-42".to_string(),
    }).await;

    // 4. Receive
    let event = sub.recv().await?;
    println!("Received: {:?}", event.peer_id);

    Ok(())
}
```

---

## Features

### 1. Strongly-Typed Events

Any type implementing `Clone + Send + Sync + 'static` can be published and subscribed to:

```rust
#[derive(Clone, Debug)]
pub struct StorageFlushed {
    pub bytes_written: u64,
}

hub.publish(StorageFlushed { bytes_written: 4096 }).await;
```

### 2. Multiple Subscribers (Fan-Out)

Multiple independent subscribers can listen to the exact same event type. Each subscriber receives a full copy of the published event:

```rust
let mut sub1 = hub.subscribe::<OrderPlaced>().await;
let mut sub2 = hub.subscribe::<OrderPlaced>().await;

hub.publish(OrderPlaced { order_id: 1 }).await;

assert_eq!(sub1.recv().await?.order_id, 1);
assert_eq!(sub2.recv().await?.order_id, 1);
```

### 3. Automatic Dead Subscriber Pruning

When a `Subscriber<E>` is dropped or exits its loop, `EventHub` automatically prunes the disconnected queue on the next publish call, preventing memory leaks.
