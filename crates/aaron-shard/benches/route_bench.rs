use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use aaron_shard::route::{
    decode_shard_key_u16, determine_shard, encode_shard_key_u16, fnv1a_64, Router, ShardKey,
};
use std::hint::black_box;

#[inline]
fn fnv1a_baseline(data: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[inline]
fn fnv1a_with_if_8(data: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    if data.len() == 8 {
        let mut hash = FNV_OFFSET_BASIS;
        hash = (hash ^ (data[0] as u64)).wrapping_mul(FNV_PRIME);
        hash = (hash ^ (data[1] as u64)).wrapping_mul(FNV_PRIME);
        hash = (hash ^ (data[2] as u64)).wrapping_mul(FNV_PRIME);
        hash = (hash ^ (data[3] as u64)).wrapping_mul(FNV_PRIME);
        hash = (hash ^ (data[4] as u64)).wrapping_mul(FNV_PRIME);
        hash = (hash ^ (data[5] as u64)).wrapping_mul(FNV_PRIME);
        hash = (hash ^ (data[6] as u64)).wrapping_mul(FNV_PRIME);
        hash = (hash ^ (data[7] as u64)).wrapping_mul(FNV_PRIME);
        return hash;
    }

    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[inline(always)]
fn wymum(a: u64, b: u64) -> u64 {
    let r = (a as u128) * (b as u128);
    ((r >> 64) ^ r) as u64
}

#[inline(always)]
fn wyr8(data: &[u8]) -> u64 {
    u64::from_ne_bytes(data[0..8].try_into().unwrap())
}

#[inline(always)]
fn wyr4(data: &[u8]) -> u64 {
    u32::from_ne_bytes(data[0..4].try_into().unwrap()) as u64
}

#[inline(always)]
fn wyr3(data: &[u8], k: usize) -> u64 {
    ((data[0] as u64) << 16) | ((data[k >> 1] as u64) << 8) | (data[k - 1] as u64)
}

fn wyhash_64(mut data: &[u8], mut seed: u64) -> u64 {
    const WYP0: u64 = 0x2d358dccaa6c78a5;
    const WYP1: u64 = 0x8bb84b93962eacc9;
    const WYP2: u64 = 0x4b33a62ed433d4a3;
    const WYP3: u64 = 0x4d5a2da51de1aa47;

    let len = data.len();
    seed ^= WYP0;
    let (a, b);

    if len <= 16 {
        if len >= 4 {
            a = (wyr4(data) << 32) | wyr4(&data[len - 4..]);
            b = (wyr4(&data[(len >> 3) << 2..]) << 32) | wyr4(&data[len - 4 - ((len >> 3) << 2)..]);
        } else if len > 0 {
            a = wyr3(data, len);
            b = 0;
        } else {
            a = 0;
            b = 0;
        }
    } else {
        let mut l = len;
        if l > 48 {
            let mut see1 = seed;
            let mut see2 = seed;
            while l > 48 {
                seed = wymum(wyr8(data) ^ WYP1, wyr8(&data[8..]) ^ seed);
                see1 = wymum(wyr8(&data[16..]) ^ WYP2, wyr8(&data[24..]) ^ see1);
                see2 = wymum(wyr8(&data[32..]) ^ WYP3, wyr8(&data[40..]) ^ see2);
                data = &data[48..];
                l -= 48;
            }
            seed ^= see1 ^ see2;
        }
        while l > 16 {
            seed = wymum(wyr8(data) ^ WYP1, wyr8(&data[8..]) ^ seed);
            data = &data[16..];
            l -= 16;
        }
        a = wyr8(&data[l - 16..]);
        b = wyr8(&data[l - 8..]);
    }
    wymum(WYP1 ^ (len as u64), wymum(a ^ WYP2, b ^ seed ^ WYP3))
}

fn bench_algorithm_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithm_comparison");

    let test_keys = [
        ("8_bytes", b"usr:1001".to_vec()),
        ("32_bytes", b"orders:account:tx:482910-xyz-991".to_vec()),
        ("128_bytes", [0x5A; 128].to_vec()),
        ("1024_bytes", [0xA5; 1024].to_vec()),
    ];

    for (name, key) in test_keys {
        group.throughput(Throughput::Bytes(key.len() as u64));

        // 1. FNV-1a (Current)
        group.bench_with_input(BenchmarkId::new("fnv1a_64", name), &key, |b, k| {
            b.iter(|| {
                black_box(fnv1a_64(black_box(k.as_slice())));
            });
        });

        // 2. XXH64
        group.bench_with_input(BenchmarkId::new("xxh64", name), &key, |b, k| {
            b.iter(|| {
                black_box(twox_hash::XxHash64::oneshot(0, black_box(k.as_slice())));
            });
        });


        // 3. WyHash (64-bit)
        group.bench_with_input(BenchmarkId::new("wyhash_64", name), &key, |b, k| {
            b.iter(|| {
                black_box(wyhash_64(black_box(k.as_slice()), 0));
            });
        });
    }

    group.finish();
}

fn bench_8byte_optimization_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("8byte_comparison");
    let key_8 = b"usr:1001";
    let key_32 = b"orders:account:tx:482910-xyz-991";

    group.throughput(Throughput::Elements(1));

    assert_eq!(fnv1a_baseline(key_8), fnv1a_with_if_8(key_8));

    group.bench_function("baseline_8bytes", |b| {
        b.iter(|| {
            black_box(fnv1a_baseline(black_box(key_8)));
        });
    });

    group.bench_function("with_if_8bytes_on_8byte_key", |b| {
        b.iter(|| {
            black_box(fnv1a_with_if_8(black_box(key_8)));
        });
    });

    group.bench_function("baseline_32bytes", |b| {
        b.iter(|| {
            black_box(fnv1a_baseline(black_box(key_32)));
        });
    });

    group.bench_function("with_if_8bytes_on_32byte_key", |b| {
        b.iter(|| {
            black_box(fnv1a_with_if_8(black_box(key_32)));
        });
    });

    // Alternating scenario (branch predictor stress test)
    let keys: [&[u8]; 2] = [key_8, key_32];
    group.bench_function("baseline_alternating_keys", |b| {
        let mut i = 0;
        b.iter(|| {
            let k = keys[i & 1];
            i = i.wrapping_add(1);
            black_box(fnv1a_baseline(black_box(k)));
        });
    });

    group.bench_function("with_if_8bytes_alternating_keys", |b| {
        let mut i = 0;
        b.iter(|| {
            let k = keys[i & 1];
            i = i.wrapping_add(1);
            black_box(fnv1a_with_if_8(black_box(k)));
        });
    });

    group.finish();
}

fn bench_fnv1a_by_key_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("fnv1a_hashing");

    let test_keys = [
        ("8_bytes", b"usr:1001".to_vec()),
        ("16_bytes", b"uuid_9a8b7c6d-5e".to_vec()),
        ("32_bytes", b"orders:account:tx:482910-xyz-991".to_vec()),
        ("128_bytes", [0x5A; 128].to_vec()),
        ("1024_bytes", [0xA5; 1024].to_vec()),
    ];

    for (name, key) in test_keys {
        group.throughput(Throughput::Bytes(key.len() as u64));
        group.bench_with_input(BenchmarkId::new("key_size", name), &key, |b, k| {
            b.iter(|| {
                black_box(fnv1a_64(black_box(k.as_slice())));
            });
        });
    }

    group.finish();
}

fn bench_determine_shard(c: &mut Criterion) {
    let mut group = c.benchmark_group("determine_shard");
    let key = b"users:session:9a8b7c6d-5e4f-3a2b-1c0d-ef1234567890";

    for total_shards in [8, 64, 1024] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("total_shards", total_shards),
            &total_shards,
            |b, &shards| {
                b.iter(|| {
                    black_box(determine_shard(black_box(key), black_box(shards)));
                });
            },
        );
    }

    group.finish();
}

fn bench_router_route(c: &mut Criterion) {
    let mut group = c.benchmark_group("router");
    let router = Router::new(64);
    let key_bytes = b"inventory:sku:12345678";
    let key_str = "inventory:sku:12345678";

    group.throughput(Throughput::Elements(1));

    group.bench_function("route_bytes", |b| {
        b.iter(|| {
            black_box(router.route(black_box(key_bytes)));
        });
    });

    group.bench_function("route_str", |b| {
        b.iter(|| {
            black_box(router.route_str(black_box(key_str)));
        });
    });

    group.finish();
}

fn bench_big_endian_lsm_key_ops(c: &mut Criterion) {
    let mut group = c.benchmark_group("big_endian_shard_key");
    let raw_key = b"account:ledger:tx_001928471928";
    let encoded = ShardKey::encode_u16(42, raw_key);

    group.throughput(Throughput::Elements(1));

    group.bench_function("encode_u16", |b| {
        b.iter(|| {
            black_box(encode_shard_key_u16(black_box(42), black_box(raw_key)));
        });
    });

    group.bench_function("decode_u16", |b| {
        b.iter(|| {
            black_box(decode_shard_key_u16(black_box(&encoded)));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_algorithm_comparison,
    bench_8byte_optimization_comparison,
    bench_fnv1a_by_key_size,
    bench_determine_shard,
    bench_router_route,
    bench_big_endian_lsm_key_ops
);
criterion_main!(benches);
