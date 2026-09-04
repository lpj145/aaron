use super::Store;
use crate::BoxError;
use fjall::{Database, KeyspaceCreateOptions};
use std::path::{Path, PathBuf};

/// Builder for configuring and opening a [`Store`] with custom performance tuning.
pub struct StoreBuilder {
    builder: fjall::DatabaseBuilder<Database>,
    path: PathBuf,
}

impl StoreBuilder {
    /// Creates a new `StoreBuilder` for the specified path.
    pub fn new(path: impl AsRef<Path>) -> Self {
        let path_buf = path.as_ref().to_path_buf();
        Self {
            builder: Database::builder(&path_buf),
            path: path_buf,
        }
    }

    /// Sets the block cache capacity in bytes (recommended: 20-25% of available RAM).
    #[must_use]
    pub fn cache_size(mut self, size_bytes: u64) -> Self {
        self.builder = self.builder.cache_size(size_bytes);
        self
    }

    /// Sets the number of background worker threads for flushing and compactions.
    #[must_use]
    pub fn worker_threads(mut self, n: usize) -> Self {
        self.builder = self.builder.worker_threads(n);
        self
    }

    /// Sets the maximum number of cached open file descriptors.
    #[must_use]
    pub fn max_cached_files(mut self, n: Option<usize>) -> Self {
        self.builder = self.builder.max_cached_files(n);
        self
    }

    /// Sets whether journal persistence is manual.
    #[must_use]
    pub fn manual_journal_persist(mut self, flag: bool) -> Self {
        self.builder = self.builder.manual_journal_persist(flag);
        self
    }

    /// Opens the configured database.
    pub fn open(self) -> Result<Store, BoxError> {
        let db = self.builder.open()?;
        let default_keyspace = db.keyspace("default", KeyspaceCreateOptions::default)?;
        Ok(Store {
            state: std::sync::Arc::new(std::sync::RwLock::new(super::StoreState {
                db,
                default_keyspace,
                stripes: std::sync::Arc::new([const { std::sync::Mutex::new(()) }; super::STRIPE_COUNT]),
            })),
            path: self.path,
            maintenance: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }
}
