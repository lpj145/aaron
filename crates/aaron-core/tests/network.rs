use aaron_core::Network;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn test_tcp_listen_connect_and_pool_reuse() {
    let network = Network::new();

    // 1. Inbound: Bind a TCP listener on an ephemeral port
    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    // Spawn server accept task
    let server_handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 5];
        socket.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping!");

        socket.write_all(b"pong!").await.unwrap();
    });

    // 2. Outbound: Connect via TcpManager with automatic pooling
    let conn1 = network.tcp.connect(server_addr).await.unwrap();
    assert_eq!(conn1.peer_addr(), server_addr);

    // Initial pool count should be 1
    assert_eq!(network.tcp.pool().count().await, 1);

    // 3. Connect again to the exact same address — should reuse conn1 from pool!
    let conn2 = network.tcp.connect(server_addr).await.unwrap();
    assert_eq!(network.tcp.pool().count().await, 1);
    assert_eq!(conn2.peer_addr(), server_addr);

    // Write ping and read pong
    conn1.write_all(b"ping!").await.unwrap();

    let mut resp = [0u8; 5];
    conn2.read_exact(&mut resp).await.unwrap();
    assert_eq!(&resp, b"pong!");

    server_handle.await.unwrap();

    // 4. Disconnect removes from pool
    let removed = network.tcp.disconnect(&server_addr).await;
    assert!(removed.is_some());
    assert_eq!(network.tcp.pool().count().await, 0);
}

#[tokio::test]
async fn test_tcp_stale_connection_auto_prune_and_reconnect() {
    let network = Network::new();

    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    // Server accepts one connection and immediately closes it
    let server_handle = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        drop(socket); // Close connection from server side

        // Accept second connection
        let (mut socket2, _) = listener.accept().await.unwrap();
        socket2.write_all(b"reconnected").await.unwrap();
    });

    let conn1 = network.tcp.connect(server_addr).await.unwrap();
    assert_eq!(network.tcp.pool().count().await, 1);

    // Read will see EOF (0 bytes) and mark connection closed
    let mut buf = [0u8; 10];
    let n = conn1.read(&mut buf).await.unwrap();
    assert_eq!(n, 0);
    assert!(conn1.is_closed());

    // Next connect should detect conn1 is closed, prune it, and connect cleanly
    let conn2 = network.tcp.connect(server_addr).await.unwrap();
    assert!(!conn2.is_closed());

    let mut resp = [0u8; 11];
    conn2.read_exact(&mut resp).await.unwrap();
    assert_eq!(&resp, b"reconnected");

    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_tcp_multiple_distinct_peers_pooling() {
    let network = Network::new();

    // Create 2 independent TCP servers
    let listener_a = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let addr_a = listener_a.local_addr().unwrap();

    let listener_b = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let addr_b = listener_b.local_addr().unwrap();

    let h_a = tokio::spawn(async move {
        let mut sockets = Vec::new();
        while let Ok((socket, _)) = listener_a.accept().await {
            sockets.push(socket);
        }
    });
    let h_b = tokio::spawn(async move {
        let mut sockets = Vec::new();
        while let Ok((socket, _)) = listener_b.accept().await {
            sockets.push(socket);
        }
    });

    // Connect to peer A
    let _conn_a = network.tcp.connect(addr_a).await.unwrap();
    assert_eq!(network.tcp.pool().count().await, 1);

    // Connect to peer B
    let _conn_b = network.tcp.connect(addr_b).await.unwrap();
    assert_eq!(network.tcp.pool().count().await, 2);

    let peers = network.tcp.pool().peer_addrs().await;
    assert!(peers.contains(&addr_a));
    assert!(peers.contains(&addr_b));

    // Clear pool
    network.tcp.pool().clear().await;
    assert_eq!(network.tcp.pool().count().await, 0);

    h_a.abort();
    h_b.abort();
}

#[tokio::test]
async fn test_tcp_full_duplex_split_reader_writer() {
    let network = Network::new();

    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    // Server echoes back whatever it receives
    let server_handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let (mut reader, mut writer) = socket.split();
        let mut buf = [0u8; 10];
        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n]).await.unwrap();
        }
    });

    let conn = network.tcp.connect(server_addr).await.unwrap();
    let (reader, writer) = conn.split();

    // Reader task awaits incoming packets without blocking writer
    let reader_handle = tokio::spawn(async move {
        let mut received = Vec::new();
        for _ in 0..5 {
            let mut buf = [0u8; 4];
            reader.read_exact(&mut buf).await.unwrap();
            received.push(buf);
        }
        received
    });

    // Writer task sends 5 messages concurrently
    for i in 0..5 {
        writer.write_all(&[i, i + 1, i + 2, i + 3]).await.unwrap();
    }

    let received = reader_handle.await.unwrap();
    assert_eq!(received.len(), 5);
    assert_eq!(received[0], [0, 1, 2, 3]);
    assert_eq!(received[4], [4, 5, 6, 7]);

    server_handle.abort();
}

#[tokio::test]
async fn test_tcp_concurrent_connect_calls_same_peer() {
    let network = Network::new();

    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let mut sockets = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            sockets.push(socket);
        }
    });

    // 20 concurrent tasks calling connect to same peer
    let mut handles = Vec::new();
    for _ in 0..20 {
        let net = network.clone();
        handles.push(tokio::spawn(async move {
            net.tcp.connect(server_addr).await.unwrap()
        }));
    }

    for h in handles {
        let conn = h.await.unwrap();
        assert_eq!(conn.peer_addr(), server_addr);
    }

    // Pool count should be 1
    assert_eq!(network.tcp.pool().count().await, 1);

    server_handle.abort();
}

#[tokio::test]
async fn test_tcp_reconnect_after_disconnect() {
    let network = Network::new();

    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let mut sockets = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            sockets.push(socket);
        }
    });

    // Connect first time
    let _conn1 = network.tcp.connect(server_addr).await.unwrap();
    assert_eq!(network.tcp.pool().count().await, 1);

    // Disconnect
    network.tcp.disconnect(&server_addr).await;
    assert_eq!(network.tcp.pool().count().await, 0);

    // Connect second time -> re-establishes and registers new connection
    let _conn2 = network.tcp.connect(server_addr).await.unwrap();
    assert_eq!(network.tcp.pool().count().await, 1);

    server_handle.abort();
}

#[tokio::test]
async fn test_tcp_large_payload_and_flush() {
    let network = Network::new();

    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let large_payload: Vec<u8> = (0..32768).map(|i| (i % 256) as u8).collect();
    let expected = large_payload.clone();

    let server_handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 32768];
        socket.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, expected);

        socket.write_all(&buf).await.unwrap();
    });

    let conn = network.tcp.connect(server_addr).await.unwrap();
    conn.write_all(&large_payload).await.unwrap();
    conn.flush().await.unwrap();

    let mut response = vec![0u8; 32768];
    conn.read_exact(&mut response).await.unwrap();
    assert_eq!(response, large_payload);

    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_tcp_manual_mark_closed_and_pool_recovery() {
    let network = Network::new();

    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let (mut s1, _) = listener.accept().await.unwrap();
        s1.write_all(b"first").await.unwrap();

        let (mut s2, _) = listener.accept().await.unwrap();
        s2.write_all(b"second").await.unwrap();
    });

    let conn1 = network.tcp.connect(server_addr).await.unwrap();
    let mut buf = [0u8; 5];
    conn1.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"first");

    // Manually mark connection closed
    conn1.mark_closed();
    assert!(conn1.is_closed());

    // Pool should recognize it is closed and connect new socket
    let conn2 = network.tcp.connect(server_addr).await.unwrap();
    assert!(!conn2.is_closed());

    let mut buf2 = [0u8; 6];
    conn2.read_exact(&mut buf2).await.unwrap();
    assert_eq!(&buf2, b"second");

    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_tcp_writer_shutdown_clean_eof() {
    let network = Network::new();

    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 8];
        let n = socket.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"shutdown");

        // Wait for client half-close EOF
        let n = socket.read(&mut buf).await.unwrap();
        assert_eq!(n, 0);
    });

    let conn = network.tcp.connect(server_addr).await.unwrap();
    let writer = conn.writer();
    writer.write_all(b"shutdown").await.unwrap();
    writer.shutdown().await.unwrap();
    assert!(writer.is_closed());

    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_udp_bind_and_datagram_exchange() {
    let network = Network::new();

    // Bind two local UDP sockets
    let socket_a = network.udp.bind("127.0.0.1:0").await.unwrap();
    let addr_a = socket_a.local_addr().unwrap();

    let socket_b = network.udp.bind("127.0.0.1:0").await.unwrap();
    let addr_b = socket_b.local_addr().unwrap();

    assert_eq!(network.udp.count().await, 2);

    // Send datagram from A to B
    socket_a.send_to(b"hello udp", addr_b).await.unwrap();

    let mut buf = [0u8; 32];
    let (n, sender) = socket_b.recv_from(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"hello udp");
    assert_eq!(sender, addr_a);

    // get_or_bind reuses existing socket for addr_a
    let socket_a_reused = network.udp.get_or_bind(addr_a).await.unwrap();
    assert!(Arc::ptr_eq(&socket_a, &socket_a_reused));

    // Unbind
    network.udp.unbind(&addr_a).await;
    assert_eq!(network.udp.count().await, 1);
}

#[tokio::test]
async fn test_udp_multi_socket_broadcast_and_burst() {
    let network = Network::new();

    let sender = network.udp.bind("127.0.0.1:0").await.unwrap();
    let sender_addr = sender.local_addr().unwrap();

    let mut receivers = Vec::new();
    let mut receiver_addrs = Vec::new();

    for _ in 0..4 {
        let sock = network.udp.bind("127.0.0.1:0").await.unwrap();
        receiver_addrs.push(sock.local_addr().unwrap());
        receivers.push(sock);
    }

    assert_eq!(network.udp.count().await, 5);

    // Broadcast message to all 4 receivers
    for &addr in &receiver_addrs {
        sender.send_to(b"broadcast_burst", addr).await.unwrap();
    }

    for rx in receivers {
        let mut buf = [0u8; 32];
        let (n, from) = rx.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"broadcast_burst");
        assert_eq!(from, sender_addr);
    }
}

#[tokio::test]
async fn test_udp_rebind_after_unbind() {
    let network = Network::new();

    let socket1 = network.udp.bind("127.0.0.1:0").await.unwrap();
    let addr = socket1.local_addr().unwrap();
    assert_eq!(network.udp.count().await, 1);

    // Unbind
    let removed = network.udp.unbind(&addr).await;
    assert!(removed.is_some());
    assert_eq!(network.udp.count().await, 0);

    // Drop previous socket handles so OS releases the bound port
    drop(socket1);
    drop(removed);

    // Re-bind using get_or_bind with same port address
    let socket2 = network.udp.get_or_bind(addr).await.unwrap();
    assert_eq!(socket2.local_addr().unwrap(), addr);
    assert_eq!(network.udp.count().await, 1);
}
