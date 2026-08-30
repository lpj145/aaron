use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let workspace_root = Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("failed to find workspace root");

    let schema_path = workspace_root.join("schemas/node.fbs");
    let schema_display = schema_path.display().to_string();

    println!("cargo:rerun-if-changed={schema_display}");

    let declarations = planus_translation::translate_files(&[&schema_path])
        .unwrap_or_else(|| panic!("failed to translate FlatBuffers schema: {schema_display}"));

    let generated_code = planus_codegen::generate_rust(&declarations, false)
        .expect("failed to generate Rust code from FlatBuffers declarations");

    // Strip hardcoded serde derives from planus templates so serde is not required as a dependency
    let generated_code = generated_code
        .replace(", ::serde::Serialize, ::serde::Deserialize", "")
        .replace("::serde::Serialize, ::serde::Deserialize,", "")
        .replace("::serde::Serialize, ::serde::Deserialize", "");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_file = PathBuf::from(out_dir).join("node_generated.rs");

    fs::write(&out_file, generated_code).unwrap_or_else(|e| {
        panic!(
            "failed to write generated code to {}: {e}",
            out_file.display()
        )
    });
}
