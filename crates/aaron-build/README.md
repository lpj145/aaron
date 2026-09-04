# aaron-build

Build-time FlatBuffers schema compiler and code generation tool for Aaron distributed applications and services.

`aaron-build` provides an idiomatic, lightweight builder pattern (similar to `tonic-build` and `prost-build`) designed to be executed inside a downstream service's `build.rs` script. It compiles `.fbs` FlatBuffers schemas into zero-copy, memory-safe Rust bindings without pulling runtime dependencies into the host compiler.

---

## Features

- **Lightweight Build Dependencies**: Depends strictly on `planus-translation` and `planus-codegen`. Does not pull Tokio, Quinn, Fjall, or any Aaron runtime crates into host builds.
- **Embedded Core Schemas**: Bundles standard Aaron FlatBuffers schemas (`node.fbs`, `membership.fbs`, `control_plane.fbs`, `shard.fbs`).
- **Seamless Includes**: Automatically resolves `include "node.fbs";` so domain schemas can utilize `Aaron.Node.Uuid` and `Aaron.Node.NodeId` without copying files.
- **Automated JSON / Serde Support**: Retains Planus's native Serde derives by default for instant JSON serialization via `serde_json`, or strips them on demand via `.remove_serde(true)`.
- **Cargo Rerun Directives**: Automatically prints `cargo:rerun-if-changed=<path>` for tracked schemas.

---

## Installation

Add `aaron-build` to your `[build-dependencies]` and `planus` to your `[dependencies]`:

```toml
[dependencies]
planus = "1.3.0"

[build-dependencies]
aaron-build = { path = "../aaron/crates/aaron-build" } # or version = "0.1"
```

---

## Quick Start

### 1. Define a Domain FlatBuffers Schema

Create your schema in `schemas/order.fbs`:

```flatbuffers
include "node.fbs";

namespace MyDomain;

table Order {
    order_id: Aaron.Node.Uuid;
    customer_id: string;
    total_cents: uint64;
}

root_type Order;
```

### 2. Configure `build.rs`

In your crate's `build.rs`:

```rust
fn main() {
    aaron_build::Builder::new()
        .schema("schemas/order.fbs")
        .include_node_schema(true)
        .compile()
        .expect("failed to compile domain FlatBuffers schema");
}
```

### 3. Include Generated Rust Code in Your Application

In `src/lib.rs` or `src/domain.rs`:

```rust
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/order_generated.rs"));
}

pub use proto::my_domain::*;
```

---

## Builder Configuration

The `Builder` API provides granular control over compilation:

| Method | Default | Description |
| :--- | :--- | :--- |
| `schema(path)` | None | Appends a single `.fbs` schema path |
| `schemas(paths)` | None | Appends an iterator of `.fbs` schema paths |
| `out_file(name)` | `<stem>_generated.rs` | Customizes output file name inside `OUT_DIR` |
| `out_dir(path)` | `$OUT_DIR` | Overrides target generation directory |
| `include_node_schema(bool)` | `false` | Stages `node.fbs` for `include "node.fbs";` resolution |
| `remove_serde(bool)` | `false` | Strips Serde derives from templates (set `true` if `serde` is not in dependencies) |
| `strip_serde(bool)` | `false` | Alias for `remove_serde` |
| `emit_rerun_directives(bool)`| `true` | Emits `cargo:rerun-if-changed` to stdout |

---

## Convenience Functions

For simple, single-schema setups:

```rust
// Compiles to $OUT_DIR/order_generated.rs
aaron_build::compile("schemas/order.fbs")
    .expect("failed to compile schema");

// Compiles to $OUT_DIR/my_custom_name.rs
aaron_build::compile_with_out("schemas/order.fbs", "my_custom_name.rs")
    .expect("failed to compile schema");
```
