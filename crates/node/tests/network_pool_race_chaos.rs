//! Exploratory chaos scenarios for the TCP/QUIC connection pools beyond the existing
//! "same peer, all connecting" concurrency test — interleaving connect() with disconnect()
//! and racing connect() against a server that behaves inconsistently, to see whether the
//! pool can end up in a state that doesn't self-heal.

use node::Network;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Interleaves `connect()` and `disconnect()` calls to the same peer from many concurrent
/// tasks. Regardless of interleaving order, the pool must never end up holding more than
/// one entry for a single peer, and a fresh `connect()` afterwards must still succeed.
#[tokio::test]
async fn test_tcp_connect_disconnect_interleave_stays_consistent() {
    let network = Network::new();
    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            // Hold each accepted connection open briefly so racing connect() calls have
            // something live to reuse from the pool before it's torn down.
            tokio::spawn(async move {
                let _socket = socket;
                tokio::time::sleep(Duration::from_millis(150)).await;
            });
        }
    });

    let mut handles = Vec::new();
    for i in 0..40 {
        let net = network.clone();
        handles.push(tokio::spawn(async move {
            if i % 2 == 0 {
                let _ = net.tcp.connect(server_addr).await;
            } else {
                net.tcp.disconnect(&server_addr).await;
            }
        }));
    }
    for h in handles {
        let _ = h.await;
    }

    let count = network.tcp.pool().count().await;
    assert!(
        count <= 1,
        "pool must never hold more than one entry for a single peer after concurrent \
         connect()/disconnect() interleaving, found {count}"
    );

    // The pool must self-heal: a fresh connect() after the chaos still succeeds cleanly.
    let conn = network
        .tcp
        .connect(server_addr)
        .await
        .expect("connect after interleave chaos must still succeed");
    assert_eq!(conn.peer_addr(), server_addr);

    server_handle.abort();
}

/// A thundering herd of `connect()` calls against a server that accepts every other
/// connection and immediately drops the rest. Explores whether `get_or_insert`'s
/// close-the-loser-and-reuse-the-winner logic ever leaves the pool pointing at a
/// connection that was actually the dropped one, or leaks a live connection outside
/// the pool that nothing can ever `disconnect()`.
#[tokio::test]
async fn test_quic_thundering_herd_against_flaky_accept_server() {
    let network = Network::new();
    let server_endpoint = network.quic.listen("127.0.0.1:0").await.unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    let accepted_count = Arc::new(AtomicUsize::new(0));
    let accepted_count_server = accepted_count.clone();
    let server_handle = tokio::spawn(async move {
        let mut i = 0usize;
        while let Some(incoming) = server_endpoint.accept().await {
            i += 1;
            if i.is_multiple_of(2) {
                // Reject every other incoming connection outright.
                incoming.refuse();
                continue;
            }
            let accepted_count_server = accepted_count_server.clone();
            tokio::spawn(async move {
                if let Ok(conn) = incoming.await {
                    accepted_count_server.fetch_add(1, Ordering::Relaxed);
                    let _ = conn.closed().await;
                }
            });
        }
    });

    let mut handles = Vec::new();
    for _ in 0..30 {
        let net = network.clone();
        handles.push(tokio::spawn(async move {
            net.quic.connect(server_addr, "localhost").await
        }));
    }

    let mut successes = 0;
    for h in handles {
        if h.await.unwrap().is_ok() {
            successes += 1;
        }
    }

    // At least some connects must succeed despite the flaky server (the pool/retry path
    // must not collapse entirely under partial rejection).
    assert!(
        successes > 0,
        "every concurrent connect() failed against a partially-accepting server"
    );

    // The pool must never accumulate more than one live entry for this single peer.
    let pool_count = network.quic.pool().count().await;
    assert!(
        pool_count <= 1,
        "QUIC pool must hold at most one entry for a single peer, found {pool_count}"
    );

    // A subsequent connect() must still work cleanly — no permanently wedged pool entry.
    let conn = network.quic.connect(server_addr, "localhost").await;
    assert!(
        conn.is_ok(),
        "connect() after flaky-server chaos must still eventually succeed: {:?}",
        conn.err()
    );

    server_handle.abort();
}
