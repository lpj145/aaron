use aaron_core::{
    DEFAULT_MAX_FRAME_SIZE, FrameError, read_frame, read_frame_with_limit, write_frame,
    write_frame_with_limit,
};
use tokio::io::duplex;

#[tokio::test]
async fn test_frame_write_and_read_roundtrip() {
    let (mut client, mut server) = duplex(1024);

    let payload = b"hello_aaron_p2p_frame";

    let write_handle = tokio::spawn(async move {
        write_frame(&mut client, payload).await.unwrap();
    });

    let read_handle = tokio::spawn(async move { read_frame(&mut server).await.unwrap().unwrap() });

    write_handle.await.unwrap();
    let received = read_handle.await.unwrap();
    assert_eq!(received, payload);
}

#[tokio::test]
async fn test_frame_multi_frame_pipelining() {
    let (mut client, mut server) = duplex(1024);

    let frames = vec![
        b"frame_1".to_vec(),
        b"frame_2_longer_payload".to_vec(),
        b"frame_3_final".to_vec(),
    ];
    let frames_clone = frames.clone();

    tokio::spawn(async move {
        for f in &frames_clone {
            write_frame(&mut client, f).await.unwrap();
        }
    });

    for expected in frames {
        let actual = read_frame(&mut server).await.unwrap().unwrap();
        assert_eq!(actual, expected);
    }

    // After all frames, clean EOF when writer closes/drops
    let eof = read_frame(&mut server).await.unwrap();
    assert_eq!(eof, None);
}

#[tokio::test]
async fn test_frame_size_limit_3mb() {
    let (mut client, _server) = duplex(1024);

    // Verify default limit is exactly 3MB
    assert_eq!(DEFAULT_MAX_FRAME_SIZE, 3 * 1024 * 1024);

    // Attempting to write > 3MB should be rejected
    let huge_payload = vec![0u8; DEFAULT_MAX_FRAME_SIZE + 1];
    let err = write_frame(&mut client, &huge_payload).await.unwrap_err();
    assert!(
        matches!(err, FrameError::FrameTooLarge { size, max } if size == DEFAULT_MAX_FRAME_SIZE + 1 && max == DEFAULT_MAX_FRAME_SIZE)
    );

    // Custom limit
    let err_custom = write_frame_with_limit(&mut client, b"12345", 4)
        .await
        .unwrap_err();
    assert!(matches!(
        err_custom,
        FrameError::FrameTooLarge { size: 5, max: 4 }
    ));

    // Reading frame exceeding limit
    let (mut client2, mut server2) = duplex(1024);
    tokio::spawn(async move {
        write_frame_with_limit(&mut client2, b"123456", 100)
            .await
            .unwrap();
    });

    let read_err = read_frame_with_limit(&mut server2, 4).await.unwrap_err();
    assert!(matches!(
        read_err,
        FrameError::FrameTooLarge { size: 6, max: 4 }
    ));
}

#[tokio::test]
async fn test_frame_unexpected_eof_detection() {
    // Incomplete 2-byte header (expected 4 bytes)
    let (mut client, mut server) = duplex(1024);
    tokio::io::AsyncWriteExt::write_all(&mut client, &[0, 0])
        .await
        .unwrap();
    drop(client);

    let err = read_frame(&mut server).await.unwrap_err();
    assert!(matches!(err, FrameError::UnexpectedEof));

    // Valid 4-byte header saying 10 bytes follow, but only 3 bytes sent
    let (mut client2, mut server2) = duplex(1024);
    let len_header = (10u32).to_be_bytes();
    tokio::io::AsyncWriteExt::write_all(&mut client2, &len_header)
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::write_all(&mut client2, &[1, 2, 3])
        .await
        .unwrap();
    drop(client2);

    let err2 = read_frame(&mut server2).await.unwrap_err();
    assert!(matches!(err2, FrameError::UnexpectedEof));
}
