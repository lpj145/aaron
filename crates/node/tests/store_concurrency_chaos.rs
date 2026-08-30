//! Chaos/exploratory tests for `Store` under genuine concurrent access.
//!
//! These deliberately use real OS threads (`std::thread::spawn` + `Barrier`) rather than
//! `tokio::spawn` on a single-threaded runtime: a synchronous, non-`.await`-ing function
//! like `Store::update`/`KeyspaceExt::update` never yields to the tokio scheduler, so on a
//! `current_thread` runtime concurrent tasks calling it never actually interleave — which is
//! why a prior "concurrent RMW" test could spawn 50 tasks and still never observe a race.
//! A `Barrier` lines every thread up to call the racy section at (as close to) the same
//! instant as possible, maximizing the chance of exposing a real data race.
//!
//! None of these tests fix anything in `src/` — they only explore/document behavior.

use node::{KeyspaceExt, Store};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

fn temp_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aaron_chaos_{}_{}_{}",
        label,
        std::process::id(),
        fastrand_seed()
    ))
}

// Tiny local counter to keep temp dir names unique across tests in the same process
// without pulling in an extra dependency.
fn fastrand_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) as u64;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    now ^ n
}

/// `Store::update` is documented as "does not hold cross-thread table locks". This test
/// hammers the same counter key from many real OS threads released simultaneously via a
/// `Barrier`, and asserts the final value equals the number of increments performed —
/// the property any caller relying on RMW semantics actually needs.
#[test]
fn test_store_update_loses_increments_under_true_thread_race() {
    let dir = temp_path("store_update_race");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();

    const THREADS: usize = 64;
    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::with_capacity(THREADS);

    for _ in 0..THREADS {
        let store = store.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store
                .update("race_counter", |curr| {
                    let n = match curr {
                        Some(s) => std::str::from_utf8(&s).unwrap().parse::<u64>().unwrap(),
                        None => 0,
                    };
                    Some(format!("{}", n + 1))
                })
                .unwrap();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    let final_value: u64 = store
        .get_string("race_counter")
        .unwrap()
        .expect("counter key must exist")
        .parse()
        .unwrap();

    assert_eq!(
        final_value,
        THREADS as u64,
        "Store::update lost {} of {} concurrent increments under a true multi-thread race \
         (get-then-insert is not atomic; two threads can read the same value before either writes it back)",
        THREADS as u64 - final_value,
        THREADS
    );

    let _ = std::fs::remove_dir_all(dir);
}

/// Same race, exercised directly against a named `Keyspace` via `KeyspaceExt::update`
/// rather than through `Store`'s default-keyspace convenience wrapper, to confirm the
/// race lives in the shared `KeyspaceExt` implementation itself, not in `Store`'s plumbing.
#[test]
fn test_keyspace_ext_update_loses_increments_under_true_thread_race() {
    let dir = temp_path("keyspace_update_race");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let ks = store.keyspace("stats").unwrap();

    const THREADS: usize = 64;
    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::with_capacity(THREADS);

    for _ in 0..THREADS {
        let ks = ks.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            ks.update("requests", |curr| {
                let n = curr.map_or(0u64, |s| {
                    std::str::from_utf8(&s).unwrap().parse::<u64>().unwrap()
                });
                Some(format!("{}", n + 1))
            })
            .unwrap();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    let final_value: u64 = ks
        .get_string("requests")
        .unwrap()
        .expect("key must exist")
        .parse()
        .unwrap();

    assert_eq!(
        final_value,
        THREADS as u64,
        "KeyspaceExt::update lost {} of {} concurrent increments (get() and insert()/remove() \
         each take their own short-lived internal lock, but nothing spans the gap between them)",
        THREADS as u64 - final_value,
        THREADS
    );

    let _ = std::fs::remove_dir_all(dir);
}

/// `install_snapshot` only briefly holds the write lock to swap `db`/`default_keyspace`
/// handles; the actual `remove_dir_all` + `copy_dir_all` + reopen happens with the lock
/// released. This explores whether a concurrent writer that receives `Ok(())` from
/// `Store::set` during that window can have its write silently discarded once the
/// temporary/old directory is torn down — i.e. a successful write that never actually lands.
#[test]
fn test_writes_confirmed_during_install_snapshot_window_can_vanish_silently() {
    let active_dir = temp_path("snapshot_window_active");
    let snap_dir = temp_path("snapshot_window_src");
    let _ = std::fs::remove_dir_all(&active_dir);
    let _ = std::fs::remove_dir_all(&snap_dir);

    {
        let snap_store = Store::open(&snap_dir).unwrap();
        snap_store.set("snapshot_marker", "present").unwrap();
        snap_store.persist().unwrap();
    }

    let store = Store::open(&active_dir).unwrap();
    let confirmed_keys: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));

    let writer_store = store.clone();
    let writer_confirmed = confirmed_keys.clone();
    let writer_stop = stop.clone();
    let writer = std::thread::spawn(move || {
        let mut i: u64 = 0;
        while !writer_stop.load(Ordering::Relaxed) {
            let key = format!("live_write_{i}");
            // Only record keys the Store itself claims were written successfully.
            if writer_store.set(&key, "value").is_ok() {
                writer_confirmed.lock().unwrap().push(key);
            }
            i += 1;
        }
    });

    // Trigger snapshot concurrently while writer thread is active
    store
        .install_snapshot(&snap_dir)
        .expect("install_snapshot must succeed");
    std::thread::sleep(Duration::from_millis(15));
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();

    let confirmed = confirmed_keys.lock().unwrap();
    assert!(
        !confirmed.is_empty(),
        "writer thread must have completed at least one write during the race"
    );

    let lost: Vec<&String> = confirmed
        .iter()
        .filter(|k| store.get(k.as_str()).unwrap().is_none())
        .collect();

    assert!(
        lost.is_empty(),
        "{} of {} writes that Store::set() reported Ok() for vanished after install_snapshot() \
         (install_snapshot never blocks concurrent writers, so a write can land in a handle that \
         gets torn down moments later, with no error ever surfaced to the writer)",
        lost.len(),
        confirmed.len()
    );

    let _ = std::fs::remove_dir_all(active_dir);
    let _ = std::fs::remove_dir_all(snap_dir);
}

/// Two callers racing to install two *different* snapshots into the same `Store` at the
/// same instant. `install_snapshot` clears and repopulates `self.path` outside of any lock
/// held for the whole operation, so this explores whether the directory can end up as a
/// corrupted mix of both snapshots, or in a state the database can no longer reopen from.
#[test]
fn test_concurrent_install_snapshot_calls_do_not_corrupt_the_store() {
    let active_dir = temp_path("dual_snapshot_active");
    let snap_a_dir = temp_path("dual_snapshot_a");
    let snap_b_dir = temp_path("dual_snapshot_b");
    let _ = std::fs::remove_dir_all(&active_dir);
    let _ = std::fs::remove_dir_all(&snap_a_dir);
    let _ = std::fs::remove_dir_all(&snap_b_dir);

    {
        let s = Store::open(&snap_a_dir).unwrap();
        s.set("which_snapshot", "A").unwrap();
        s.persist().unwrap();
    }
    {
        let s = Store::open(&snap_b_dir).unwrap();
        s.set("which_snapshot", "B").unwrap();
        s.persist().unwrap();
    }

    let store = Store::open(&active_dir).unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let store_a = store.clone();
    let barrier_a = barrier.clone();
    let snap_a = snap_a_dir.clone();
    let t_a = std::thread::spawn(move || {
        barrier_a.wait();
        store_a.install_snapshot(&snap_a)
    });

    let store_b = store.clone();
    let barrier_b = barrier.clone();
    let snap_b = snap_b_dir.clone();
    let t_b = std::thread::spawn(move || {
        barrier_b.wait();
        store_b.install_snapshot(&snap_b)
    });

    let res_a = t_a.join().unwrap();
    let res_b = t_b.join().unwrap();

    // At least one racer should be able to complete cleanly.
    assert!(
        res_a.is_ok() || res_b.is_ok(),
        "both concurrent install_snapshot calls failed: a={:?}, b={:?}",
        res_a.err(),
        res_b.err()
    );

    // Whatever the outcome, the store must land on exactly one of the two snapshots —
    // never a torn/corrupted mix, and never a value that reads back as neither.
    let final_value = store.get_string("which_snapshot").unwrap();
    assert!(
        matches!(final_value.as_deref(), Some("A") | Some("B")),
        "store ended up in an inconsistent state after racing install_snapshot calls: {:?} \
         (errors: a={:?}, b={:?})",
        final_value,
        res_a.err(),
        res_b.err()
    );

    let _ = std::fs::remove_dir_all(active_dir);
    let _ = std::fs::remove_dir_all(snap_a_dir);
    let _ = std::fs::remove_dir_all(snap_b_dir);
}
