use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let schema_path = out_dir.join("node.fbs");
    fs::write(&schema_path, aaron_build::schemas::NODE_FBS)
        .expect("failed to write embedded node.fbs schema");

    aaron_build::Builder::new()
        .schema(&schema_path)
        .out_file("node_generated.rs")
        .remove_serde(true)
        .compile()
        .expect("failed to compile node.fbs FlatBuffers schema");
}

