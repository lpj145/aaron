use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use fjall::{Database, Iter, KeyspaceCreateOptions, PersistMode, Slice, UserValue};
pub use fjall::{Keyspace, OwnedWriteBatch as WriteBatch, Readable, Snapshot};

use crate::BoxError;

mod backup;
mod builder;
pub mod error;
mod keyspace;
mod scan;

pub use builder::StoreBuilder;
pub use error::StoreError;
pub use keyspace::KeyspaceExt;
pub use scan::{KeyValue, Page, ScanOptions};

use backup::copy_dir_all;

pub(crate) struct StoreState {
    pub(crate) db: Database,
    pub(crate) default_keyspace: Keyspace,
}

/// A thread-safe, embeddable key-value storage engine powered by Fjall.
///
/// Manages keyspaces and provides convenient key-value, Read-Modify-Write (RMW),
/// atomic batch writing, paginated scanning, snapshotting, and backup operations.
///
/// # Shared State Across Clones
///
/// `Store` shares internal database handles via an `Arc<RwLock<StoreState>>`.
/// When [`install_snapshot`](Self::install_snapshot) replaces the underlying database,
/// all cloned `Store` handles (such as across supervised services in a `Node`)
/// seamlessly observe the new database state without dangling pointer errors.
#[derive(Clone)]
pub struct Store {
    pub(crate) state: Arc<RwLock<StoreState>>,
    pub(crate) path: PathBuf,
    pub(crate) maintenance: Arc<std::sync::atomic::AtomicBool>,
}

impl Store {
    /// Opens or creates a new Store database at the specified filesystem path with default settings.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BoxError> {
        Self::builder(path).open()
    }

    /// Returns a [`StoreBuilder`] to configure performance settings (cache size, worker threads, etc.) before opening.
    pub fn builder(path: impl AsRef<Path>) -> StoreBuilder {
        StoreBuilder::new(path)
    }

    fn read_state(&self) -> std::sync::RwLockReadGuard<'_, StoreState> {
        self.state.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write_state(&self) -> std::sync::RwLockWriteGuard<'_, StoreState> {
        self.state.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Returns true if the store is currently locked in maintenance mode (e.g. during snapshot installation).
    pub fn is_maintenance(&self) -> bool {
        self.maintenance.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Sets or clears the store maintenance mode.
    ///
    /// When maintenance mode is active, mutating operations (`set`, `remove`, `update`, etc.)
    /// are rejected immediately with an error.
    pub fn set_maintenance(&self, enabled: bool) {
        self.maintenance
            .store(enabled, std::sync::atomic::Ordering::SeqCst);
    }

    fn check_writable(&self) -> Result<(), BoxError> {
        if self.is_maintenance() {
            return Err(Box::new(std::io::Error::other(
                "Store is locked: snapshot installation or maintenance in progress",
            )));
        }
        Ok(())
    }

    /// Returns the database root filesystem path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Opens or creates an isolated keyspace with the given name.
    pub fn keyspace(&self, name: &str) -> Result<Keyspace, BoxError> {
        let guard = self.read_state();
        let keyspace = guard.db.keyspace(name, KeyspaceCreateOptions::default)?;
        Ok(keyspace)
    }

    /// Returns a handle to the default keyspace.
    pub fn default_keyspace(&self) -> Keyspace {
        self.read_state().default_keyspace.clone()
    }

    /// Retrieves a value by key from the default keyspace.
    pub fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Slice>, BoxError> {
        let guard = self.read_state();
        let val = guard.default_keyspace.get(key.as_ref())?;
        Ok(val)
    }

    /// Retrieves a UTF-8 string value by key from the default keyspace.
    pub fn get_string(&self, key: impl AsRef<[u8]>) -> Result<Option<String>, BoxError> {
        let guard = self.read_state();
        guard.default_keyspace.get_string(key)
    }

    /// Sets a key-value pair in the default keyspace.
    pub fn set(&self, key: impl AsRef<[u8]>, value: impl Into<UserValue>) -> Result<(), BoxError> {
        self.check_writable()?;
        let guard = self.read_state();
        guard.default_keyspace.insert(key.as_ref(), value)?;
        Ok(())
    }

    /// Removes a key from the default keyspace.
    pub fn remove(&self, key: impl AsRef<[u8]>) -> Result<(), BoxError> {
        self.check_writable()?;
        let guard = self.read_state();
        guard.default_keyspace.remove(key.as_ref())?;
        Ok(())
    }

    /// Checks if a key exists in the default keyspace.
    pub fn contains_key(&self, key: impl AsRef<[u8]>) -> Result<bool, BoxError> {
        let guard = self.read_state();
        let exists = guard.default_keyspace.contains_key(key.as_ref())?;
        Ok(exists)
    }

    /// Returns the approximate number of items in the default keyspace.
    pub fn len(&self) -> Result<usize, BoxError> {
        let guard = self.read_state();
        Ok(guard.default_keyspace.len()?)
    }

    /// Returns true if the default keyspace is empty.
    pub fn is_empty(&self) -> Result<bool, BoxError> {
        let guard = self.read_state();
        Ok(guard.default_keyspace.is_empty()?)
    }

    /// Performs a convenience Read-Modify-Write update on a key in the default keyspace.
    ///
    /// The closure `f` receives the current value (or `None` if missing) and returns:
    /// - `Some(new_value)` to insert or update the value, or
    /// - `None` to remove the key.
    ///
    /// Returns the previous value (or `None` if it did not exist).
    /// Note: This is an application-level helper and does not hold cross-thread table locks.
    pub fn update<F, V>(&self, key: impl AsRef<[u8]>, f: F) -> Result<Option<Slice>, BoxError>
    where
        F: FnOnce(Option<Slice>) -> Option<V>,
        V: Into<UserValue>,
    {
        self.check_writable()?;
        let guard = self.read_state();
        guard.default_keyspace.update(key, f)
    }

    /// Initializes a new atomic [`WriteBatch`].
    ///
    /// Allows batching multiple insertions and deletions across keyspaces to be committed atomically.
    pub fn batch(&self) -> WriteBatch {
        self.read_state().db.batch()
    }

    /// Executes a paginated scan over the default keyspace with the given [`ScanOptions`].
    pub fn scan(&self, options: ScanOptions<'_>) -> Result<Page, BoxError> {
        let guard = self.read_state();
        guard.default_keyspace.scan(options)
    }

    /// Convenient helper for prefix-based cursor pagination on the default keyspace.
    pub fn scan_prefix(
        &self,
        prefix: impl AsRef<[u8]>,
        start_after: Option<impl AsRef<[u8]>>,
        limit: usize,
    ) -> Result<Page, BoxError> {
        let guard = self.read_state();
        guard
            .default_keyspace
            .scan_prefix(prefix, start_after, limit)
    }

    /// Returns an iterator prefix scan over the default keyspace.
    pub fn prefix(&self, prefix: impl AsRef<[u8]>) -> Iter {
        let guard = self.read_state();
        guard.default_keyspace.prefix(prefix.as_ref())
    }

    /// Returns an iterator over all key-value pairs in the default keyspace.
    pub fn iter(&self) -> Iter {
        let guard = self.read_state();
        guard.default_keyspace.iter()
    }

    /// Creates a cross-keyspace point-in-time [`Snapshot`].
    ///
    /// Snapshots provide a consistent, lock-free read view of the database at the current sequence number.
    pub fn snapshot(&self) -> Snapshot {
        self.read_state().db.snapshot()
    }

    /// Persists all pending journal writes and flushes data to disk with fsync.
    pub fn persist(&self) -> Result<(), BoxError> {
        let guard = self.read_state();
        guard.db.persist(PersistMode::SyncAll)?;
        Ok(())
    }

    /// Creates a complete point-in-time backup of the database to `target_dir`.
    ///
    /// Flushes and syncs all data to disk first, then creates a full copy of the database files.
    pub fn backup(&self, target_dir: impl AsRef<Path>) -> Result<(), BoxError> {
        self.persist()?;
        copy_dir_all(self.path(), target_dir.as_ref())?;
        Ok(())
    }

    /// Restores and opens a new Store from a snapshot or backup directory into `target_dir`.
    ///
    /// If `target_dir` already exists, it is cleaned first to prevent remnant file corruption.
    pub fn restore(
        snapshot_dir: impl AsRef<Path>,
        target_dir: impl AsRef<Path>,
    ) -> Result<Self, BoxError> {
        let snapshot_dir = snapshot_dir.as_ref();
        let target_dir = target_dir.as_ref();

        if target_dir.exists() {
            std::fs::remove_dir_all(target_dir)?;
        }
        copy_dir_all(snapshot_dir, target_dir)?;
        Self::open(target_dir)
    }

    /// Installs a snapshot directory into this store, replacing all current data in-place.
    ///
    /// This flushes active data, closes the active database, replaces the underlying directory contents
    /// with the snapshot data, and atomically reopens the database for all cloned `Store` handles.
    pub fn install_snapshot(&self, snapshot_dir: impl AsRef<Path>) -> Result<(), BoxError> {
        let snapshot_dir = snapshot_dir.as_ref();
        if snapshot_dir == self.path {
            return Err(Box::new(std::io::Error::other(
                "snapshot source cannot be the current store directory",
            )));
        }

        // Set maintenance mode to reject concurrent writes immediately
        self.set_maintenance(true);
        struct MaintenanceGuard<'a>(&'a Store);
        impl<'a> Drop for MaintenanceGuard<'a> {
            fn drop(&mut self) {
                self.0.set_maintenance(false);
            }
        }
        let _maintenance_guard = MaintenanceGuard(self);

        // Acquire exclusive write lock spanning the ENTIRE snapshot installation
        // to ensure zero concurrent writes land in temporary/discarded handles.
        let mut state = self.write_state();

        // 1. Flush pending data on the active DB
        state.db.persist(fjall::PersistMode::SyncAll)?;

        // 2. Open a temporary database to replace current handles so active directory file locks are released
        static SWAP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let count = SWAP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp_dir =
            std::env::temp_dir().join(format!("store_swap_{}_{}", std::process::id(), count));

        let temp_db = Database::builder(&temp_dir).open()?;
        let temp_ks = temp_db.keyspace("default", KeyspaceCreateOptions::default)?;

        state.default_keyspace = temp_ks;
        state.db = temp_db;

        // 3. Clear existing store directory and copy snapshot files
        if self.path.exists() {
            std::fs::remove_dir_all(&self.path)?;
        }
        copy_dir_all(snapshot_dir, &self.path)?;

        // 4. Reopen database from the freshly installed directory
        let new_db = Database::builder(&self.path).open()?;
        let new_default = new_db.keyspace("default", KeyspaceCreateOptions::default)?;

        state.db = new_db;
        state.default_keyspace = new_default;

        // 5. Clean up temporary directory
        let _ = std::fs::remove_dir_all(temp_dir);

        Ok(())
    }

    /// Returns a handle to the underlying [`fjall::Database`].
    pub fn raw_db(&self) -> Database {
        self.read_state().db.clone()
    }

    /// Returns a list of all existing keyspace names in the database.
    pub fn list_keyspaces(&self) -> Vec<String> {
        self.read_state()
            .db
            .list_keyspace_names()
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.path())
            .finish_non_exhaustive()
    }
}
