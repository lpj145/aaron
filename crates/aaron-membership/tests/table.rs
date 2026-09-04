use aaron_membership::{Member, MemberStatus, MembershipChange, MembershipTable};
use aaron_core::{NodeId, Uuid};
use std::net::SocketAddr;
use std::time::Duration;

#[tokio::test]
async fn test_table_basic_upsert_and_conflict_resolution() {
    let local_id = NodeId::new(Uuid::new(1, 1), 10, None);
    let local_addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();
    let table = MembershipTable::new(local_id.clone(), local_addr);

    // 1. Initial local member check
    let local = table.local_member().await;
    assert_eq!(local.node_id, local_id);
    assert_eq!(local.addr, local_addr);
    assert_eq!(local.status, MemberStatus::Alive);
    assert_eq!(local.incarnation, 10);

    // 2. Discover remote peer (Node 2)
    let peer_id = NodeId::new(Uuid::new(2, 2), 1, None);
    let peer_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
    let peer = Member::new(peer_id.clone(), peer_addr);

    let change = table.upsert(peer.clone()).await;
    assert_eq!(change, Some(MembershipChange::Joined(peer.clone())));

    // 3. Node 2 transitions to Suspect (same incarnation 1)
    let suspect_peer = Member::with_status(peer_id.clone(), peer_addr, MemberStatus::Suspect, 1);
    let change = table.upsert(suspect_peer.clone()).await;
    assert_eq!(change, Some(MembershipChange::Suspect(suspect_peer)));

    // 4. Stale Alive update (same incarnation 1) should be ignored while in Suspect
    let stale_alive = Member::with_status(peer_id.clone(), peer_addr, MemberStatus::Alive, 1);
    let change = table.upsert(stale_alive).await;
    assert_eq!(change, None);

    // 5. Node 2 refutes with higher incarnation 2
    let refuted_alive = Member::with_status(peer_id.clone(), peer_addr, MemberStatus::Alive, 2);
    let change = table.upsert(refuted_alive.clone()).await;
    assert_eq!(change, Some(MembershipChange::Alive(refuted_alive)));

    // 6. Old incarnation update (incarnation 1) is discarded as stale
    let old_update = Member::with_status(peer_id, peer_addr, MemberStatus::Suspect, 1);
    let change = table.upsert(old_update).await;
    assert_eq!(change, None);
}

#[tokio::test]
async fn test_table_local_node_refutation_on_false_suspicion() {
    let local_id = NodeId::new(Uuid::new(1, 1), 5, None);
    let local_addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();
    let table = MembershipTable::new(local_id.clone(), local_addr);

    // Simulate incoming gossip stating our local node is Suspect at incarnation 5
    let false_suspicion = Member::with_status(local_id, local_addr, MemberStatus::Suspect, 5);
    let change = table.upsert(false_suspicion).await;

    // Table must refute by incrementing incarnation to 6 and staying Alive
    assert!(
        matches!(change, Some(MembershipChange::Refuted(m)) if m.incarnation == 6 && m.status == MemberStatus::Alive)
    );

    let local = table.local_member().await;
    assert_eq!(local.incarnation, 6);
    assert_eq!(local.status, MemberStatus::Alive);
}

#[tokio::test]
async fn test_table_suspect_expiration_to_dead() {
    let local_id = NodeId::new(Uuid::random(), 1, None);
    let local_addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();
    let table = MembershipTable::new(local_id, local_addr);

    let peer_id = NodeId::new(Uuid::random(), 1, None);
    let peer_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
    let peer = Member::with_status(peer_id.clone(), peer_addr, MemberStatus::Suspect, 1);

    table.upsert(peer).await;

    // Immediately after insertion, it should not be expired
    let expired = table.expire_suspects(Duration::from_millis(50)).await;
    assert!(expired.is_empty());

    // Wait past the timeout
    tokio::time::sleep(Duration::from_millis(60)).await;

    let expired = table.expire_suspects(Duration::from_millis(50)).await;
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].node_id, peer_id);
    assert_eq!(expired[0].status, MemberStatus::Dead);

    // Verify member is now Dead in table
    let stored = table.get(&peer_id.id()).await.unwrap();
    assert_eq!(stored.status, MemberStatus::Dead);
}

#[tokio::test]
async fn test_table_random_probe_and_k_selection() {
    let local_id = NodeId::new(Uuid::new(0, 0), 1, None);
    let local_addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();
    let table = MembershipTable::new(local_id.clone(), local_addr);

    for i in 1..=5 {
        let peer_id = NodeId::new(Uuid::new(i, i), 1, None);
        let peer_addr: SocketAddr = format!("127.0.0.1:800{i}").parse().unwrap();
        table.upsert(Member::new(peer_id, peer_addr)).await;
    }

    // Probe target should never pick local node
    let target = table.random_probe_target(&local_id.id()).await.unwrap();
    assert_ne!(target.node_id.id(), local_id.id());

    // Selecting k=3 members
    let k_members = table.random_k_members(3, &[local_id.id()]).await;
    assert_eq!(k_members.len(), 3);

    // Active members includes local + 5 peers = 6
    let active = table.all_active_members().await;
    assert_eq!(active.len(), 6);
}
