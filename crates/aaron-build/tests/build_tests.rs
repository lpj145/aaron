use std::fs;
use tempfile::tempdir;

#[test]
fn test_compile_basic_schema() {
    let dir = tempdir().expect("failed to create tempdir");
    let schema_file = dir.path().join("simple.fbs");
    let out_dir = dir.path().join("out");

    fs::write(
        &schema_file,
        r#"
namespace Test;

table Message {
    content: string;
    id: uint64;
}

root_type Message;
"#,
    )
    .unwrap();

    let out_file = aaron_build::Builder::new()
        .schema(&schema_file)
        .out_dir(&out_dir)
        .compile()
        .expect("compile failed");

    assert!(out_file.exists());
    let code = fs::read_to_string(&out_file).unwrap();
    assert!(code.contains("pub struct Message"));
}

#[test]
fn test_compile_with_included_node_schema() {
    let dir = tempdir().expect("failed to create tempdir");
    let schema_file = dir.path().join("order.fbs");
    let out_dir = dir.path().join("out");

    fs::write(
        &schema_file,
        r#"
include "node.fbs";

namespace Store;

table Order {
    order_id: Aaron.Node.Uuid;
    amount: uint64;
}

root_type Order;
"#,
    )
    .unwrap();

    let out_file = aaron_build::Builder::new()
        .schema(&schema_file)
        .out_dir(&out_dir)
        .include_node_schema(true)
        .compile()
        .expect("compile with node.fbs failed");

    assert!(out_file.exists());
    let code = fs::read_to_string(&out_file).unwrap();
    assert!(code.contains("pub struct Order"));
    assert!(code.contains("pub struct Uuid") || code.contains("Uuid"));
}

#[test]
fn test_custom_output_filename() {
    let dir = tempdir().expect("failed to create tempdir");
    let schema_file = dir.path().join("inventory.fbs");
    let out_dir = dir.path().join("out");

    fs::write(
        &schema_file,
        r#"
namespace Inventory;

table Item {
    sku: string;
    stock: uint32;
}

root_type Item;
"#,
    )
    .unwrap();

    let out_file = aaron_build::Builder::new()
        .schema(&schema_file)
        .out_dir(&out_dir)
        .out_file("custom_inventory.rs")
        .compile()
        .expect("custom out_file compile failed");

    assert_eq!(out_file, out_dir.join("custom_inventory.rs"));
    assert!(out_file.exists());
}
