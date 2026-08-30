use membership_service::{Member, MemberStatus, MembershipEvent};
use node::{EventHub, NodeId, Uuid};
use std::net::SocketAddr;

#[test]
fn test_member_creation_and_status_checks() {
    let node_id = NodeId::new(Uuid::new(0xAAAA, 0xBBBB), 100, None);
    let addr: SocketAddr = "192.168.1.50:7946".parse().unwrap();

    let member = Member::new(node_id.clone(), addr);

    assert_eq!(member.node_id, node_id);
    assert_eq!(member.addr, addr);
    assert_eq!(member.status, MemberStatus::Alive);
    assert_eq!(member.incarnation, 100);
    assert!(member.is_alive());
    assert!(!member.is_suspect());
    assert!(!member.is_dead());
    assert!(!member.is_left());
}

#[test]
fn test_member_status_transitions() {
    let node_id = NodeId::new(Uuid::random(), 10, None);
    let addr: SocketAddr = "127.0.0.1:7946".parse().unwrap();

    let mut member = Member::with_status(node_id, addr, MemberStatus::Suspect, 10);
    assert!(member.is_suspect());

    member.status = MemberStatus::Dead;
    assert!(member.is_dead());

    member.status = MemberStatus::Left;
    assert!(member.is_left());
}

#[tokio::test]
async fn test_member_events_published_to_event_hub() {
    let event_hub = EventHub::new();
    let mut sub = event_hub.subscribe::<MembershipEvent>().await;

    let node_id = NodeId::new(Uuid::random(), 1, None);
    let addr: SocketAddr = "192.168.1.100:7946".parse().unwrap();
    let member = Member::new(node_id.clone(), addr);

    // 1. Publish MembershipEvent::Joined
    event_hub
        .publish(MembershipEvent::Joined(member.clone()))
        .await;
    let event = sub.recv().await.unwrap();
    assert_eq!(event, MembershipEvent::Joined(member.clone()));
    assert_eq!(event.member(), &member);

    // 2. Publish MembershipEvent::Suspect
    let suspect_member = Member::with_status(node_id.clone(), addr, MemberStatus::Suspect, 1);
    event_hub
        .publish(MembershipEvent::Suspect(suspect_member.clone()))
        .await;
    let event = sub.recv().await.unwrap();
    assert_eq!(event, MembershipEvent::Suspect(suspect_member));

    // 3. Publish MembershipEvent::Alive (refutation with higher incarnation)
    let alive_refuted = Member::with_status(node_id.clone(), addr, MemberStatus::Alive, 2);
    event_hub
        .publish(MembershipEvent::Alive(alive_refuted.clone()))
        .await;
    let event = sub.recv().await.unwrap();
    assert_eq!(event, MembershipEvent::Alive(alive_refuted));

    // 4. Publish MembershipEvent::Dead
    let dead_member = Member::with_status(node_id.clone(), addr, MemberStatus::Dead, 2);
    event_hub
        .publish(MembershipEvent::Dead(dead_member.clone()))
        .await;
    let event = sub.recv().await.unwrap();
    assert_eq!(event, MembershipEvent::Dead(dead_member));

    // 5. Publish MembershipEvent::Left
    let left_member = Member::with_status(node_id.clone(), addr, MemberStatus::Left, 2);
    event_hub
        .publish(MembershipEvent::Left(left_member.clone()))
        .await;
    let event = sub.recv().await.unwrap();
    assert_eq!(event, MembershipEvent::Left(left_member));
}
