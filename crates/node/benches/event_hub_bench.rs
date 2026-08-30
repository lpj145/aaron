use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use node::EventHub;
use std::hint::black_box;
use std::sync::Arc;
use tokio::runtime::Runtime;

#[derive(Clone, Debug, PartialEq)]
struct BenchmarkEvent {
    id: u64,
    payload: [u8; 64],
}

#[derive(Clone, Debug, PartialEq)]
struct TopicAEvent(u64);

#[derive(Clone, Debug, PartialEq)]
struct TopicBEvent(u64);

#[derive(Clone, Debug, PartialEq)]
struct TopicCEvent(u64);

#[derive(Clone, Debug, PartialEq)]
struct TopicDEvent(u64);

fn bench_publish_fanout(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("event_hub_fanout");

    for num_subscribers in [1, 10, 50, 100] {
        group.throughput(Throughput::Elements(num_subscribers as u64));
        group.bench_with_input(
            BenchmarkId::new("subscribers", num_subscribers),
            &num_subscribers,
            |b, &subs| {
                b.to_async(&rt).iter_custom(|iters| async move {
                    let hub = EventHub::with_capacity(16384);
                    let mut receivers = Vec::with_capacity(subs);

                    for _ in 0..subs {
                        receivers.push(hub.subscribe::<BenchmarkEvent>().await);
                    }

                    // Background drainer tasks to keep queues clear
                    let drain_handles: Vec<_> = receivers
                        .into_iter()
                        .map(|mut rx| {
                            tokio::spawn(async move {
                                while rx.recv().await.is_ok() {
                                    // consumed
                                }
                            })
                        })
                        .collect();

                    let event = BenchmarkEvent {
                        id: 42,
                        payload: [7u8; 64],
                    };

                    let start = std::time::Instant::now();
                    for _ in 0..iters {
                        let delivered = hub.publish(black_box(event.clone())).await;
                        black_box(delivered);
                    }
                    let elapsed = start.elapsed();

                    hub.clear_all().await;
                    for handle in drain_handles {
                        handle.abort();
                    }

                    elapsed
                });
            },
        );
    }
    group.finish();
}

fn bench_concurrent_publishers(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("event_hub_concurrent_producers");

    for num_producers in [1, 2, 4, 8] {
        group.throughput(Throughput::Elements(num_producers as u64 * 100));
        group.bench_with_input(
            BenchmarkId::new("producers", num_producers),
            &num_producers,
            |b, &prods| {
                b.to_async(&rt).iter_custom(|iters| async move {
                    let hub = Arc::new(EventHub::with_capacity(32768));
                    let mut rx = hub.subscribe::<BenchmarkEvent>().await;

                    let drain_handle =
                        tokio::spawn(async move { while let Ok(_event) = rx.recv().await {} });

                    let start = std::time::Instant::now();
                    for _ in 0..iters {
                        let mut tasks = Vec::with_capacity(prods);
                        for p in 0..prods {
                            let hub_clone = hub.clone();
                            tasks.push(tokio::spawn(async move {
                                for i in 0..100 {
                                    let event = BenchmarkEvent {
                                        id: (p as u64) * 1000 + i,
                                        payload: [1u8; 64],
                                    };
                                    hub_clone.publish(black_box(event)).await;
                                }
                            }));
                        }
                        for task in tasks {
                            task.await.unwrap();
                        }
                    }
                    let elapsed = start.elapsed();

                    hub.clear_all().await;
                    drain_handle.abort();

                    elapsed
                });
            },
        );
    }
    group.finish();
}

fn bench_multi_topic_parallelism(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("event_hub_multi_topic_isolation");

    group.throughput(Throughput::Elements(400));
    group.bench_function("4_parallel_topics", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let hub = Arc::new(EventHub::with_capacity(16384));

            let mut rx_a = hub.subscribe::<TopicAEvent>().await;
            let mut rx_b = hub.subscribe::<TopicBEvent>().await;
            let mut rx_c = hub.subscribe::<TopicCEvent>().await;
            let mut rx_d = hub.subscribe::<TopicDEvent>().await;

            let d_a = tokio::spawn(async move { while rx_a.recv().await.is_ok() {} });
            let d_b = tokio::spawn(async move { while rx_b.recv().await.is_ok() {} });
            let d_c = tokio::spawn(async move { while rx_c.recv().await.is_ok() {} });
            let d_d = tokio::spawn(async move { while rx_d.recv().await.is_ok() {} });

            let start = std::time::Instant::now();
            for _ in 0..iters {
                let h1 = hub.clone();
                let t1 = tokio::spawn(async move {
                    for i in 0..100 {
                        h1.publish(TopicAEvent(i)).await;
                    }
                });

                let h2 = hub.clone();
                let t2 = tokio::spawn(async move {
                    for i in 0..100 {
                        h2.publish(TopicBEvent(i)).await;
                    }
                });

                let h3 = hub.clone();
                let t3 = tokio::spawn(async move {
                    for i in 0..100 {
                        h3.publish(TopicCEvent(i)).await;
                    }
                });

                let h4 = hub.clone();
                let t4 = tokio::spawn(async move {
                    for i in 0..100 {
                        h4.publish(TopicDEvent(i)).await;
                    }
                });

                let (r1, r2, r3, r4) = tokio::join!(t1, t2, t3, t4);
                r1.unwrap();
                r2.unwrap();
                r3.unwrap();
                r4.unwrap();
            }
            let elapsed = start.elapsed();

            hub.clear_all().await;
            d_a.abort();
            d_b.abort();
            d_c.abort();
            d_d.abort();

            elapsed
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_publish_fanout,
    bench_concurrent_publishers,
    bench_multi_topic_parallelism
);
criterion_main!(benches);
