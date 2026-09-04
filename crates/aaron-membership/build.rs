use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let schema_path = out_dir.join("membership.fbs");
    fs::write(&schema_path, aaron_build::schemas::MEMBERSHIP_FBS)
        .expect("failed to write embedded membership.fbs schema");

    aaron_build::Builder::new()
        .schema(&schema_path)
        .include_node_schema(true)
        .out_file("membership_generated.rs")
        .remove_serde(true)
        .compile()
        .expect("failed to compile membership.fbs FlatBuffers schema");
}

