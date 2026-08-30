use crate::BoxError;
use fjall::{Keyspace, UserKey, UserValue};

/// A single key-value entry returned from a paginated scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyValue {
    /// The raw binary key.
    pub key: UserKey,
    /// The raw binary value.
    pub value: UserValue,
}

impl KeyValue {
    /// Returns the key as a UTF-8 string slice, or `None` if invalid UTF-8.
    pub fn key_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.key).ok()
    }

    /// Returns the value as a UTF-8 string slice, or `None` if invalid UTF-8.
    pub fn value_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.value).ok()
    }
}

/// A paginated result set from a scan operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// The key-value pairs contained in this page.
    pub items: Vec<KeyValue>,
    /// Cursor pointing to the last key in `items`. Can be passed to `start_after` for the next page.
    pub next_cursor: Option<UserKey>,
    /// `true` if there are more items remaining beyond this page.
    pub has_more: bool,
}

/// Options for configuring paginated scans over a keyspace without heap allocations.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScanOptions<'a> {
    /// Optional prefix filter to only scan keys starting with this prefix.
    pub prefix: Option<&'a [u8]>,
    /// Exclusive lower bound: start scanning strictly after this key (cursor pagination).
    pub start_after: Option<&'a [u8]>,
    /// Inclusive lower bound: start scanning from this key.
    pub start_from: Option<&'a [u8]>,
    /// Inclusive upper bound: stop scanning when a key exceeds this bound.
    pub end_at: Option<&'a [u8]>,
    /// Maximum number of items to return in this page (defaults to 50 if not specified).
    pub limit: Option<usize>,
    /// Number of matching items to skip before collecting results.
    pub offset: usize,
    /// If `true`, scans keys in reverse order.
    pub reverse: bool,
}

impl<'a> ScanOptions<'a> {
    /// Creates a new default `ScanOptions`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filters keys by prefix without heap allocations.
    pub fn prefix(mut self, prefix: &'a (impl AsRef<[u8]> + ?Sized)) -> Self {
        self.prefix = Some(prefix.as_ref());
        self
    }

    /// Sets the cursor to start scanning strictly after `cursor` (exclusive) without heap allocations.
    pub fn start_after(mut self, cursor: &'a (impl AsRef<[u8]> + ?Sized)) -> Self {
        self.start_after = Some(cursor.as_ref());
        self
    }

    /// Sets the key to start scanning from (inclusive) without heap allocations.
    pub fn start_from(mut self, key: &'a (impl AsRef<[u8]> + ?Sized)) -> Self {
        self.start_from = Some(key.as_ref());
        self
    }

    /// Sets the upper limit key to stop scanning at (inclusive) without heap allocations.
    pub fn end_at(mut self, key: &'a (impl AsRef<[u8]> + ?Sized)) -> Self {
        self.end_at = Some(key.as_ref());
        self
    }

    /// Sets the maximum number of items returned in the page.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Sets the offset (number of items to skip).
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Sets whether to scan in reverse order.
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }
}

pub(crate) fn scan_keyspace(
    keyspace: &Keyspace,
    options: ScanOptions<'_>,
) -> Result<Page, BoxError> {
    let limit = options.limit.unwrap_or(50);
    let target_fetch = limit.saturating_add(1);
    let forward = !options.reverse;

    let raw_iter = if forward {
        if let Some(prefix) = options.prefix {
            if let Some(start_from) = options.start_from {
                if start_from > prefix {
                    keyspace.range(start_from..)
                } else {
                    keyspace.prefix(prefix)
                }
            } else if let Some(start_after) = options.start_after {
                if start_after >= prefix {
                    keyspace.range(start_after..)
                } else {
                    keyspace.prefix(prefix)
                }
            } else {
                keyspace.prefix(prefix)
            }
        } else if let Some(start_from) = options.start_from {
            keyspace.range(start_from..)
        } else if let Some(start_after) = options.start_after {
            keyspace.range(start_after..)
        } else {
            keyspace.iter()
        }
    } else if let Some(prefix) = options.prefix {
        keyspace.prefix(prefix)
    } else {
        keyspace.iter()
    };

    let mut items = Vec::new();
    let mut skipped = 0;

    let mut process_kv = |k: UserKey, v: UserValue| -> bool {
        if let Some(prefix) = options.prefix
            && !k.starts_with(prefix)
        {
            if (forward && &*k > prefix) || (!forward && &*k < prefix) {
                return false;
            }
            return true;
        }

        if let Some(start_after) = options.start_after
            && ((forward && &*k <= start_after) || (!forward && &*k >= start_after))
        {
            return true;
        }

        if let Some(start_from) = options.start_from
            && ((forward && &*k < start_from) || (!forward && &*k > start_from))
        {
            return true;
        }

        if let Some(end_at) = options.end_at
            && ((forward && &*k > end_at) || (!forward && &*k < end_at))
        {
            return false;
        }

        if skipped < options.offset {
            skipped += 1;
            return true;
        }

        items.push(KeyValue { key: k, value: v });
        items.len() < target_fetch
    };

    if forward {
        for guard in raw_iter {
            let (k, v) = guard.into_inner()?;
            if !process_kv(k, v) {
                break;
            }
        }
    } else {
        for guard in raw_iter.rev() {
            let (k, v) = guard.into_inner()?;
            if !process_kv(k, v) {
                break;
            }
        }
    }

    let has_more = items.len() > limit;
    if has_more {
        items.pop();
    }

    let next_cursor = items.last().map(|kv| kv.key.clone());

    Ok(Page {
        items,
        next_cursor,
        has_more,
    })
}
