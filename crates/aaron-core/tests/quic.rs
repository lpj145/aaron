use aaron_core::{Network, Uuid, generate_node_cert, generate_self_signed_cert};

#[tokio::test]
async fn test_quic_self_signed_cert_generation() {
    let (cert, key) =
        generate_self_signed_cert(vec!["localhost".to_string(), "node1".to_string()]).unwrap();
    assert!(!cert.is_empty());
    assert!(!key.secret_der().is_empty());
}

#[tokio::test]
async fn test_quic_node_uuid_bound_lifecycle() {
    let network = Network::new();
    let node_uuid = Uuid::new(0x1234_5678, 0x9abc_def0);

    // 1. Generate node certificate bound to Uuid
    let (cert, key) = generate_node_cert(node_uuid).unwrap();
    assert!(!cert.is_empty());
    assert!(!key.secret_der().is_empty());

    // 2. Start server for this specific node
    let server_endpoint = network
        .quic
        .listen_for_node("127.0.0.1:0", node_uuid)
        .await
        .unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let incoming = server_endpoint.accept().await.unwrap();
        let connection = incoming.await.unwrap();
        let (mut send, mut recv) = connection.accept_bi().await.unwrap();

        let mut buf = [0u8; 4];
        recv.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"node");

        send.write_all(b"auth").await.unwrap();
        send.finish().unwrap();

        let _ = connection.closed().await;
    });

    // 3. Connect to node using its UUID
    let conn = network
        .quic
        .connect_node(server_addr, node_uuid)
        .await
        .unwrap();
    assert_eq!(network.quic.pool().count().await, 1);

    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    send.write_all(b"node").await.unwrap();
    send.finish().unwrap();

    let mut buf = [0u8; 4];
    recv.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"auth");

    conn.close(0u32.into(), b"done");
    let _ = server_handle.await;
}

#[tokio::test]
async fn test_quic_listen_connect_and_bi_stream_exchange() {
    let network = Network::new();

    // 1. Inbound: Bind a QUIC endpoint on an ephemeral UDP port with P2P TLS
    let server_endpoint = network.quic.listen("127.0.0.1:0").await.unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    // Spawn server accept task
    let server_handle = tokio::spawn(async move {
        let incoming = server_endpoint.accept().await.unwrap();
        let connection = incoming.await.unwrap();

        let (mut send, mut recv) = connection.accept_bi().await.unwrap();
        let mut buf = [0u8; 9];
        recv.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"quic ping");

        send.write_all(b"quic pong").await.unwrap();
        send.finish().unwrap();

        // Keep connection alive until client closes/disconnects
        let _ = connection.closed().await;
    });

    // 2. Outbound: Connect via QuicManager with P2P Web-of-Trust TLS verification and pooling
    let conn1 = network
        .quic
        .connect(server_addr, "localhost")
        .await
        .unwrap();
    assert_eq!(network.quic.pool().count().await, 1);

    // 3. Connect again to the exact same peer — should reuse conn1 from the pool
    let conn2 = network
        .quic
        .connect(server_addr, "localhost")
        .await
        .unwrap();
    assert_eq!(network.quic.pool().count().await, 1);
    assert_eq!(conn1.stable_id(), conn2.stable_id());

    // 4. Open a multiplexed bi-directional stream over the pooled connection
    let (mut send, mut recv) = conn2.open_bi().await.unwrap();
    send.write_all(b"quic ping").await.unwrap();
    send.finish().unwrap();

    let mut resp = [0u8; 9];
    recv.read_exact(&mut resp).await.unwrap();
    assert_eq!(&resp, b"quic pong");

    // 5. Disconnect removes from pool and closes connection
    let removed = network.quic.disconnect(&server_addr).await;
    assert!(removed.is_some());
    assert_eq!(network.quic.pool().count().await, 0);

    if let Some(conn) = removed {
        conn.close(0u32.into(), b"done");
    }

    let _ = server_handle.await;
}

#[tokio::test]
async fn test_quic_stale_connection_auto_prune_and_reconnect() {
    let network = Network::new();

    let server_endpoint = network.quic.listen("127.0.0.1:0").await.unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    // Server accepts one connection, immediately closes it, then accepts a second connection
    let server_handle = tokio::spawn(async move {
        let incoming1 = server_endpoint.accept().await.unwrap();
        let conn1 = incoming1.await.unwrap();
        conn1.close(1u32.into(), b"server shutdown connection");

        let incoming2 = server_endpoint.accept().await.unwrap();
        let conn2 = incoming2.await.unwrap();
        let (mut send, mut recv) = conn2.accept_bi().await.unwrap();
        let mut buf = [0u8; 6];
        recv.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"re-req");
        send.write_all(b"re-res").await.unwrap();
        send.finish().unwrap();

        let _ = conn2.closed().await;
    });

    let conn1 = network
        .quic
        .connect(server_addr, "localhost")
        .await
        .unwrap();
    assert_eq!(network.quic.pool().count().await, 1);

    // Wait for the close to propagate
    let _ = conn1.closed().await;
    assert!(conn1.close_reason().is_some());

    // Connect again -> pool should detect closed conn1, purge it, and establish a new working connection
    let conn2 = network
        .quic
        .connect(server_addr, "localhost")
        .await
        .unwrap();
    assert!(conn2.close_reason().is_none());

    let (mut send, mut recv) = conn2.open_bi().await.unwrap();
    send.write_all(b"re-req").await.unwrap();
    send.finish().unwrap();

    let mut resp = [0u8; 6];
    recv.read_exact(&mut resp).await.unwrap();
    assert_eq!(&resp, b"re-res");

    conn2.close(0u32.into(), b"done");
    let _ = server_handle.await;
}

#[tokio::test]
async fn test_quic_large_payload_transfer() {
    let network = Network::new();

    let server_endpoint = network.quic.listen("127.0.0.1:0").await.unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    // 64 KB test payload
    let large_data: Vec<u8> = (0..65536).map(|i| (i % 256) as u8).collect();
    let large_data_clone = large_data.clone();

    let server_handle = tokio::spawn(async move {
        let incoming = server_endpoint.accept().await.unwrap();
        let connection = incoming.await.unwrap();

        let (mut send, mut recv) = connection.accept_bi().await.unwrap();
        let received = recv.read_to_end(100_000).await.unwrap();
        assert_eq!(received, large_data_clone);

        send.write_all(&received).await.unwrap();
        send.finish().unwrap();

        let _ = connection.closed().await;
    });

    let conn = network
        .quic
        .connect(server_addr, "localhost")
        .await
        .unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();

    send.write_all(&large_data).await.unwrap();
    send.finish().unwrap();

    let echo = recv.read_to_end(100_000).await.unwrap();
    assert_eq!(echo, large_data);

    conn.close(0u32.into(), b"done");
    let _ = server_handle.await;
}

#[tokio::test]
async fn test_quic_multi_peer_pooling_and_isolation() {
    let network = Network::new();

    let server_a = network.quic.listen("127.0.0.1:0").await.unwrap();
    let addr_a = server_a.local_addr().unwrap();

    let server_b = network.quic.listen("127.0.0.1:0").await.unwrap();
    let addr_b = server_b.local_addr().unwrap();

    let h_a = tokio::spawn(async move {
        let incoming = server_a.accept().await.unwrap();
        let conn = incoming.await.unwrap();
        let (mut send, mut recv) = conn.accept_bi().await.unwrap();
        let mut buf = [0u8; 6];
        recv.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"peer-a");
        send.write_all(b"resp-a").await.unwrap();
        send.finish().unwrap();
        let _ = conn.closed().await;
    });

    let h_b = tokio::spawn(async move {
        let incoming = server_b.accept().await.unwrap();
        let conn = incoming.await.unwrap();
        let (mut send, mut recv) = conn.accept_bi().await.unwrap();
        let mut buf = [0u8; 6];
        recv.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"peer-b");
        send.write_all(b"resp-b").await.unwrap();
        send.finish().unwrap();
        let _ = conn.closed().await;
    });

    let conn_a = network.quic.connect(addr_a, "localhost").await.unwrap();
    let conn_b = network.quic.connect(addr_b, "localhost").await.unwrap();
    assert_eq!(network.quic.pool().count().await, 2);

    let (mut send_a, mut recv_a) = conn_a.open_bi().await.unwrap();
    send_a.write_all(b"peer-a").await.unwrap();
    send_a.finish().unwrap();
    let mut resp_a = [0u8; 6];
    recv_a.read_exact(&mut resp_a).await.unwrap();
    assert_eq!(&resp_a, b"resp-a");

    let (mut send_b, mut recv_b) = conn_b.open_bi().await.unwrap();
    send_b.write_all(b"peer-b").await.unwrap();
    send_b.finish().unwrap();
    let mut resp_b = [0u8; 6];
    recv_b.read_exact(&mut resp_b).await.unwrap();
    assert_eq!(&resp_b, b"resp-b");

    conn_a.close(0u32.into(), b"done");
    conn_b.close(0u32.into(), b"done");

    let _ = tokio::join!(h_a, h_b);
}

#[tokio::test]
async fn test_quic_unidirectional_streams() {
    let network = Network::new();

    let server_endpoint = network.quic.listen("127.0.0.1:0").await.unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let server_handle = tokio::spawn(async move {
        let incoming = server_endpoint.accept().await.unwrap();
        let conn = incoming.await.unwrap();
        let mut recv = conn.accept_uni().await.unwrap();
        let mut buf = [0u8; 8];
        recv.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"uni_data");
        let _ = done_tx.send(());
        let _ = conn.closed().await;
    });

    let conn = network
        .quic
        .connect(server_addr, "localhost")
        .await
        .unwrap();
    let mut send = conn.open_uni().await.unwrap();
    send.write_all(b"uni_data").await.unwrap();
    send.finish().unwrap();

    let _ = done_rx.await;
    conn.close(0u32.into(), b"done");
    let _ = server_handle.await;
}

#[tokio::test]
async fn test_quic_listen_with_custom_cert() {
    let network = Network::new();

    let (cert, key) =
        generate_self_signed_cert(vec!["custom-peer".to_string(), "localhost".to_string()])
            .unwrap();
    let server_endpoint = network
        .quic
        .listen_with_cert("127.0.0.1:0", cert, key)
        .await
        .unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let incoming = server_endpoint.accept().await.unwrap();
        let conn = incoming.await.unwrap();
        let (mut send, mut recv) = conn.accept_bi().await.unwrap();
        let mut buf = [0u8; 6];
        recv.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"custom");
        send.write_all(b"custom_ok").await.unwrap();
        send.finish().unwrap();
        let _ = conn.closed().await;
    });

    let conn = network
        .quic
        .connect(server_addr, "custom-peer")
        .await
        .unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    send.write_all(b"custom").await.unwrap();
    send.finish().unwrap();

    let mut resp = [0u8; 9];
    recv.read_exact(&mut resp).await.unwrap();
    assert_eq!(&resp, b"custom_ok");

    conn.close(0u32.into(), b"done");
    let _ = server_handle.await;
}

#[tokio::test]
async fn test_quic_concurrent_connect_calls_same_peer() {
    let network = Network::new();

    let server_endpoint = network.quic.listen("127.0.0.1:0").await.unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        while let Some(incoming) = server_endpoint.accept().await {
            tokio::spawn(async move {
                if let Ok(conn) = incoming.await {
                    let _ = conn.closed().await;
                }
            });
        }
    });

    // 20 concurrent tasks calling quic.connect to the same peer
    let mut handles = Vec::new();
    for _ in 0..20 {
        let net = network.clone();
        handles.push(tokio::spawn(async move {
            net.quic.connect(server_addr, "localhost").await.unwrap()
        }));
    }

    for h in handles {
        let conn = h.await.unwrap();
        assert_eq!(conn.remote_address(), server_addr);
    }

    // Only 1 physical QUIC connection is registered in the pool
    assert_eq!(network.quic.pool().count().await, 1);

    server_handle.abort();
}

#[tokio::test]
async fn test_quic_multiple_concurrent_bi_streams_over_single_pooled_connection() {
    let network = Network::new();

    let server_endpoint = network.quic.listen("127.0.0.1:0").await.unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let incoming = server_endpoint.accept().await.unwrap();
        let connection = incoming.await.unwrap();

        for _ in 0..5 {
            let (mut send, mut recv) = connection.accept_bi().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 4];
                recv.read_exact(&mut buf).await.unwrap();
                send.write_all(&buf).await.unwrap();
                send.finish().unwrap();
            });
        }

        // Keep server connection alive while client reads
        let _ = connection.closed().await;
    });

    let conn = network
        .quic
        .connect(server_addr, "localhost")
        .await
        .unwrap();
    assert_eq!(network.quic.pool().count().await, 1);

    // Concurrently open 5 bi-directional streams over the same QUIC connection
    let mut tasks = Vec::new();
    for i in 0..5u8 {
        let conn_clone = conn.clone();
        tasks.push(tokio::spawn(async move {
            let (mut send, mut recv) = conn_clone.open_bi().await.unwrap();
            send.write_all(&[i, i + 1, i + 2, i + 3]).await.unwrap();
            send.finish().unwrap();

            let mut resp = [0u8; 4];
            recv.read_exact(&mut resp).await.unwrap();
            assert_eq!(resp, [i, i + 1, i + 2, i + 3]);
        }));
    }

    for t in tasks {
        t.await.unwrap();
    }

    conn.close(0u32.into(), b"done");
    let _ = server_handle.await;
}

#[tokio::test]
async fn test_quic_ipv6_loopback_connection() {
    let network = Network::new();

    // 1. Listen on IPv6 localhost ([::1]:0)
    let server_endpoint = match network.quic.listen("[::1]:0").await {
        Ok(ep) => ep,
        Err(e) => {
            // Some CI/docker environments do not enable IPv6 loopback
            eprintln!("Skipping IPv6 test as [::1] bind failed: {e}");
            return;
        }
    };
    let server_addr = server_endpoint.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let incoming = server_endpoint.accept().await.unwrap();
        let connection = incoming.await.unwrap();
        let (mut send, mut recv) = connection.accept_bi().await.unwrap();

        let mut buf = [0u8; 4];
        recv.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ipv6");

        send.write_all(b"resp").await.unwrap();
        send.finish().unwrap();

        let _ = connection.closed().await;
    });

    // 2. Connect to IPv6 address
    let conn = network
        .quic
        .connect(server_addr, "localhost")
        .await
        .unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();

    send.write_all(b"ipv6").await.unwrap();
    send.finish().unwrap();

    let mut buf = [0u8; 4];
    recv.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"resp");

    conn.close(0u32.into(), b"done");
    let _ = server_handle.await;
}
