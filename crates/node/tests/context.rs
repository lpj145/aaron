use node::{Context, Env, EventHub, Network, NodeId, Store, Uuid};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, PartialEq, Eq)]
struct IngestCompleted {
    record_id: u64,
    keyspace: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkerTask {
    task_id: usize,
    payload: String,
}

#[tokio::test]
async fn test_context_construction_and_subsystem_access() {
    let temp_dir = std::env::temp_dir().join(format!("test_context_basic_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();
    let node_uuid = Uuid::new(0x1111, 0x2222);
    let identity = NodeId::with_current_incarnation(node_uuid, None);
    let env = Arc::new(Env::detect());
    let network = Network::new();
    let event_hub = EventHub::new();
    let token = CancellationToken::new();

    let ctx = Context::new(
        event_hub.clone(),
        network.clone(),
        store.clone(),
        identity,
        env.clone(),
        token.clone(),
    );

    // 1. Identity access
    assert_eq!(ctx.identity.id(), node_uuid);

    // 2. Env access
    assert_eq!(ctx.env.hostname, env.hostname);

    // 3. Store access
    let ks = ctx.store.keyspace("test_ctx").unwrap();
    ks.insert("hello", b"world").unwrap();
    assert_eq!(ks.get("hello").unwrap().unwrap(), b"world");

    // 4. EventHub access
    let mut sub = ctx.event_hub.subscribe::<WorkerTask>().await;
    ctx.event_hub
        .publish(WorkerTask {
            task_id: 1,
            payload: "init".to_string(),
        })
        .await;
    let event = sub.recv().await.unwrap();
    assert_eq!(event.task_id, 1);
    assert_eq!(event.payload, "init");

    // 5. Network access
    let listener = ctx.network.tcp.listen("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let conn = ctx.network.tcp.connect(addr).await.unwrap();
    assert_eq!(conn.peer_addr(), addr);

    // 6. Token access
    assert!(!ctx.token.is_cancelled());
    token.cancel();
    assert!(ctx.token.is_cancelled());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_context_multi_service_pipeline_coordination() {
    let temp_dir =
        std::env::temp_dir().join(format!("test_context_pipeline_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();
    let node_uuid = Uuid::random();
    let identity = NodeId::with_current_incarnation(node_uuid, None);
    let env = Arc::new(Env::detect());
    let network = Network::new();
    let event_hub = EventHub::new();
    let token = CancellationToken::new();

    let ctx = Context::new(event_hub, network, store, identity, env, token);

    // Service 3: UDP Receiver awaiting egress data
    let egress_socket = ctx.network.udp.bind("127.0.0.1:0").await.unwrap();
    let egress_addr = egress_socket.local_addr().unwrap();

    let egress_handle = tokio::spawn(async move {
        let mut buf = [0u8; 64];
        let (n, _) = egress_socket.recv_from(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf[..n]).to_string()
    });

    // Service 2: Processor Service listening to EventHub, reading Store, and transmitting via UDP
    let ctx_proc = ctx.clone();
    let proc_handle = tokio::spawn(async move {
        let mut sub = ctx_proc.event_hub.subscribe::<IngestCompleted>().await;
        let event = sub.recv().await.unwrap();

        // Read raw data from Store
        let ks = ctx_proc.store.keyspace(&event.keyspace).unwrap();
        let raw = ks.get(event.record_id.to_be_bytes()).unwrap().unwrap();
        let text = String::from_utf8(raw.to_vec()).unwrap();

        let processed = format!("PROCESSED: {text}");

        // Save processed result into another keyspace
        let out_ks = ctx_proc.store.keyspace("processed_data").unwrap();
        out_ks
            .insert(event.record_id.to_be_bytes(), processed.as_bytes())
            .unwrap();

        // Transmit UDP datagram to egress
        let udp = ctx_proc.network.udp.bind("127.0.0.1:0").await.unwrap();
        udp.send_to(processed.as_bytes(), egress_addr)
            .await
            .unwrap();
    });

    // Service 1: Ingest Service writing data to store and notifying pipeline
    let ctx_ingest = ctx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let ks = ctx_ingest.store.keyspace("raw_data").unwrap();
        ks.insert(42u64.to_be_bytes(), b"sensor_temperature_24C")
            .unwrap();

        ctx_ingest
            .event_hub
            .publish(IngestCompleted {
                record_id: 42,
                keyspace: "raw_data".to_string(),
            })
            .await;
    });

    proc_handle.await.unwrap();
    let received_output = egress_handle.await.unwrap();
    assert_eq!(received_output, "PROCESSED: sensor_temperature_24C");

    // Verify processed output in store
    let out_ks = ctx.store.keyspace("processed_data").unwrap();
    let saved = out_ks.get(42u64.to_be_bytes()).unwrap().unwrap();
    assert_eq!(saved, b"PROCESSED: sensor_temperature_24C");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_context_concurrent_tasks_sharing_all_subsystems() {
    let temp_dir =
        std::env::temp_dir().join(format!("test_context_concurrent_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();
    let node_uuid = Uuid::random();
    let identity = NodeId::with_current_incarnation(node_uuid, None);
    let env = Arc::new(Env::detect());
    let network = Network::new();
    let event_hub = EventHub::new();
    let token = CancellationToken::new();

    let ctx = Context::new(event_hub, network, store, identity, env, token);

    // Setup TCP server for concurrent connection verification
    let tcp_listener = ctx.network.tcp.listen("127.0.0.1:0").await.unwrap();
    let tcp_addr = tcp_listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        while let Ok((mut socket, _)) = tcp_listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4];
                while socket.read_exact(&mut buf).await.is_ok() {
                    if socket.write_all(&buf).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    let mut tasks = Vec::new();
    for task_id in 0..10 {
        let task_ctx = ctx.clone();
        tasks.push(tokio::spawn(async move {
            // 1. Check Identity
            assert_eq!(task_ctx.identity.id(), node_uuid);

            // 2. Read Env
            assert!(!task_ctx.env.hostname.is_empty());

            // 3. Write to Store
            let ks = task_ctx.store.keyspace("concurrent_ks").unwrap();
            let key = format!("task_{task_id}");
            let val = format!("val_{task_id}");
            ks.insert(&key, val.as_bytes()).unwrap();

            // 4. TCP Echo
            let conn = task_ctx.network.tcp.connect(tcp_addr).await.unwrap();
            conn.write_all(b"echo").await.unwrap();
            let mut echo_buf = [0u8; 4];
            conn.read_exact(&mut echo_buf).await.unwrap();
            assert_eq!(&echo_buf, b"echo");

            // 5. Read back Store
            let read_val = ks.get(&key).unwrap().unwrap();
            assert_eq!(read_val, val.as_bytes());
        }));
    }

    for t in tasks {
        t.await.unwrap();
    }

    server_handle.abort();
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_context_store_persistence_and_node_identity_continuity() {
    let temp_dir =
        std::env::temp_dir().join(format!("test_context_continuity_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let node_uuid = Uuid::random();

    // --- BOOT 1 ---
    {
        let store = Store::open(&temp_dir).unwrap();
        let identity = NodeId::new(node_uuid, 1000, None);
        let env = Arc::new(Env::detect());
        let token = CancellationToken::new();
        let ctx = Context::new(EventHub::new(), Network::new(), store, identity, env, token);

        let ks = ctx.store.keyspace("cluster_state").unwrap();
        ks.insert("epoch", 1u64.to_be_bytes()).unwrap();
        ctx.store.persist().unwrap();
    }

    // --- BOOT 2 (re-opening same directory) ---
    {
        let store = Store::open(&temp_dir).unwrap();
        let identity = NodeId::new(node_uuid, 2000, None); // Incremented incarnation
        let env = Arc::new(Env::detect());
        let token = CancellationToken::new();
        let ctx = Context::new(EventHub::new(), Network::new(), store, identity, env, token);

        // Verify same node UUID, new incarnation
        assert_eq!(ctx.identity.id(), node_uuid);
        assert_eq!(ctx.identity.incarnation, 2000);

        // Verify stored state survived reboot
        let ks = ctx.store.keyspace("cluster_state").unwrap();
        let epoch = ks.get("epoch").unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(epoch.as_slice().try_into().unwrap()), 1);
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_context_cancellation_token_child_propagation() {
    let parent_token = CancellationToken::new();
    let temp_dir = std::env::temp_dir().join(format!("test_context_token_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = Store::open(&temp_dir).unwrap();
    let node_uuid = Uuid::random();
    let identity = NodeId::with_current_incarnation(node_uuid, None);
    let env = Arc::new(Env::detect());
    let ctx = Context::new(
        EventHub::new(),
        Network::new(),
        store,
        identity,
        env,
        parent_token.clone(),
    );

    let child_ctx = ctx.with_child_token();
    assert!(!ctx.token.is_cancelled());
    assert!(!child_ctx.token.is_cancelled());

    // Cancelling child does not cancel parent
    child_ctx.token.cancel();
    assert!(child_ctx.token.is_cancelled());
    assert!(!ctx.token.is_cancelled());

    // Creating another child and cancelling parent cancels the child
    let child_ctx2 = ctx.with_child_token();
    parent_token.cancel();
    assert!(ctx.token.is_cancelled());
    assert!(child_ctx2.token.is_cancelled());

    let _ = std::fs::remove_dir_all(&temp_dir);
}
