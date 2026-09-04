use aaron_core::EventHub;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TestEvent {
    id: usize,
    payload: String,
}

#[tokio::test]
async fn test_slow_subscriber_does_not_block_publisher_or_healthy_subscribers() {
    let hub = EventHub::with_capacity(4); // Small capacity for quick saturation

    // 1. Create a Stalled Subscriber that never reads
    let _stalled_sub = hub.subscribe::<TestEvent>().await;

    // 2. Create 2 Healthy Fast Subscribers
    let mut sub_1 = hub.subscribe::<TestEvent>().await;
    let mut sub_2 = hub.subscribe::<TestEvent>().await;

    let sub_1_count = Arc::new(AtomicUsize::new(0));
    let sub_2_count = Arc::new(AtomicUsize::new(0));

    let c1 = sub_1_count.clone();
    let task_1 = tokio::spawn(async move {
        while let Ok(_evt) = sub_1.recv().await {
            c1.fetch_add(1, Ordering::Relaxed);
        }
    });

    let c2 = sub_2_count.clone();
    let task_2 = tokio::spawn(async move {
        while let Ok(_evt) = sub_2.recv().await {
            c2.fetch_add(1, Ordering::Relaxed);
        }
    });

    // 3. Publish 30 events rapidly — must complete within 200ms without blocking!
    let publish_fut = async {
        for i in 0..30 {
            hub.publish(TestEvent {
                id: i,
                payload: format!("msg_{i}"),
            })
            .await;
        }
    };

    let res = tokio::time::timeout(Duration::from_millis(500), publish_fut).await;
    assert!(
        res.is_ok(),
        "Publisher must never block on a stalled subscriber"
    );

    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        sub_1_count.load(Ordering::Relaxed),
        30,
        "Healthy subscriber 1 must receive all 30 events"
    );
    assert_eq!(
        sub_2_count.load(Ordering::Relaxed),
        30,
        "Healthy subscriber 2 must receive all 30 events"
    );

    task_1.abort();
    task_2.abort();
}

#[tokio::test]
async fn test_event_hub_live_subscriber_count_and_automatic_gc() {
    let hub = EventHub::new();

    let sub1 = hub.subscribe::<TestEvent>().await;
    let sub2 = hub.subscribe::<TestEvent>().await;
    let sub3 = hub.subscribe::<TestEvent>().await;

    assert_eq!(hub.subscriber_count::<TestEvent>().await, 3);

    // Drop sub1 and sub2 without publishing any events
    drop(sub1);
    drop(sub2);

    // subscriber_count must immediately prune the disconnected subscribers
    assert_eq!(
        hub.subscriber_count::<TestEvent>().await,
        1,
        "Dead subscribers must be pruned when querying subscriber_count"
    );

    drop(sub3);
    assert_eq!(hub.subscriber_count::<TestEvent>().await, 0);
}
