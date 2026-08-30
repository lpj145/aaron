use node::EventHub;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Debug, PartialEq, Eq)]
struct PeerJoined {
    id: String,
    port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StateUpdated {
    key: String,
    version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LogMessage {
    level: u8,
    msg: String,
}

#[tokio::test]
async fn test_event_hub_basic_pub_sub() {
    let hub = EventHub::new();

    let mut sub = hub.subscribe::<PeerJoined>().await;
    assert_eq!(hub.subscriber_count::<PeerJoined>().await, 1);

    let delivered = hub
        .publish(PeerJoined {
            id: "node-1".to_string(),
            port: 9000,
        })
        .await;
    assert_eq!(delivered, 1);

    let event = sub.recv().await.unwrap();
    assert_eq!(event.id, "node-1");
    assert_eq!(event.port, 9000);
}

#[tokio::test]
async fn test_event_hub_multiple_subscribers_fan_out() {
    let hub = EventHub::new();

    let mut sub1 = hub.subscribe::<PeerJoined>().await;
    let mut sub2 = hub.subscribe::<PeerJoined>().await;
    let mut sub3 = hub.subscribe::<PeerJoined>().await;
    assert_eq!(hub.subscriber_count::<PeerJoined>().await, 3);

    let event_to_send = PeerJoined {
        id: "node-fanout".to_string(),
        port: 8080,
    };

    let delivered = hub.publish(event_to_send.clone()).await;
    assert_eq!(delivered, 3);

    assert_eq!(sub1.recv().await.unwrap(), event_to_send);
    assert_eq!(sub2.recv().await.unwrap(), event_to_send);
    assert_eq!(sub3.recv().await.unwrap(), event_to_send);
}

#[tokio::test]
async fn test_event_hub_type_isolation() {
    let hub = EventHub::new();

    let mut sub_peer = hub.subscribe::<PeerJoined>().await;
    let mut sub_state = hub.subscribe::<StateUpdated>().await;

    // Publish PeerJoined
    hub.publish(PeerJoined {
        id: "peer-abc".to_string(),
        port: 7000,
    })
    .await;

    // Publish StateUpdated
    hub.publish(StateUpdated {
        key: "config/leader".to_string(),
        version: 42,
    })
    .await;

    let peer_event = sub_peer.recv().await.unwrap();
    assert_eq!(peer_event.id, "peer-abc");

    let state_event = sub_state.recv().await.unwrap();
    assert_eq!(state_event.key, "config/leader");
    assert_eq!(state_event.version, 42);

    // sub_peer should have nothing more
    assert_eq!(sub_peer.try_recv().unwrap(), None);
    assert_eq!(sub_state.try_recv().unwrap(), None);
}

#[tokio::test]
async fn test_event_hub_dead_subscriber_pruning() {
    let hub = EventHub::new();

    let mut sub1 = hub.subscribe::<PeerJoined>().await;
    let sub2 = hub.subscribe::<PeerJoined>().await;
    assert_eq!(hub.subscriber_count::<PeerJoined>().await, 2);

    // Drop sub2
    drop(sub2);

    // Publish event — should detect sub2 is dead, prune it, and deliver only to sub1
    let delivered = hub
        .publish(PeerJoined {
            id: "alive-only".to_string(),
            port: 1111,
        })
        .await;

    assert_eq!(delivered, 1);
    assert_eq!(hub.subscriber_count::<PeerJoined>().await, 1);

    let event = sub1.recv().await.unwrap();
    assert_eq!(event.id, "alive-only");
}

#[tokio::test]
async fn test_event_hub_try_recv() {
    let hub = EventHub::new();

    let mut sub = hub.subscribe::<StateUpdated>().await;

    // Initially empty
    assert_eq!(sub.try_recv().unwrap(), None);

    hub.publish(StateUpdated {
        key: "k1".to_string(),
        version: 1,
    })
    .await;

    // Now has item
    let item = sub.try_recv().unwrap().unwrap();
    assert_eq!(item.key, "k1");

    // Empty again
    assert_eq!(sub.try_recv().unwrap(), None);
}

#[tokio::test]
async fn test_event_hub_clear_and_clear_all() {
    let hub = EventHub::new();

    let _sub1 = hub.subscribe::<PeerJoined>().await;
    let _sub2 = hub.subscribe::<StateUpdated>().await;
    assert_eq!(hub.subscriber_count::<PeerJoined>().await, 1);
    assert_eq!(hub.subscriber_count::<StateUpdated>().await, 1);

    // Clear specific type
    hub.clear::<PeerJoined>().await;
    assert_eq!(hub.subscriber_count::<PeerJoined>().await, 0);
    assert_eq!(hub.subscriber_count::<StateUpdated>().await, 1);

    // Clear all
    hub.clear_all().await;
    assert_eq!(hub.subscriber_count::<StateUpdated>().await, 0);
}

#[tokio::test]
async fn test_event_hub_high_throughput_stress() {
    let hub = EventHub::new();
    let subscriber_count = 5;
    let messages_per_sender = 200;
    let sender_count = 3;
    let total_expected_per_sub = messages_per_sender * sender_count;

    let received_counters = Arc::new(AtomicUsize::new(0));

    let mut sub_handles = Vec::new();
    for _ in 0..subscriber_count {
        let mut sub = hub.subscribe_with_capacity::<LogMessage>(512).await;
        let counter = received_counters.clone();
        sub_handles.push(tokio::spawn(async move {
            let mut count = 0;
            while count < total_expected_per_sub {
                if let Ok(msg) = sub.recv().await {
                    assert_eq!(msg.level, 1);
                    count += 1;
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }
            count
        }));
    }

    let mut pub_handles = Vec::new();
    for s in 0..sender_count {
        let hub_clone = hub.clone();
        pub_handles.push(tokio::spawn(async move {
            for m in 0..messages_per_sender {
                hub_clone
                    .publish(LogMessage {
                        level: 1,
                        msg: format!("sender-{s}-msg-{m}"),
                    })
                    .await;
            }
        }));
    }

    for p in pub_handles {
        p.await.unwrap();
    }

    for s in sub_handles {
        let count = s.await.unwrap();
        assert_eq!(count, total_expected_per_sub);
    }

    assert_eq!(
        received_counters.load(Ordering::SeqCst),
        total_expected_per_sub * subscriber_count
    );
}

#[tokio::test]
async fn test_event_hub_interleaved_multi_types_concurrency() {
    let hub = EventHub::new();

    let mut sub_peer = hub.subscribe::<PeerJoined>().await;
    let mut sub_state = hub.subscribe::<StateUpdated>().await;
    let mut sub_log = hub.subscribe::<LogMessage>().await;

    let hub1 = hub.clone();
    let hub2 = hub.clone();
    let hub3 = hub.clone();

    let h1 = tokio::spawn(async move {
        for i in 0..50 {
            hub1.publish(PeerJoined {
                id: format!("peer-{i}"),
                port: 8000 + (i as u16),
            })
            .await;
        }
    });

    let h2 = tokio::spawn(async move {
        for i in 0..50 {
            hub2.publish(StateUpdated {
                key: format!("k-{i}"),
                version: i,
            })
            .await;
        }
    });

    let h3 = tokio::spawn(async move {
        for i in 0..50 {
            hub3.publish(LogMessage {
                level: 2,
                msg: format!("log-{i}"),
            })
            .await;
        }
    });

    for _ in 0..50 {
        let p = sub_peer.recv().await.unwrap();
        assert!(p.id.starts_with("peer-"));

        let s = sub_state.recv().await.unwrap();
        assert!(s.key.starts_with("k-"));

        let l = sub_log.recv().await.unwrap();
        assert_eq!(l.level, 2);
    }

    h1.await.unwrap();
    h2.await.unwrap();
    h3.await.unwrap();
}
