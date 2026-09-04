//! # aaron-build
//!
//! Build-time FlatBuffers compiler and code generation tool for Aaron distributed applications.
//!
//! This crate enables downstream Aaron services and applications to compile their domain `.fbs`
//! schemas into high-performance, zero-copy Rust bindings inside their `build.rs` script.
//!
//! ## Example `build.rs`
//!
//! ```no_run
//! fn main() {
//!     aaron_build::Builder::new()
//!         .schema("schemas/order.fbs")
//!         .include_node_schema(true)
//!         .compile()
//!         .expect("failed to compile domain flatbuffers");
//! }
//! ```
//!
//! ## Example Usage in Application Code
//!
//! ```ignore
//! pub mod proto {
//!     include!(concat!(env!("OUT_DIR"), "/order_generated.rs"));
//! }
//! ```

pub mod schemas;

use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Error encountered during FlatBuffers translation or code generation.
#[derive(Debug)]
pub enum BuildError {
    /// No input schemas were provided to the builder.
    NoSchemasProvided,
    /// Specified schema file was not found on the filesystem.
    SchemaNotFound(PathBuf),
    /// Planus failed to translate the FlatBuffers schema files into declarations.
    TranslationFailed(String),
    /// Planus failed to generate Rust code from the declarations.
    CodegenFailed(String),
    /// Standard IO error while reading or writing files.
    Io(io::Error),
    /// Expected environment variable (like `OUT_DIR`) is missing.
    EnvVar(env::VarError),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSchemasProvided => write!(f, "No FlatBuffers schema files provided to compile"),
            Self::SchemaNotFound(p) => write!(f, "FlatBuffers schema not found: {}", p.display()),
            Self::TranslationFailed(msg) => write!(f, "Failed to translate FlatBuffers schema: {msg}"),
            Self::CodegenFailed(msg) => write!(f, "Failed to generate Rust code from FlatBuffers: {msg}"),
            Self::Io(e) => write!(f, "I/O error during schema build: {e}"),
            Self::EnvVar(e) => write!(f, "Environment variable error: {e}"),
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::EnvVar(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for BuildError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<env::VarError> for BuildError {
    fn from(e: env::VarError) -> Self {
        Self::EnvVar(e)
    }
}

/// Fluent builder for compiling FlatBuffers `.fbs` schemas at build time.
#[derive(Debug, Clone)]
pub struct Builder {
    schemas: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    out_file: Option<PathBuf>,
    remove_serde: bool,
    include_node_schema: bool,
    emit_rerun_directives: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            schemas: Vec::new(),
            out_dir: None,
            out_file: None,
            remove_serde: false,
            include_node_schema: false,
            emit_rerun_directives: true,
        }
    }
}

impl Builder {
    /// Creates a new builder instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a FlatBuffers schema file (`.fbs`) to be compiled.
    pub fn schema<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.schemas.push(path.as_ref().to_path_buf());
        self
    }

    /// Adds multiple FlatBuffers schema files to be compiled.
    pub fn schemas<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        for p in paths {
            self.schemas.push(p.as_ref().to_path_buf());
        }
        self
    }

    /// Overrides the output directory (defaults to Cargo's `OUT_DIR` environment variable).
    pub fn out_dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.out_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Sets the output filename or full path.
    ///
    /// If only a filename (e.g. `order_generated.rs`) is given, it is placed in `out_dir`.
    /// If not specified, defaults to `<schema_stem>_generated.rs`.
    pub fn out_file<P: AsRef<Path>>(mut self, file: P) -> Self {
        self.out_file = Some(file.as_ref().to_path_buf());
        self
    }

    /// Controls whether Serde derives (`::serde::Serialize, ::serde::Deserialize`)
    /// from Planus templates should be removed from the generated code (default: `false`).
    ///
    /// When kept as default (`false`), FlatBuffers structures retain Serde derives, enabling
    /// seamless JSON serialization/deserialization with `serde_json`.
    /// Set to `true` when compiling in environments where `serde` is not a dependency.
    pub fn remove_serde(mut self, remove: bool) -> Self {
        self.remove_serde = remove;
        self
    }

    /// Alias for [`remove_serde`].
    pub fn strip_serde(self, remove: bool) -> Self {
        self.remove_serde(remove)
    }

    /// Automatically provisions Aaron's standard `node.fbs` schema, making types like
    /// `Aaron.Node.Uuid` and `Aaron.Node.NodeId` available for `include "node.fbs";`.
    pub fn include_node_schema(mut self, include: bool) -> Self {
        self.include_node_schema = include;
        self
    }

    /// Controls whether `cargo:rerun-if-changed` instructions are printed to stdout (default: `true`).
    pub fn emit_rerun_directives(mut self, emit: bool) -> Self {
        self.emit_rerun_directives = emit;
        self
    }

    /// Compiles the schemas and writes the generated Rust source file.
    ///
    /// Returns the absolute path of the generated Rust source file.
    pub fn compile(self) -> Result<PathBuf, BuildError> {
        if self.schemas.is_empty() {
            return Err(BuildError::NoSchemasProvided);
        }

        for s in &self.schemas {
            if !s.exists() {
                return Err(BuildError::SchemaNotFound(s.clone()));
            }
        }

        let out_dir = match self.out_dir {
            Some(d) => d,
            None => {
                let var = env::var("OUT_DIR")?;
                PathBuf::from(var)
            }
        };

        if !out_dir.exists() {
            fs::create_dir_all(&out_dir)?;
        }

        if self.emit_rerun_directives {
            for s in &self.schemas {
                println!("cargo:rerun-if-changed={}", s.display());
            }
        }

        // Determine destination output file path
        let target_out_file = if let Some(ref custom_file) = self.out_file {
            if custom_file.is_absolute() {
                custom_file.clone()
            } else {
                out_dir.join(custom_file)
            }
        } else {
            let primary_stem = self.schemas[0]
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("schema");
            out_dir.join(format!("{primary_stem}_generated.rs"))
        };

        // Prepare compilation target list
        let mut schemas_to_translate: Vec<PathBuf> = Vec::new();

        if self.include_node_schema {
            // Stage schemas and standard node.fbs in a dedicated build staging directory
            // so relative `include "node.fbs";` resolves seamlessly.
            let staging_dir = out_dir.join("aaron_fbs_staging");
            fs::create_dir_all(&staging_dir)?;

            let staged_node_path = staging_dir.join("node.fbs");
            fs::write(&staged_node_path, schemas::NODE_FBS)?;

            for schema in &self.schemas {
                let file_name = schema.file_name().ok_or_else(|| {
                    BuildError::SchemaNotFound(schema.clone())
                })?;
                let staged_path = staging_dir.join(file_name);
                fs::copy(schema, &staged_path)?;
                schemas_to_translate.push(staged_path);
            }
        } else {
            schemas_to_translate.extend(self.schemas.iter().cloned());
        }

        let slice_paths: Vec<&Path> = schemas_to_translate.iter().map(|p| p.as_path()).collect();

        let declarations = planus_translation::translate_files(&slice_paths).ok_or_else(|| {
            BuildError::TranslationFailed(format!(
                "{}",
                slice_paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;

        let mut generated_code = planus_codegen::generate_rust(&declarations, false)
            .map_err(|e| BuildError::CodegenFailed(e.to_string()))?;

        if self.remove_serde {
            generated_code = generated_code
                .replace(", ::serde::Serialize, ::serde::Deserialize", "")
                .replace("::serde::Serialize, ::serde::Deserialize,", "")
                .replace("::serde::Serialize, ::serde::Deserialize", "");
        }

        fs::write(&target_out_file, generated_code)?;

        Ok(target_out_file)
    }
}

/// Convenience function to compile a single FlatBuffers schema to `$OUT_DIR/<schema_stem>_generated.rs`.
pub fn compile<P: AsRef<Path>>(schema: P) -> Result<PathBuf, BuildError> {
    Builder::new().schema(schema).compile()
}

/// Convenience function to compile a single FlatBuffers schema to a specified output file name.
pub fn compile_with_out<P: AsRef<Path>, O: AsRef<Path>>(
    schema: P,
    out_file: O,
) -> Result<PathBuf, BuildError> {
    Builder::new().schema(schema).out_file(out_file).compile()
}
