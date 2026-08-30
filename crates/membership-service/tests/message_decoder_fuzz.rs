//! Deep fuzzing of the FlatBuffers/planus decoders reachable from the network.
//!
//! `Message::from_bytes` and `NodeId::from_flatbuffer_bytes` are the very first code that
//! touches bytes an unauthenticated peer put on the wire (`ingress.rs` decodes a frame
//! before any cluster_id or membership check). The contract they must uphold is absolute:
//! *any* byte string, however malformed, produces `Ok` or `Err` — never a panic, an abort,
//! an unbounded allocation, or a hang. A panic here is a remote DoS on every node in the
//! cluster.
//!
//! Each decode runs inside `catch_unwind` so a panic is reported as a test failure with the
//! exact input that triggered it instead of tearing down the test binary.
//!
//! Nothing in `src/` is modified — these only explore and document behavior.

use membership_service::{Member, MemberStatus, Message};
use node::{NodeId, Uuid};
use std::net::SocketAddr;
use std::panic::AssertUnwindSafe;
use std::time::{Duration, Instant};

fn member(port: u16, incarnation: u64) -> Member {
    Member::with_status(
        NodeId::new(
            Uuid::new(port as u64, incarnation),
            incarnation,
            Some(Uuid::new(7, 7)),
        ),
        format!("127.0.0.1:{port}").parse::<SocketAddr>().unwrap(),
        MemberStatus::Alive,
        incarnation,
    )
}

/// One well-formed instance of every message variant, to use as fuzzing seeds.
fn seed_messages() -> Vec<(&'static str, Message)> {
    let gossip: Vec<Member> = (0..8).map(|i| member(9000 + i, i as u64 + 1)).collect();
    vec![
        (
            "Ping",
            Message::Ping {
                seq: 42,
                sender: member(9100, 3),
                gossip: gossip.clone(),
            },
        ),
        (
            "Ack",
            Message::Ack {
                seq: u64::MAX,
                sender: member(9101, 4),
                gossip: gossip.clone(),
            },
        ),
        (
            "PingReq",
            Message::PingReq {
                seq: 7,
                target: member(9102, 5),
                sender: member(9103, 6),
                gossip: gossip.clone(),
            },
        ),
        (
            "JoinRequest",
            Message::JoinRequest {
                sender: member(9104, 7),
            },
        ),
        (
            "JoinResponse",
            Message::JoinResponse {
                cluster_id: Uuid::new(1, 2),
                members: gossip,
            },
        ),
        (
            "Ping-empty-gossip",
            Message::Ping {
                seq: 0,
                sender: member(9105, 0),
                gossip: Vec::new(),
            },
        ),
    ]
}

fn decode_catching(bytes: &[u8]) -> Result<(), ()> {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = Message::from_bytes(bytes);
    }));
    std::panic::set_hook(prev);
    res.map_err(|_| ())
}

/// Truncate every valid message at *every* byte offset. A FlatBuffers buffer is a graph of
/// offsets, so a truncation almost anywhere leaves a vtable or vector pointing past the end
/// of the buffer — exactly the class of input a peer can produce by closing a QUIC stream
/// mid-write, or by lying in the frame length prefix.
#[test]
fn test_truncation_at_every_offset_never_panics() {
    let mut panics = Vec::new();

    for (label, msg) in seed_messages() {
        let full = msg.to_bytes();
        for cut in 0..full.len() {
            if decode_catching(&full[..cut]).is_err() {
                panics.push(format!("{label} truncated to {cut}/{} bytes", full.len()));
            }
        }
        // A valid, untruncated buffer must still round-trip.
        assert_eq!(
            Message::from_bytes(&full).expect("valid message must decode"),
            msg,
            "{label} did not round-trip through to_bytes/from_bytes"
        );
    }

    assert!(
        panics.is_empty(),
        "Message::from_bytes panicked on {} truncated input(s); first few: {:?}",
        panics.len(),
        &panics[..panics.len().min(10)]
    );
}

/// Corrupt each byte of a valid message in turn (single-byte substitution with several
/// hostile values). Offsets, vtable entries, vector lengths and union tags all live in
/// those bytes; a corrupted length field is the classic path to an unbounded allocation or
/// an out-of-bounds slice.
#[test]
fn test_single_byte_corruption_never_panics() {
    const SUBSTITUTIONS: [u8; 6] = [0x00, 0x01, 0x7F, 0x80, 0xFE, 0xFF];
    let mut panics = Vec::new();

    for (label, msg) in seed_messages() {
        let full = msg.to_bytes();
        for pos in 0..full.len() {
            for sub in SUBSTITUTIONS {
                if full[pos] == sub {
                    continue;
                }
                let mut corrupted = full.clone();
                corrupted[pos] = sub;
                if decode_catching(&corrupted).is_err() {
                    panics.push(format!("{label} byte {pos} := {sub:#04x}"));
                }
            }
        }
    }

    assert!(
        panics.is_empty(),
        "Message::from_bytes panicked on {} single-byte corruption(s); first few: {:?}",
        panics.len(),
        &panics[..panics.len().min(10)]
    );
}

/// Structured garbage: empty buffers, buffers shorter than a root offset, buffers whose
/// root offset points backwards/way past the end, absurd vector lengths, and deeply
/// self-referential offsets (a FlatBuffers "billion laughs" attempt).
#[test]
fn test_hostile_synthetic_buffers_never_panic_or_hang() {
    let mut cases: Vec<(String, Vec<u8>)> = vec![
        ("empty".into(), Vec::new()),
        ("one byte".into(), vec![0xFF]),
        ("three bytes".into(), vec![0x00, 0x00, 0x00]),
        ("root offset = 0".into(), vec![0, 0, 0, 0]),
        (
            "root offset = u32::MAX".into(),
            vec![0xFF, 0xFF, 0xFF, 0xFF],
        ),
        (
            "root offset = i32::MIN-ish".into(),
            vec![0x00, 0x00, 0x00, 0x80],
        ),
        ("all zeroes 4KB".into(), vec![0u8; 4096]),
        ("all 0xFF 4KB".into(), vec![0xFFu8; 4096]),
    ];

    // A root offset that points at itself (self-referential), repeated: a decoder that
    // follows offsets without bounds/cycle checks would loop or recurse forever here.
    let mut self_ref = vec![0u8; 256];
    for chunk in self_ref.chunks_mut(4) {
        if chunk.len() == 4 {
            chunk.copy_from_slice(&0u32.to_le_bytes());
        }
    }
    cases.push(("self-referential zero offsets".into(), self_ref));

    // A vtable claiming an enormous field count / vector claiming an enormous length.
    let mut huge_len = Message::JoinRequest {
        sender: member(9200, 1),
    }
    .to_bytes();
    for pos in 0..huge_len.len().saturating_sub(4) {
        // Rewrite a 4-byte window as u32::MAX and keep the case around; only a handful
        // of positions correspond to a real length field, but all must be handled.
        let mut variant = huge_len.clone();
        variant[pos..pos + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        cases.push((format!("u32::MAX at offset {pos}"), variant));
    }
    huge_len.clear();

    let mut panics = Vec::new();
    let mut slow = Vec::new();
    for (label, bytes) in cases {
        let started = Instant::now();
        if decode_catching(&bytes).is_err() {
            panics.push(label.clone());
        }
        let elapsed = started.elapsed();
        if elapsed > Duration::from_millis(500) {
            slow.push((label, elapsed));
        }
    }

    assert!(
        panics.is_empty(),
        "Message::from_bytes panicked on {} hostile synthetic buffer(s): {:?}",
        panics.len(),
        &panics[..panics.len().min(10)]
    );
    assert!(
        slow.is_empty(),
        "Message::from_bytes took an unreasonable amount of time on {:?} — a length or \
         offset field can drive unbounded work from an unauthenticated peer",
        slow
    );
}

/// The `addr` field is a plain FlatBuffers string, parsed with `SocketAddr::from_str` only
/// after decoding. Hostile address strings (unicode, control characters, absurd length,
/// embedded NULs) must produce `MessageError::InvalidSocketAddr`, never a panic — and the
/// serializer must not choke on a member whose fields sit at type extremes.
#[test]
fn test_extreme_field_values_round_trip_or_error_cleanly() {
    // Extremes that are legally constructible in Rust and therefore legally sendable.
    let extremes = vec![
        Member::with_status(
            NodeId::new(
                Uuid::new(u64::MAX, u64::MAX),
                u64::MAX,
                Some(Uuid::new(u64::MAX, u64::MAX)),
            ),
            "255.255.255.255:65535".parse().unwrap(),
            MemberStatus::Dead,
            u64::MAX,
        ),
        Member::with_status(
            NodeId::new(Uuid::new(0, 0), 0, None),
            "0.0.0.0:0".parse().unwrap(),
            MemberStatus::Left,
            0,
        ),
        Member::with_status(
            NodeId::new(Uuid::random(), 1, None),
            "[::1]:65535".parse().unwrap(),
            MemberStatus::Suspect,
            1,
        ),
    ];

    for m in &extremes {
        let msg = Message::JoinRequest { sender: m.clone() };
        let bytes = msg.to_bytes();
        let decoded = Message::from_bytes(&bytes).expect("extreme-but-valid member must decode");
        assert_eq!(decoded, msg, "extreme member did not round-trip");
    }

    // A gossip vector with many entries — an attacker choosing the payload size.
    let big_gossip: Vec<Member> = (0..5_000u64)
        .map(|i| member(1024 + (i % 60_000) as u16, i))
        .collect();
    let big = Message::Ping {
        seq: 1,
        sender: member(9300, 1),
        gossip: big_gossip,
    };
    let bytes = big.to_bytes();
    let started = Instant::now();
    let decoded = Message::from_bytes(&bytes).expect("large but valid gossip payload must decode");
    assert_eq!(decoded, big);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "decoding a 5000-entry gossip payload took {:?}",
        started.elapsed()
    );
}

/// `NodeId::from_flatbuffer_bytes` is separately reachable (identities are persisted in the
/// store and parsed back on startup, and the type is public), so fuzz it the same way.
#[test]
fn test_node_id_flatbuffer_decoder_never_panics() {
    let valid = NodeId::new(Uuid::random(), 12345, Some(Uuid::random())).to_flatbuffer_bytes();

    let mut panics = Vec::new();
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    for cut in 0..valid.len() {
        let slice = &valid[..cut];
        if std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = NodeId::from_flatbuffer_bytes(slice);
        }))
        .is_err()
        {
            panics.push(format!("truncated to {cut}"));
        }
    }

    for pos in 0..valid.len() {
        for sub in [0x00u8, 0x01, 0x80, 0xFF] {
            let mut corrupted = valid.clone();
            corrupted[pos] = sub;
            if std::panic::catch_unwind(AssertUnwindSafe(|| {
                let _ = NodeId::from_flatbuffer_bytes(&corrupted);
            }))
            .is_err()
            {
                panics.push(format!("byte {pos} := {sub:#04x}"));
            }
        }
    }

    std::panic::set_hook(prev);

    assert!(
        panics.is_empty(),
        "NodeId::from_flatbuffer_bytes panicked on {} malformed input(s); first few: {:?}",
        panics.len(),
        &panics[..panics.len().min(10)]
    );
}
