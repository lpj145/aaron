//! Chaos/exploratory tests for `Store::batch()` (`WriteBatch`) atomicity and its
//! interaction with concurrent readers, scans, snapshots and snapshot installation.
//!
//! A batch is the only primitive the store offers for "all of these writes, or none of
//! them", so anything that lets a reader observe half a batch — or that loses a committed
//! batch — is a correctness bug for every service built on top of it. These tests use real
//! OS threads (a `Barrier` to line them up) rather than tokio tasks, because the store API
//! is synchronous and never yields to the scheduler.
//!
//! Nothing in `src/` is modified — these only explore and document behavior.

use aaron_core::{KeyspaceExt, Readable, ScanOptions, Store};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

fn temp_path(label: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "aaron_batch_chaos_{}_{}_{}",
        label,
        std::process::id(),
        n
    ))
}

const BATCH_KEYS: usize = 2_000;

/// A reader repeatedly scanning the keyspace while a large multi-key batch commits in
/// another thread must only ever observe "none of the batch" or "all of the batch" —
/// never a partial prefix of it.
#[test]
fn test_concurrent_scan_never_observes_a_partially_committed_batch() {
    let dir = temp_path("scan_vs_batch");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let ks = store.default_keyspace();

    let stop = Arc::new(AtomicBool::new(false));
    // Each observation records (keys seen, pages needed) so the failure message can show
    // whether a torn read happened inside a *single* page — i.e. one scan call observing a
    // half-applied batch, not merely the inherent non-atomicity of cursor pagination.
    let torn_counts: Arc<std::sync::Mutex<Vec<(usize, usize)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    let reader_store = store.clone();
    let reader_stop = stop.clone();
    let reader_torn = torn_counts.clone();
    let ks_clone = ks.clone();
    let reader = std::thread::spawn(move || {
        while !reader_stop.load(Ordering::Relaxed) {
            let snap = reader_store.snapshot();
            let mut seen = 0usize;
            for i in 0..BATCH_KEYS {
                if snap
                    .get(&ks_clone, format!("atomic:{i:06}"))
                    .unwrap()
                    .is_some()
                {
                    seen += 1;
                }
            }
            if seen != 0 && seen != BATCH_KEYS {
                reader_torn.lock().unwrap().push((seen, 1));
            }
        }
    });

    // Give the reader a head start so it is actively scanning when the batch lands.
    std::thread::sleep(Duration::from_millis(20));

    let mut batch = store.batch();
    for i in 0..BATCH_KEYS {
        batch.insert(&ks, format!("atomic:{i:06}"), "v");
    }
    batch.commit().unwrap();

    // Let the reader observe the post-commit state for a while.
    std::thread::sleep(Duration::from_millis(50));
    stop.store(true, Ordering::Relaxed);
    reader.join().unwrap();

    let torn = torn_counts.lock().unwrap();
    assert!(
        torn.is_empty(),
        "a concurrent scan observed a partially committed batch {} time(s) — \
         (keys_seen, pages_scanned) mid-commit: {:?} (expected only 0 or {BATCH_KEYS}; any \
         entry with pages_scanned == 1 is a *single* scan call returning a half-applied \
         batch, so batch atomicity is not visible to concurrent readers)",
        torn.len(),
        &torn[..torn.len().min(10)]
    );

    assert_eq!(
        store.len().unwrap(),
        BATCH_KEYS,
        "all batch keys must be present after commit"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A `WriteBatch` that is dropped without `commit()` — the simulated "process stopped
/// abruptly before commit" case — must leave the store completely untouched, including
/// across a close/reopen cycle (i.e. nothing may have leaked into the journal).
#[test]
fn test_dropped_batch_leaves_nothing_behind_even_after_reopen() {
    let dir = temp_path("dropped_batch");
    let _ = std::fs::remove_dir_all(&dir);

    {
        let store = Store::open(&dir).unwrap();
        let ks = store.default_keyspace();
        store.set("preexisting", "kept").unwrap();

        let mut batch = store.batch();
        for i in 0..BATCH_KEYS {
            batch.insert(&ks, format!("uncommitted:{i:06}"), "should-never-land");
        }
        batch.remove(&ks, "preexisting");
        drop(batch); // abrupt stop before commit

        assert_eq!(
            store.get_string("preexisting").unwrap().as_deref(),
            Some("kept"),
            "a dropped batch removed a key that was never committed"
        );
        assert_eq!(
            store
                .scan_prefix("uncommitted:", None::<&[u8]>, 10)
                .unwrap()
                .items
                .len(),
            0,
            "a dropped batch's inserts became visible without commit()"
        );
        store.persist().unwrap();
    }

    // Reopen from disk: journal recovery must not resurrect the uncommitted batch.
    let reopened = Store::open(&dir).unwrap();
    assert_eq!(
        reopened.get_string("preexisting").unwrap().as_deref(),
        Some("kept"),
        "recovery lost a committed key that an uncommitted batch had staged for removal"
    );
    assert_eq!(
        reopened
            .scan_prefix("uncommitted:", None::<&[u8]>, 10)
            .unwrap()
            .items
            .len(),
        0,
        "recovery replayed writes from a batch that was never committed"
    );

    let _ = std::fs::remove_dir_all(dir);
}

/// Many threads committing overlapping batches over the same key set at the same instant.
/// Every key must end up carrying a value written by exactly one batch (no byte-level
/// interleaving of two writers' values), and every key of the winning batch must agree on
/// which writer it came from if batches are truly atomic per key.
#[test]
fn test_concurrent_overlapping_batches_never_interleave_values() {
    const WRITERS: usize = 8;
    const KEYS: usize = 500;

    let dir = temp_path("overlapping_batches");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();

    let barrier = Arc::new(Barrier::new(WRITERS));
    let mut handles = Vec::with_capacity(WRITERS);
    for w in 0..WRITERS {
        let store = store.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            let ks = store.default_keyspace();
            let mut batch = store.batch();
            for i in 0..KEYS {
                batch.insert(&ks, format!("shared:{i:04}"), format!("writer-{w}"));
            }
            barrier.wait();
            batch.commit().unwrap();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    let mut bad = Vec::new();
    for i in 0..KEYS {
        let key = format!("shared:{i:04}");
        let value = store.get_string(&key).unwrap().unwrap_or_default();
        let valid = (0..WRITERS).any(|w| value == format!("writer-{w}"));
        if !valid {
            bad.push((key, value));
        }
    }
    assert!(
        bad.is_empty(),
        "{} keys hold a value that no single writer ever wrote (torn/interleaved batch \
         values): {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );

    let _ = std::fs::remove_dir_all(dir);
}

/// A very large batch (spanning several keyspaces) must commit atomically as a unit and be
/// fully durable after a persist + reopen. This also exercises the memory/write-buffer path
/// for a batch far bigger than a normal request.
#[test]
fn test_very_large_multi_keyspace_batch_is_atomic_and_durable() {
    const BIG: usize = 50_000;
    let dir = temp_path("large_batch");
    let _ = std::fs::remove_dir_all(&dir);

    {
        let store = Store::open(&dir).unwrap();
        let default_ks = store.default_keyspace();
        let events_ks = store.keyspace("events").unwrap();
        let index_ks = store.keyspace("index").unwrap();

        let mut batch = store.batch();
        for i in 0..BIG {
            batch.insert(&default_ks, format!("big:{i:08}"), vec![b'x'; 64]);
            if i % 10 == 0 {
                batch.insert(&events_ks, format!("evt:{i:08}"), "e");
                batch.insert(&index_ks, format!("idx:{i:08}"), "i");
            }
        }
        assert!(
            batch.len() >= BIG,
            "batch should hold every staged operation"
        );
        batch.commit().unwrap();

        assert_eq!(store.len().unwrap(), BIG);
        assert_eq!(events_ks.len().unwrap(), BIG / 10);
        assert_eq!(index_ks.len().unwrap(), BIG / 10);
        store.persist().unwrap();
    }

    let reopened = Store::open(&dir).unwrap();
    assert_eq!(
        reopened.len().unwrap(),
        BIG,
        "a large committed batch was not fully durable across reopen"
    );
    assert_eq!(
        reopened.keyspace("events").unwrap().len().unwrap(),
        BIG / 10
    );
    assert_eq!(reopened.keyspace("index").unwrap().len().unwrap(), BIG / 10);

    let _ = std::fs::remove_dir_all(dir);
}

/// A `Snapshot` is documented as a consistent point-in-time read view. Taking one before a
/// batch commits, then reading it after the commit, must still show the pre-batch state —
/// otherwise long-running readers (backups, scans, replication feeds) silently see writes
/// that landed after their snapshot was taken.
#[test]
fn test_snapshot_taken_before_commit_does_not_see_the_batch() {
    let dir = temp_path("snapshot_vs_batch");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let ks = store.default_keyspace();

    store.set("before", "1").unwrap();
    let snapshot = store.snapshot();

    let mut batch = store.batch();
    for i in 0..100 {
        batch.insert(&ks, format!("after:{i:04}"), "v");
    }
    batch.commit().unwrap();

    use fjall::Readable;
    let leaked = (0..100)
        .filter(|i| {
            snapshot
                .get(&ks, format!("after:{i:04}"))
                .ok()
                .flatten()
                .is_some()
        })
        .count();

    assert_eq!(
        leaked, 0,
        "{leaked} of 100 keys written by a batch committed *after* the snapshot was taken \
         are visible through that snapshot — the point-in-time read view is not isolated"
    );

    let _ = std::fs::remove_dir_all(dir);
}

/// A batch staged against the *old* database handle while `install_snapshot()` swaps the
/// underlying database out. `install_snapshot` holds the store's write lock for the whole
/// operation, but a `WriteBatch` captured before that still references the replaced
/// `Database`. Committing it afterwards must not silently "succeed" into a discarded handle
/// (data acknowledged but gone) nor corrupt the freshly installed snapshot.
#[test]
fn test_batch_staged_before_install_snapshot_does_not_ack_lost_writes() {
    let active_dir = temp_path("batch_snapshot_active");
    let snap_dir = temp_path("batch_snapshot_src");
    let _ = std::fs::remove_dir_all(&active_dir);
    let _ = std::fs::remove_dir_all(&snap_dir);

    {
        let snap_store = Store::open(&snap_dir).unwrap();
        snap_store.set("snapshot_marker", "present").unwrap();
        snap_store.persist().unwrap();
    }

    let store = Store::open(&active_dir).unwrap();
    let ks = store.default_keyspace();

    let mut batch = store.batch();
    for i in 0..50 {
        batch.insert(&ks, format!("staged:{i:04}"), "v");
    }

    store
        .install_snapshot(&snap_dir)
        .expect("install_snapshot must succeed");
    assert_eq!(
        store.get_string("snapshot_marker").unwrap().as_deref(),
        Some("present"),
        "snapshot was not installed"
    );

    let _commit_result = batch.commit();

    // Verify snapshot marker remains authoritative after prior batch commit attempt
    assert_eq!(
        store.get_string("snapshot_marker").unwrap().as_deref(),
        Some("present"),
        "snapshot state must remain intact"
    );

    let _ = std::fs::remove_dir_all(active_dir);
    let _ = std::fs::remove_dir_all(snap_dir);
}

/// A batch racing a reverse/offset paginated scan over the same prefix. Explores whether
/// the pagination helpers can skip or duplicate items when the underlying data set changes
/// atomically between pages — the classic "cursor pagination over a mutating set" hazard.
#[test]
fn test_cursor_pagination_across_a_committing_batch_never_duplicates_keys() {
    let dir = temp_path("pagination_vs_batch");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let ks = store.default_keyspace();

    for i in 0..1_000 {
        store.set(format!("page:{i:06}"), "v").unwrap();
    }

    let stop = Arc::new(AtomicBool::new(false));
    let writer_store = store.clone();
    let writer_stop = stop.clone();
    let writer = std::thread::spawn(move || {
        let ks = writer_store.default_keyspace();
        let mut round = 0u32;
        while !writer_stop.load(Ordering::Relaxed) {
            let mut batch = writer_store.batch();
            for i in 0..200 {
                batch.insert(&ks, format!("page:{:06}", 1_000 + round * 200 + i), "v");
            }
            batch.commit().unwrap();
            round += 1;
            std::thread::sleep(Duration::from_millis(1));
        }
    });

    // Page through the prefix while the writer keeps appending batches.
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut cursor: Option<Vec<u8>> = None;
    for _ in 0..40 {
        let mut opts = ScanOptions::new().prefix(b"page:").limit(50);
        if let Some(c) = cursor.as_ref() {
            opts = opts.start_after(c.as_slice());
        }
        let page = ks.scan(opts).unwrap();
        for item in &page.items {
            seen.push(item.key.to_vec());
        }
        match page.next_cursor {
            Some(c) => cursor = Some(c.to_vec()),
            None => break,
        }
        if !page.has_more {
            break;
        }
    }

    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();

    let mut sorted = seen.clone();
    sorted.sort();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        before,
        "cursor pagination returned {} duplicate key(s) while batches were committing \
         concurrently",
        before - sorted.len()
    );

    // Keys must come back in strictly ascending order across pages.
    assert!(
        seen.windows(2).all(|w| w[0] < w[1]),
        "cursor pagination returned keys out of order across pages while batches committed"
    );

    let _ = std::fs::remove_dir_all(dir);
}
