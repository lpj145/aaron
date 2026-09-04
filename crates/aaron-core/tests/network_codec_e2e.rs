use aaron_core::{
    DEFAULT_MAX_FRAME_SIZE, FrameError, Network, Uuid, read_frame, read_frame_with_limit,
    write_frame, write_frame_with_limit,
};
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn test_codec_zero_byte_empty_frame_roundtrip() {
    let network = Network::new();
    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        // Read empty frame
        let frame = read_frame(&mut socket).await.unwrap().unwrap();
        assert_eq!(frame, Vec::<u8>::new());

        // Echo back empty frame
        write_frame(&mut socket, &[]).await.unwrap();
    });

    let conn = network.tcp.connect(server_addr).await.unwrap();
    let (reader, writer) = conn.split();

    let mut r_guard = reader.inner().lock().await;
    let mut w_guard = writer.inner().lock().await;

    write_frame(&mut *w_guard, &[]).await.unwrap();
    let echoed = read_frame(&mut *r_guard).await.unwrap().unwrap();
    assert_eq!(echoed, Vec::<u8>::new());

    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_codec_high_throughput_burst_over_tcp() {
    let network = Network::new();
    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let frame_count = 200;

    let server_handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        for i in 0..frame_count {
            let frame = read_frame(&mut socket).await.unwrap().unwrap();
            assert_eq!(frame, format!("msg_{i}").into_bytes());

            // Echo back with prefix
            let mut response = b"echo_".to_vec();
            response.extend_from_slice(&frame);
            write_frame(&mut socket, &response).await.unwrap();
        }
    });

    let conn = network.tcp.connect(server_addr).await.unwrap();
    let (reader, writer) = conn.split();

    let write_handle = tokio::spawn(async move {
        let mut w = writer.inner().lock().await;
        for i in 0..frame_count {
            let payload = format!("msg_{i}").into_bytes();
            write_frame(&mut *w, &payload).await.unwrap();
        }
    });

    let read_handle = tokio::spawn(async move {
        let mut r = reader.inner().lock().await;
        for i in 0..frame_count {
            let resp = read_frame(&mut *r).await.unwrap().unwrap();
            let expected = format!("echo_msg_{i}").into_bytes();
            assert_eq!(resp, expected);
        }
    });

    write_handle.await.unwrap();
    read_handle.await.unwrap();
    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_codec_over_quic_multiplexed_streams() {
    let network = Network::new();
    let node_uuid = Uuid::random();

    let server_endpoint = network
        .quic
        .listen_for_node("127.0.0.1:0", node_uuid)
        .await
        .unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let incoming = server_endpoint.accept().await.unwrap();
        let conn = incoming.await.unwrap();

        for _ in 0..10 {
            let (mut send, mut recv) = conn.accept_bi().await.unwrap();
            tokio::spawn(async move {
                while let Ok(Some(frame)) = read_frame(&mut recv).await {
                    let mut echo = b"quic_echo_".to_vec();
                    echo.extend_from_slice(&frame);
                    write_frame(&mut send, &echo).await.unwrap();
                }
                let _ = send.finish();
            });
        }
    });

    let conn = network
        .quic
        .connect_node(server_addr, node_uuid)
        .await
        .unwrap();

    // Spawn 10 concurrent bi-directional streams each streaming multiple frames
    let mut stream_tasks = Vec::new();
    for stream_idx in 0..10 {
        let conn_clone = conn.clone();
        stream_tasks.push(tokio::spawn(async move {
            let (mut send, mut recv) = conn_clone.open_bi().await.unwrap();

            for frame_idx in 0..5 {
                let payload = format!("stream_{stream_idx}_frame_{frame_idx}").into_bytes();
                write_frame(&mut send, &payload).await.unwrap();

                let resp = read_frame(&mut recv).await.unwrap().unwrap();
                let expected =
                    format!("quic_echo_stream_{stream_idx}_frame_{frame_idx}").into_bytes();
                assert_eq!(resp, expected);
            }

            let _ = send.finish();
        }));
    }

    for t in stream_tasks {
        t.await.unwrap();
    }

    conn.close(0u32.into(), b"done");
    server_handle.abort();
}

#[tokio::test]
async fn test_codec_frame_size_limit_rejection_over_tcp() {
    let network = Network::new();
    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        // Server has default 3MB max limit
        let err = read_frame(&mut socket).await.unwrap_err();
        assert!(
            matches!(err, FrameError::FrameTooLarge { size, max } if size == DEFAULT_MAX_FRAME_SIZE + 10 && max == DEFAULT_MAX_FRAME_SIZE)
        );
    });

    let mut stream = tokio::net::TcpStream::connect(server_addr).await.unwrap();

    // Send a 4-byte header claiming a payload larger than 3MB
    let huge_len = (DEFAULT_MAX_FRAME_SIZE as u32) + 10;
    stream.write_all(&huge_len.to_be_bytes()).await.unwrap();

    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_codec_custom_limit_over_quic() {
    let network = Network::new();
    let server_endpoint = network.quic.listen("127.0.0.1:0").await.unwrap();
    let server_addr = server_endpoint.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let incoming = server_endpoint.accept().await.unwrap();
        let conn = incoming.await.unwrap();
        let (_send, mut recv) = conn.accept_bi().await.unwrap();

        // Limit server reader to 1024 bytes
        let err = read_frame_with_limit(&mut recv, 1024).await.unwrap_err();
        assert!(
            matches!(err, FrameError::FrameTooLarge { size, max } if size == 2048 && max == 1024)
        );
    });

    let conn = network
        .quic
        .connect(server_addr, "localhost")
        .await
        .unwrap();
    let (mut send, _recv) = conn.open_bi().await.unwrap();

    let payload = vec![0xAA; 2048];
    write_frame_with_limit(&mut send, &payload, 4096)
        .await
        .unwrap();

    server_handle.await.unwrap();
    conn.close(0u32.into(), b"done");
}

#[tokio::test]
async fn test_codec_unexpected_disconnect_during_frame_read() {
    let network = Network::new();
    let listener = network.tcp.listen("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        // Header says 100 bytes follow
        let header = (100u32).to_be_bytes();
        socket.write_all(&header).await.unwrap();
        // Send only 10 bytes then abruptly close
        socket.write_all(&[1u8; 10]).await.unwrap();
        drop(socket);
    });

    let conn = network.tcp.connect(server_addr).await.unwrap();
    let (reader, _writer) = conn.split();
    let mut r = reader.inner().lock().await;

    let err = read_frame(&mut *r).await.unwrap_err();
    assert!(matches!(err, FrameError::UnexpectedEof));

    server_handle.await.unwrap();
}
