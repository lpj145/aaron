use aaron_core::{Network, Uuid};

#[tokio::test]
async fn test_tls_p2p_identity_verification_and_spoofing_prevention() {
    let network_server = Network::new();
    let network_client = Network::new();

    let legitimate_node_uuid = Uuid::random();
    let impostor_node_uuid = Uuid::random();

    // 1. Server listens presenting certificate bound to legitimate_node_uuid
    let endpoint = network_server
        .quic
        .listen_for_node("127.0.0.1:0", legitimate_node_uuid)
        .await
        .unwrap();
    let server_addr = endpoint.local_addr().unwrap();

    // Accept incoming QUIC streams in background
    let ep = endpoint.clone();
    let server_task = tokio::spawn(async move {
        while let Some(incoming) = ep.accept().await {
            if let Ok(connection) = incoming.await {
                tokio::spawn(async move {
                    while let Ok((mut send, mut recv)) = connection.accept_bi().await {
                        let mut buf = [0u8; 5];
                        if recv.read_exact(&mut buf).await.is_ok() {
                            let _ = send.write_all(&buf).await;
                            let _ = send.finish();
                        }
                    }
                });
            }
        }
    });

    // 2. Client connects specifying the expected legitimate_node_uuid -> SUCCESS
    let legitimate_conn = network_client
        .quic
        .connect_node(server_addr, legitimate_node_uuid)
        .await;

    assert!(
        legitimate_conn.is_ok(),
        "Client must successfully establish TLS connection when server cert matches expected UUID"
    );

    let conn = legitimate_conn.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    send.write_all(b"HELLO").await.unwrap();

    let mut resp = [0u8; 5];
    recv.read_exact(&mut resp).await.unwrap();
    assert_eq!(&resp, b"HELLO");

    // 3. Client connects to the same server, but expecting impostor_node_uuid -> REJECTED AT TLS HANDSHAKE
    let rogue_client_net = Network::new();
    let spoofed_conn_res = rogue_client_net
        .quic
        .connect_node(server_addr, impostor_node_uuid)
        .await;

    assert!(
        spoofed_conn_res.is_err(),
        "Client must reject TLS handshake when server certificate does NOT match the expected target NodeId UUID"
    );

    endpoint.close(0u32.into(), b"closed");
    server_task.abort();
}

#[tokio::test]
async fn test_tls_p2p_generic_hostname_connection() {
    let network_server = Network::new();
    let network_client = Network::new();
    let node_uuid = Uuid::random();

    let endpoint = network_server
        .quic
        .listen_for_node("127.0.0.1:0", node_uuid)
        .await
        .unwrap();
    let server_addr = endpoint.local_addr().unwrap();

    let ep = endpoint.clone();
    let server_task = tokio::spawn(async move {
        while let Some(incoming) = ep.accept().await {
            let _ = incoming.await;
        }
    });

    // Server certificate includes SANs: node_hex, node-{node_hex}, localhost, 127.0.0.1
    let conn_res = network_client.quic.connect(server_addr, "localhost").await;

    assert!(
        conn_res.is_ok(),
        "Generic connection matching standard SAN (localhost) must succeed"
    );

    endpoint.close(0u32.into(), b"closed");
    server_task.abort();
}
