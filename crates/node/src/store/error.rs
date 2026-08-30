use crate::error::{Error, ErrorKind};
use snafu::Snafu;
use std::path::PathBuf;

/// Strongly-typed storage errors for Aaron Node.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum StoreError {
    #[snafu(display("Store is locked for snapshot installation or maintenance"))]
    LockedForMaintenance,

    #[snafu(display("Keyspace '{name}' not found"))]
    KeyspaceNotFound { name: String },

    #[snafu(display("Snapshot source path '{path:?}' cannot be the current store directory"))]
    SnapshotSameDir { path: PathBuf },

    #[snafu(display("Storage engine error: {source}"))]
    Fjall { source: fjall::Error },

    #[snafu(display("I/O error at '{path:?}': {source}"))]
    Io {
        source: std::io::Error,
        path: Option<PathBuf>,
    },

    #[snafu(display("Invalid UTF-8 data: {source}"))]
    Utf8 { source: std::str::Utf8Error },
}

impl From<StoreError> for Error {
    fn from(err: StoreError) -> Self {
        let kind = match err {
            StoreError::LockedForMaintenance => ErrorKind::LockedForMaintenance,
            StoreError::KeyspaceNotFound { .. } => ErrorKind::NotFound,
            StoreError::SnapshotSameDir { .. } => ErrorKind::InvalidInput,
            StoreError::Fjall { .. } => ErrorKind::Internal,
            StoreError::Io { ref source, .. } => match source.kind() {
                std::io::ErrorKind::NotFound => ErrorKind::NotFound,
                std::io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
                _ => ErrorKind::Internal,
            },
            StoreError::Utf8 { .. } => ErrorKind::InvalidInput,
        };
        Error::new(kind, err.to_string()).with_source(err)
    }
}

impl From<fjall::Error> for StoreError {
    fn from(source: fjall::Error) -> Self {
        StoreError::Fjall { source }
    }
}
