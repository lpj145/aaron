use super::scan::{Page, ScanOptions, scan_keyspace};
use crate::BoxError;
use fjall::{Keyspace, Slice, UserValue};
use std::hash::{DefaultHasher, Hasher};
use std::sync::Mutex;

const STRIPE_COUNT: usize = 256;
static UPDATE_STRIPES: [Mutex<()>; STRIPE_COUNT] = [const { Mutex::new(()) }; STRIPE_COUNT];

fn get_stripe_lock(key: &[u8]) -> &Mutex<()> {
    let mut hasher = DefaultHasher::new();
    hasher.write(key);
    let idx = (hasher.finish() as usize) % STRIPE_COUNT;
    &UPDATE_STRIPES[idx]
}

/// Extension trait providing helper methods such as `update` (RMW), `scan`, and `get_string` for [`Keyspace`].
pub trait KeyspaceExt {
    /// Retrieves a UTF-8 string value by key from this keyspace without double-allocating bytes.
    fn get_string(&self, key: impl AsRef<[u8]>) -> Result<Option<String>, BoxError>;

    /// Performs a linearizable, thread-safe Read-Modify-Write (RMW) operation on a key in this keyspace.
    ///
    /// Synchronized via fine-grained striped key locking to guarantee atomic read-modify-write without
    /// lost updates under high concurrency.
    ///
    /// The closure `f` receives the current value (or `None` if missing) and returns:
    /// - `Some(new_value)` to insert or update the value, or
    /// - `None` to remove the key.
    ///
    /// Returns the previous value (or `None` if it did not exist).
    fn update<F, V>(&self, key: impl AsRef<[u8]>, f: F) -> Result<Option<Slice>, BoxError>
    where
        F: FnOnce(Option<Slice>) -> Option<V>,
        V: Into<UserValue>;

    /// Executes a paginated scan with the given [`ScanOptions`].
    fn scan(&self, options: ScanOptions<'_>) -> Result<Page, BoxError>;

    /// Convenient helper for prefix-based cursor pagination.
    fn scan_prefix(
        &self,
        prefix: impl AsRef<[u8]>,
        start_after: Option<impl AsRef<[u8]>>,
        limit: usize,
    ) -> Result<Page, BoxError> {
        let prefix_bytes = prefix.as_ref();
        let mut opts = ScanOptions::new().prefix(prefix_bytes).limit(limit);
        let cursor_bytes = start_after.as_ref().map(|c| c.as_ref());
        if let Some(cursor) = cursor_bytes {
            opts = opts.start_after(cursor);
        }
        self.scan(opts)
    }
}

impl KeyspaceExt for Keyspace {
    fn get_string(&self, key: impl AsRef<[u8]>) -> Result<Option<String>, BoxError> {
        match self.get(key.as_ref())? {
            Some(slice) => {
                let s = std::str::from_utf8(&slice)
                    .map_err(|e| Box::new(e) as BoxError)?
                    .to_owned();
                Ok(Some(s))
            }
            None => Ok(None),
        }
    }

    fn update<F, V>(&self, key: impl AsRef<[u8]>, f: F) -> Result<Option<Slice>, BoxError>
    where
        F: FnOnce(Option<Slice>) -> Option<V>,
        V: Into<UserValue>,
    {
        let key = key.as_ref();
        let _guard = get_stripe_lock(key)
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let old_val = self.get(key)?;
        let new_val = f(old_val.clone());

        match new_val {
            Some(v) => {
                self.insert(key, v)?;
            }
            None => {
                self.remove(key)?;
            }
        }

        Ok(old_val)
    }

    fn scan(&self, options: ScanOptions<'_>) -> Result<Page, BoxError> {
        scan_keyspace(self, options)
    }
}
