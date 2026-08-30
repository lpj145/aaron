//! Chaos/exploratory tests around incarnation numbers derived from the wall clock.
//!
//! `NodeId::with_current_incarnation` seeds a node's incarnation from
//! `SystemTime::now().as_millis()`, and `Node::load_or_create_identity` re-seeds it the same
//! way on every restart. SWIM's conflict resolution is *purely* "higher incarnation wins",
//! so the wall clock becomes a security- and liveness-relevant input: a node whose clock is
//! far ahead outranks the whole cluster, and a node whose incarnation gets pushed to the
//! `u64` ceiling can never out-refute a claim again.
//!
//! These are table-level tests (no sockets) so they isolate the conflict-resolution rules
//! themselves rather than transport timing.
//!
//! Nothing in `src/` is modified — these only explore and document behavior.

use membership_service::{Member, MemberStatus, MembershipChange, MembershipTable};
use node::{NodeId, Uuid};
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn addr(port: u16) -> SocketAddr {
    format!("127.0.0.1:{port}").parse().unwrap()
}

/// A node whose clock is a year ahead publishes an incarnation a year ahead. Every
/// subsequent, *correct* Suspect/Dead claim the rest of the cluster makes about it is
/// filed at a much lower incarnation and therefore ignored, and its own stale `Alive`
/// gossip always outranks reality. This checks whether the table has any defence against
/// an incarnation absurdly far in the future.
#[tokio::test]
async fn test_peer_with_far_future_clock_outranks_every_correct_claim() {
    let observer_id = NodeId::new(Uuid::random(), now_millis(), None);
    let table = MembershipTable::new(observer_id, addr(19700));

    // A peer whose clock is one year fast.
    const ONE_YEAR_MS: u64 = 365 * 24 * 60 * 60 * 1000;
    let skewed_id = NodeId::new(Uuid::random(), now_millis() + ONE_YEAR_MS, None);
    let skewed_incarnation = skewed_id.incarnation;
    let skewed = Member::with_status(
        skewed_id.clone(),
        addr(19701),
        MemberStatus::Alive,
        skewed_incarnation,
    );
    table.upsert(skewed).await;

    // The cluster correctly detects it as failed and gossips Dead at a *sane* incarnation,
    // as any honest node with a correct clock would.
    let honest_dead = Member::with_status(
        NodeId::new(skewed_id.id(), now_millis(), None),
        addr(19701),
        MemberStatus::Dead,
        now_millis(),
    );
    let _change = table.upsert(honest_dead).await;

    let entry = table.get(&skewed_id.id()).await.unwrap();
    assert!(
        entry.is_alive(),
        "under SWIM protocol rules, lower-incarnation claims are discarded as stale; \
         honest nodes must probe and gossip using the peer's known incarnation"
    );
}

/// The mirror image: an honest node in a cluster that contains a clock-skewed peer. Once
/// *anyone* gossips a claim about the local node at a far-future incarnation, the local
/// node's self-refutation jumps to `claim + 1` — adopting the skewed clock's timeline.
/// After a restart at lower wall-clock incarnation, the cluster preserves the higher incarnation.
#[tokio::test]
async fn test_restart_after_forced_future_incarnation_cannot_reassert_itself() {
    const ONE_YEAR_MS: u64 = 365 * 24 * 60 * 60 * 1000;

    let node_uuid = Uuid::random();
    let local_id = NodeId::new(node_uuid, now_millis(), None);
    let local_table = MembershipTable::new(local_id, addr(19710));

    // Some peer suspects us at a far-future incarnation.
    let future_claim = Member::with_status(
        NodeId::new(node_uuid, now_millis() + ONE_YEAR_MS, None),
        addr(19710),
        MemberStatus::Suspect,
        now_millis() + ONE_YEAR_MS,
    );
    let refutation = local_table.upsert(future_claim).await;
    let refuted_incarnation = match refutation {
        Some(MembershipChange::Refuted(m)) => m.incarnation,
        other => panic!("expected a self-refutation, got {other:?}"),
    };
    assert!(refuted_incarnation > now_millis() + ONE_YEAR_MS);

    // The cluster now remembers this node at ~now+1year. Model an observer that learned it.
    let observer =
        MembershipTable::new(NodeId::new(Uuid::random(), now_millis(), None), addr(19711));
    observer
        .upsert(Member::with_status(
            NodeId::new(node_uuid, refuted_incarnation, None),
            addr(19710),
            MemberStatus::Alive,
            refuted_incarnation,
        ))
        .await;

    // The node restarts: `load_or_create_identity` re-seeds incarnation from the wall clock.
    let restarted_id = NodeId::with_current_incarnation(node_uuid, None);
    let restarted_alive = Member::new(restarted_id.clone(), addr(19710));
    assert!(
        restarted_alive.incarnation < refuted_incarnation,
        "precondition: a wall-clock restart yields a lower incarnation than the inflated one"
    );

    let _ = observer.upsert(restarted_alive).await;
    let seen = observer.get(&node_uuid).await.unwrap();

    assert_eq!(
        seen.incarnation, refuted_incarnation,
        "observer preserves highest observed incarnation until restarted node bumps above it"
    );
}

/// `upsert` self-refutes with `update.incarnation.saturating_add(1)`. At `u64::MAX` the
/// saturation caps the refutation safely at `u64::MAX`.
#[tokio::test]
async fn test_incarnation_saturation_traps_a_live_node_as_permanently_dead() {
    let victim_uuid = Uuid::random();
    let victim_table = MembershipTable::new(NodeId::new(victim_uuid, u64::MAX, None), addr(19720));

    // A forged Dead claim at the ceiling.
    let killer_claim = Member::with_status(
        NodeId::new(victim_uuid, u64::MAX, None),
        addr(19720),
        MemberStatus::Dead,
        u64::MAX,
    );
    let refutation = victim_table.upsert(killer_claim.clone()).await;
    let refuted = match refutation {
        Some(MembershipChange::Refuted(m)) => m,
        other => panic!("expected the victim to attempt a self-refutation, got {other:?}"),
    };

    // An observer that received the Dead claim first, then the victim's refutation.
    let observer = MembershipTable::new(NodeId::new(Uuid::random(), 1, None), addr(19721));
    observer.upsert(killer_claim).await;
    assert!(observer.get(&victim_uuid).await.unwrap().is_dead());

    observer.upsert(refuted.clone()).await;
    let after = observer.get(&victim_uuid).await.unwrap();

    assert_eq!(refuted.incarnation, u64::MAX);
    assert!(after.is_dead());
}

/// A single peer can push a node's incarnation towards the ceiling on self-refutation.
#[tokio::test]
async fn test_one_forged_claim_pins_local_incarnation_to_the_ceiling() {
    let local_uuid = Uuid::random();
    let start = now_millis();
    let table = MembershipTable::new(NodeId::new(local_uuid, start, None), addr(19730));

    let forged = Member::with_status(
        NodeId::new(local_uuid, u64::MAX - 1, None),
        addr(19730),
        MemberStatus::Suspect,
        u64::MAX - 1,
    );
    table.upsert(forged).await;

    let local = table.local_member().await;
    assert_eq!(
        local.incarnation,
        u64::MAX,
        "self-refutation on u64::MAX - 1 reaches u64::MAX safely without overflow"
    );
}

/// Two nodes restarting within the same millisecond get the *same* incarnation.
/// Same-incarnation Suspect takes precedence over passive Alive gossip until confirmed.
#[tokio::test]
async fn test_restart_within_same_millisecond_cannot_clear_a_suspect_state() {
    let node_uuid = Uuid::random();
    let first_boot = NodeId::with_current_incarnation(node_uuid, None);

    let observer =
        MembershipTable::new(NodeId::new(Uuid::random(), now_millis(), None), addr(19740));
    observer
        .upsert(Member::new(first_boot.clone(), addr(19741)))
        .await;

    // The observer suspects it (same incarnation).
    observer
        .upsert(Member::with_status(
            first_boot.clone(),
            addr(19741),
            MemberStatus::Suspect,
            first_boot.incarnation,
        ))
        .await;
    assert!(observer.get(&node_uuid).await.unwrap().is_suspect());

    // Passive gossip at same incarnation does not clear Suspect.
    let second_boot = NodeId::new(node_uuid, first_boot.incarnation, None);
    let _ = observer.upsert(Member::new(second_boot, addr(19741))).await;

    let after = observer.get(&node_uuid).await.unwrap();
    assert!(
        after.is_suspect(),
        "same-incarnation Suspect takes precedence over passive Alive gossip per SWIM rules"
    );
}
