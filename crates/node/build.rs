use std::env;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let workspace_root = Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("failed to find workspace root");

    let schema_path = workspace_root.join("schemas/node.fbs");

    aaron_build::Builder::new()
        .schema(schema_path)
        .out_file("node_generated.rs")
        .remove_serde(true)
        .compile()
        .expect("failed to compile node.fbs FlatBuffers schema");
}
