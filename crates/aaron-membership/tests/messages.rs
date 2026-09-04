use aaron_membership::{Member, MemberStatus, Message};
use aaron_core::{NodeId, Uuid};
use std::net::SocketAddr;

#[test]
fn test_ping_and_ack_flatbuffers_roundtrip() {
    let node_a = NodeId::new(Uuid::new(1, 2), 10, Some(Uuid::new(99, 99)));
    let addr_a: SocketAddr = "192.168.1.10:7946".parse().unwrap();
    let sender = Member::new(node_a, addr_a);

    let node_b = NodeId::new(Uuid::new(3, 4), 20, None);
    let addr_b: SocketAddr = "192.168.1.20:7946".parse().unwrap();
    let gossip_item = Member::with_status(node_b, addr_b, MemberStatus::Suspect, 20);

    // 1. Ping roundtrip
    let ping = Message::Ping {
        seq: 42,
        sender: sender.clone(),
        gossip: vec![gossip_item.clone()],
    };

    let bytes = ping.to_bytes();
    let decoded = Message::from_bytes(&bytes).unwrap();
    assert_eq!(ping, decoded);

    // 2. Ack roundtrip
    let ack = Message::Ack {
        seq: 42,
        sender,
        gossip: vec![gossip_item],
    };

    let ack_bytes = ack.to_bytes();
    let decoded_ack = Message::from_bytes(&ack_bytes).unwrap();
    assert_eq!(ack, decoded_ack);
}

#[test]
fn test_ping_req_flatbuffers_roundtrip() {
    let sender = Member::new(
        NodeId::new(Uuid::random(), 1, None),
        "127.0.0.1:8001".parse().unwrap(),
    );
    let target = Member::new(
        NodeId::new(Uuid::random(), 2, None),
        "127.0.0.1:8002".parse().unwrap(),
    );
    let gossip_1 = Member::with_status(
        NodeId::new(Uuid::random(), 3, None),
        "127.0.0.1:8003".parse().unwrap(),
        MemberStatus::Dead,
        3,
    );

    let ping_req = Message::PingReq {
        seq: 100,
        target,
        sender,
        gossip: vec![gossip_1],
    };

    let bytes = ping_req.to_bytes();
    let decoded = Message::from_bytes(&bytes).unwrap();
    assert_eq!(ping_req, decoded);
}

#[test]
fn test_join_request_and_response_roundtrip() {
    let joiner = Member::new(
        NodeId::new(Uuid::random(), 10, None),
        "10.0.0.5:7946".parse().unwrap(),
    );

    let join_req = Message::JoinRequest {
        sender: joiner.clone(),
    };
    let req_bytes = join_req.to_bytes();
    let decoded_req = Message::from_bytes(&req_bytes).unwrap();
    assert_eq!(join_req, decoded_req);

    let seed_1 = Member::new(
        NodeId::new(Uuid::random(), 100, None),
        "10.0.0.1:7946".parse().unwrap(),
    );
    let seed_2 = Member::new(
        NodeId::new(Uuid::random(), 100, None),
        "10.0.0.2:7946".parse().unwrap(),
    );

    let cluster_id = Uuid::new(0x1111, 0x2222);
    let join_resp = Message::JoinResponse {
        cluster_id,
        members: vec![seed_1, seed_2, joiner],
    };
    let resp_bytes = join_resp.to_bytes();
    let decoded_resp = Message::from_bytes(&resp_bytes).unwrap();
    assert_eq!(join_resp, decoded_resp);
}

#[test]
fn test_invalid_corrupted_message_bytes() {
    let corrupted = [0u8; 8];
    let err = Message::from_bytes(&corrupted);
    assert!(err.is_err());
}
