# Store Module

A high-performance, thread-safe, embeddable Key-Value storage engine built on top of [Fjall 3.1](https://crates.io/crates/fjall) (LSM-tree).

---

## Table of Contents

- [Overview](#overview)
- [Architecture & Modules](#architecture--modules)
- [Quick Start](#quick-start)
- [Feature Guide](#feature-guide)
  - [1. Opening & Tuning with `StoreBuilder`](#1-opening--tuning-with-storebuilder)
  - [2. Basic Key-Value Operations](#2-basic-key-value-operations)
  - [3. Multiple Isolated Keyspaces (Namespaces)](#3-multiple-isolated-keyspaces-namespaces)
  - [4. Read-Modify-Write (RMW)](#4-read-modify-write-rmw)
  - [5. Paginated Scans (Cursor & Offset)](#5-paginated-scans-cursor--offset)
  - [6. Atomic Write Batches (`WriteBatch`)](#6-atomic-write-batches-writebatch)
  - [7. Point-in-Time Read Snapshots (`Snapshot`)](#7-point-in-time-read-snapshots-snapshot)
  - [8. Physical Backups, Restores & In-Place Snapshot Installation](#8-physical-backups-restores--in-place-snapshot-installation)
- [Performance & Concurrency](#performance--concurrency)

---

## Overview

The `Store` module provides an ergonomic, zero-overhead abstraction over Fjall. It supports:
- Multi-keyspace data isolation (each keyspace is its own LSM-tree).
- Atomic Read-Modify-Write (RMW) closures.
- Zero-allocation cursor-based and prefix-based pagination.
- Multi-keyspace atomic write batches (`WriteBatch`).
- Lock-free Point-in-Time MVCC read snapshots (`Snapshot`).
- Physical crash-consistent backups, restorations, and live snapshot installations.

---

## Architecture & Modules

```
crates/node/src/store/
├── mod.rs        # Module root, Store struct, and public re-exports
├── builder.rs    # StoreBuilder for memory and thread tuning
├── keyspace.rs   # KeyspaceExt trait (RMW, scans, string helpers)
├── scan.rs       # KeyValue, Page, zero-allocation ScanOptions, range search
├── backup.rs     # Physical snapshot copying & validation helpers
└── README.md     # Module documentation
```

---

## Quick Start

```rust
use node::{Store, KeyspaceExt, ScanOptions};

fn main() -> Result<(), node::BoxError> {
    // Open database at the given directory path
    let store = Store::open("./data/node_db")?;

    // Basic CRUD
    store.set("app:name", "Aaron Node")?;
    if let Some(name) = store.get_string("app:name")? {
        println!("App Name: {name}");
    }

    // Read-Modify-Write (RMW)
    store.update("counter", |curr| {
        let val = curr.map_or(0, |s| std::str::from_utf8(&s).unwrap().parse::<i32>().unwrap());
        Some(format!("{}", val + 1))
    })?;

    Ok(())
}
```

---

## Feature Guide

### 1. Opening & Tuning with `StoreBuilder`

Use `Store::builder(path)` to configure hardware parameters, block caches, and background worker threads:

```rust
use node::Store;

let store = Store::builder("./data/node_db")
    .cache_size(64 * 1024 * 1024)   // 64 MB block cache for fast reads
    .worker_threads(4)               // Background compaction & flush workers
    .max_cached_files(Some(128))     // Maximum open file descriptor cache
    .open()?;
```

---

### 2. Basic Key-Value Operations

Operations run directly against the default keyspace:

```rust
// Set values (accepts &str, String, &[u8], Vec<u8>, Slice, etc.)
store.set("user:100", "Alice")?;
store.set("bin:data", vec![0xDE, 0xAD, 0xBE, 0xEF])?;

// Get as raw Slice (zero-copy) or validated UTF-8 String
let slice = store.get("bin:data")?; // Option<fjall::Slice>
let name = store.get_string("user:100")?; // Option<String>

// Query metadata
let exists = store.contains_key("user:100")?;
let total_items = store.len()?;
let is_empty = store.is_empty()?;

// Remove key
store.remove("user:100")?;
```

---

### 3. Multiple Isolated Keyspaces (Namespaces)

Each keyspace is physically isolated in its own LSM-tree directory:

```rust
use node::{Store, KeyspaceExt};

let store = Store::open("./data/node_db")?;

// Open isolated keyspaces
let users = store.keyspace("users")?;
let metrics = store.keyspace("metrics")?;

users.insert("id_1", "Alice")?;
metrics.insert("id_1", "42.0")?;

// Keyspaces do not collide
assert_eq!(users.get_string("id_1")?, Some("Alice".to_string()));
assert_eq!(metrics.get_string("id_1")?, Some("42.0".to_string()));
assert_eq!(store.get("id_1")?, None);
```

---

### 4. Read-Modify-Write (RMW)

The `update` method accepts a closure that receives the current value and returns `Some(new_value)` to write or `None` to remove the key:

```rust
// Atomically update counter or initialize to 1
store.update("requests", |curr| {
    let count = match curr {
        Some(slice) => std::str::from_utf8(&slice).unwrap().parse::<u64>().unwrap(),
        None => 0,
    };
    Some(format!("{}", count + 1))
})?;

// Delete a key conditionally inside an update
store.update::<_, String>("temporary_flag", |_| None)?;
```

---

### 5. Paginated Scans (Cursor & Offset)

The `ScanOptions` struct borrows key prefixes and cursors without heap allocations:

```rust
use node::{Store, KeyspaceExt, ScanOptions};

let store = Store::open("./data/node_db")?;

// Page 1: scan keys with prefix "user:" with limit 10
let page1 = store.scan(
    ScanOptions::new()
        .prefix("user:")
        .limit(10)
)?;

for item in &page1.items {
    println!("{}: {}", item.key_str().unwrap(), item.value_str().unwrap());
}

// Page 2: continue from the cursor returned by Page 1
if page1.has_more {
    let page2 = store.scan(
        ScanOptions::new()
            .prefix("user:")
            .start_after(page1.next_cursor.as_deref().unwrap())
            .limit(10)
    )?;
}

// Shortcut for prefix pagination
let shortcut_page = store.scan_prefix("user:", None::<&str>, 10)?;
```

Additional scan options supported:
- `.start_from(key)`: Inclusive lower bound.
- `.end_at(key)`: Inclusive upper bound.
- `.reverse(true)`: Reverse iteration.
- `.offset(n)`: Skip first `n` items.

---

### 6. Atomic Write Batches (`WriteBatch`)

Batch multiple operations across different keyspaces and commit them in a single atomic disk write:

```rust
use node::Store;

let store = Store::open("./data/node_db")?;
let audit_log = store.keyspace("audit")?;

let mut batch = store.batch();
batch.insert(store.default_keyspace(), "account:1", "balance: 500");
batch.insert(store.default_keyspace(), "account:2", "balance: 750");
batch.insert(&audit_log, "tx:101", "transfer 250 from 1 to 2");

// Atomically commit all changes
batch.commit()?;
```

---

### 7. Point-in-Time Read Snapshots (`Snapshot`)

Snapshots provide lock-free, consistent views of the entire database:

```rust
use fjall::Readable;
use node::Store;

let store = Store::open("./data/node_db")?;
store.set("version", "1.0")?;

// Create point-in-time snapshot
let snapshot = store.snapshot();

// Subsequent writes do not mutate the snapshot view
store.set("version", "2.0")?;

assert_eq!(store.get_string("version")?, Some("2.0".to_string()));
let snap_val = snapshot.get(store.default_keyspace(), "version")?.unwrap();
assert_eq!(&*snap_val, b"1.0");
```

---

### 8. Physical Backups, Restores & In-Place Snapshot Installation

#### Backup
Persists all dirty data with `fsync` and creates a physical copy of the LSM-tree files:
```rust
store.backup("./backups/backup_2026_08_29")?;
```

#### Restore
Initializes a new `Store` from a backup directory into a new path:
```rust
let restored_store = Store::restore("./backups/backup_2026_08_29", "./data/restored_node")?;
```

#### In-Place Snapshot Installation (Raft / Live Recovery)
Swaps the active store's directory with a snapshot directory in-place:
```rust
let mut store = Store::open("./data/active_node")?;

// Replaces all current data with the snapshot state and reopens transparently
store.install_snapshot("./snapshots/epoch_500")?;
```

---

## Performance & Concurrency

- **Thread-Safety**: `Store` and `Keyspace` implement `Clone`, `Send`, and `Sync` (`Arc`-backed).
- **Zero-Allocation Scans**: `ScanOptions<'a>` uses borrowed slices (`&'a [u8]`), avoiding heap allocations.
- **Direct Index Seeking**: Scans using cursors or ranges seek directly in the LSM-tree index in $O(\log N)$ time.
- **Zero-Copy Slices**: Reading binary data with `store.get(...)` returns `Option<fjall::Slice>`, eliminating memory copies.
- **Batching**: Use `WriteBatch` for bulk writes to maximize I/O throughput.
