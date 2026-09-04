use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let schema_path = out_dir.join("shard.fbs");
    fs::write(&schema_path, aaron_build::schemas::SHARD_FBS)
        .expect("failed to write embedded shard.fbs schema");

    aaron_build::Builder::new()
        .schema(&schema_path)
        .include_node_schema(true)
        .out_file("shard_generated.rs")
        .remove_serde(true)
        .compile()
        .expect("failed to compile shard.fbs FlatBuffers schema");
}

