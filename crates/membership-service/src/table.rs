use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use node::{NodeId, Uuid};

use crate::member::{Member, MemberStatus};

/// Represents a state transition in the membership table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembershipChange {
    /// A new node has joined the cluster.
    Joined(Member),
    /// A node was reaffirmed or refuted as Alive with a higher incarnation.
    Alive(Member),
    /// A node is suspected of having failed.
    Suspect(Member),
    /// A node has been declared Dead after suspect timeout or confirmation.
    Dead(Member),
    /// A node has voluntarily Left the cluster.
    Left(Member),
    /// The local node was falsely suspected and refuted with an incremented incarnation.
    Refuted(Member),
}

#[derive(Debug, Clone)]
struct TableEntry {
    member: Member,
    state_updated_at: Instant,
    last_rtt: Option<Duration>,
}

/// Thread-safe cluster membership table implementing SWIM conflict resolution rules.
#[derive(Clone)]
pub struct MembershipTable {
    local_node: Arc<RwLock<Member>>,
    entries: Arc<RwLock<HashMap<Uuid, TableEntry>>>,
    rng_state: Arc<AtomicU64>,
}

impl MembershipTable {
    /// Creates a new `MembershipTable` with the specified local node identity and bind address.
    pub fn new(local_id: NodeId, local_addr: SocketAddr) -> Self {
        Self::new_with_tags(local_id, local_addr, Vec::new())
    }

    /// Creates a new `MembershipTable` with local node identity, bind address, and initial tags.
    pub fn new_with_tags(local_id: NodeId, local_addr: SocketAddr, tags: Vec<String>) -> Self {
        let initial_seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0xDEADBEEF, |d| d.as_nanos() as u64);

        let local_member = Member::new(local_id, local_addr).with_tags(tags);

        Self {
            local_node: Arc::new(RwLock::new(local_member)),
            entries: Arc::new(RwLock::new(HashMap::new())),
            rng_state: Arc::new(AtomicU64::new(initial_seed)),
        }
    }

    /// Returns a copy of the current local member representation.
    pub async fn local_member(&self) -> Member {
        self.local_node.read().await.clone()
    }

    /// Returns the local cluster ID if configured.
    pub async fn cluster_id(&self) -> Option<Uuid> {
        self.local_node.read().await.node_id.cluster_id
    }

    /// Sets or updates the cluster ID of the local node.
    pub async fn set_cluster_id(&self, cluster_id: Uuid) {
        let mut local = self.local_node.write().await;
        local.node_id.cluster_id = Some(cluster_id);
    }

    /// Updates the local node's incarnation (used when refuting false suspicions).
    pub async fn increment_local_incarnation(&self) -> Member {
        let mut local = self.local_node.write().await;
        local.incarnation += 1;
        local.node_id.incarnation = local.incarnation;
        local.status = MemberStatus::Alive;
        local.clone()
    }

    /// Applies a member update according to SWIM incarnation and state precedence rules.
    pub async fn upsert(&self, update: Member) -> Option<MembershipChange> {
        let (local_id, local_cluster) = {
            let local = self.local_node.read().await;
            (local.node_id.id(), local.node_id.cluster_id)
        };

        // Cluster isolation: if local cluster is established, drop updates with mismatched cluster_id
        if let Some(expected_cluster) = local_cluster
            && update.node_id.cluster_id != Some(expected_cluster)
        {
            return None;
        }

        // 1. Handling updates regarding our own local node
        if update.node_id.id() == local_id {
            let mut local = self.local_node.write().await;
            if update.status != MemberStatus::Alive && update.incarnation >= local.incarnation {
                // False suspicion, death, or leave claim: Refute by incrementing local incarnation
                local.incarnation = update.incarnation.saturating_add(1);
                local.node_id.incarnation = local.incarnation;
                local.status = MemberStatus::Alive;
                return Some(MembershipChange::Refuted(local.clone()));
            }
            return None;
        }

        // 2. Handling updates for remote nodes
        let mut table = self.entries.write().await;
        let id = update.node_id.id();

        if let Some(entry) = table.get_mut(&id) {
            let current = &mut entry.member;

            // Higher incarnation always takes precedence
            if update.incarnation > current.incarnation {
                *current = update.clone();
                entry.state_updated_at = Instant::now();

                return match update.status {
                    MemberStatus::Alive => Some(MembershipChange::Alive(update)),
                    MemberStatus::Suspect => Some(MembershipChange::Suspect(update)),
                    MemberStatus::Dead => Some(MembershipChange::Dead(update)),
                    MemberStatus::Left => Some(MembershipChange::Left(update)),
                };
            }

            // Same incarnation: sync tags if available, and apply state transition hierarchy
            if update.incarnation == current.incarnation {
                if !update.tags.is_empty() && current.tags != update.tags {
                    current.tags = update.tags.clone();
                }

                if current.status == MemberStatus::Alive && update.status == MemberStatus::Suspect {
                    current.status = MemberStatus::Suspect;
                    entry.state_updated_at = Instant::now();
                    return Some(MembershipChange::Suspect(current.clone()));
                }

                if (current.status == MemberStatus::Alive
                    || current.status == MemberStatus::Suspect)
                    && update.status == MemberStatus::Dead
                {
                    current.status = MemberStatus::Dead;
                    entry.state_updated_at = Instant::now();
                    return Some(MembershipChange::Dead(current.clone()));
                }

                if (current.status == MemberStatus::Alive
                    || current.status == MemberStatus::Suspect)
                    && update.status == MemberStatus::Left
                {
                    current.status = MemberStatus::Left;
                    entry.state_updated_at = Instant::now();
                    return Some(MembershipChange::Left(current.clone()));
                }
            }

            // Stale update (lower incarnation or lower status priority) -> discard
            None
        } else {
            // New member discovered
            let change = match update.status {
                MemberStatus::Alive => MembershipChange::Joined(update.clone()),
                MemberStatus::Suspect => MembershipChange::Suspect(update.clone()),
                MemberStatus::Dead => MembershipChange::Dead(update.clone()),
                MemberStatus::Left => MembershipChange::Left(update.clone()),
            };

            table.insert(
                id,
                TableEntry {
                    member: update,
                    state_updated_at: Instant::now(),
                    last_rtt: None,
                },
            );

            Some(change)
        }
    }

    /// Gets a specific member by its UUID (including the local node).
    pub async fn get(&self, id: &Uuid) -> Option<Member> {
        let local = self.local_node.read().await;
        if local.node_id.id() == *id {
            return Some(local.clone());
        }

        let table = self.entries.read().await;
        table.get(id).map(|e| e.member.clone())
    }

    /// Returns a list of all active (non-Dead and non-Left) members, including local node.
    pub async fn all_active_members(&self) -> Vec<Member> {
        let local = self.local_node.read().await.clone();
        let table = self.entries.read().await;

        let mut list = vec![local];
        for entry in table.values() {
            if entry.member.status != MemberStatus::Dead
                && entry.member.status != MemberStatus::Left
            {
                list.push(entry.member.clone());
            }
        }
        list
    }

    /// Returns a list of all known members in the table (including Dead and Left), plus local node.
    pub async fn all_members(&self) -> Vec<Member> {
        let local = self.local_node.read().await.clone();
        let table = self.entries.read().await;

        let mut list = vec![local];
        for entry in table.values() {
            list.push(entry.member.clone());
        }
        list
    }

    /// Records the observed probe round-trip latency (RTT) for a member.
    pub async fn record_rtt(&self, id: &Uuid, rtt: Duration) {
        let mut table = self.entries.write().await;
        if let Some(entry) = table.get_mut(id) {
            entry.last_rtt = Some(rtt);
        }
    }

    /// Retrieves the last measured RTT latency for a member.
    pub async fn get_rtt(&self, id: &Uuid) -> Option<Duration> {
        let table = self.entries.read().await;
        table.get(id).and_then(|e| e.last_rtt)
    }

    /// Returns a list of all known members along with their last observed RTT latency.
    pub async fn all_members_with_rtt(&self) -> Vec<(Member, Option<Duration>)> {
        let local = self.local_node.read().await.clone();
        let table = self.entries.read().await;

        let mut list = vec![(local, Some(Duration::ZERO))];
        for entry in table.values() {
            list.push((entry.member.clone(), entry.last_rtt));
        }
        list
    }

    /// Selects a random active peer (excluding local node and specified exclusion IDs) to probe.
    pub async fn random_probe_target(&self, exclude: &Uuid) -> Option<Member> {
        let table = self.entries.read().await;
        let mut candidates: Vec<Member> = table
            .values()
            .filter(|e| {
                e.member.node_id.id() != *exclude
                    && e.member.status != MemberStatus::Dead
                    && e.member.status != MemberStatus::Left
            })
            .map(|e| e.member.clone())
            .collect();

        if candidates.is_empty() {
            return None;
        }

        let idx = (self.next_rand() as usize) % candidates.len();
        Some(candidates.swap_remove(idx))
    }

    /// Selects up to `k` random active members (excluding given IDs) for indirect pings (`PingReq`) or gossip.
    pub async fn random_k_members(&self, k: usize, exclude: &[Uuid]) -> Vec<Member> {
        if k == 0 {
            return Vec::new();
        }

        let table = self.entries.read().await;
        let mut candidates: Vec<Member> = table
            .values()
            .filter(|e| {
                !exclude.contains(&e.member.node_id.id())
                    && e.member.status != MemberStatus::Dead
                    && e.member.status != MemberStatus::Left
            })
            .map(|e| e.member.clone())
            .collect();

        let mut selected = Vec::with_capacity(k.min(candidates.len()));
        while selected.len() < k && !candidates.is_empty() {
            let idx = (self.next_rand() as usize) % candidates.len();
            selected.push(candidates.swap_remove(idx));
        }

        selected
    }

    /// Collects up to `max_items` members to piggyback as gossip updates in ping/ack messages.
    pub async fn collect_gossip_payload(&self, max_items: usize) -> Vec<Member> {
        let local = self.local_node.read().await.clone();
        if max_items <= 1 {
            return vec![local];
        }

        let table = self.entries.read().await;
        let mut priority_candidates: Vec<Member> = Vec::new();
        let mut normal_candidates: Vec<Member> = Vec::new();

        for entry in table.values() {
            if entry.member.status != MemberStatus::Alive {
                priority_candidates.push(entry.member.clone());
            } else {
                normal_candidates.push(entry.member.clone());
            }
        }

        let mut gossip = vec![local];
        let target_len = max_items.min(priority_candidates.len() + normal_candidates.len() + 1);

        // Prioritize dissemination of state transitions (Suspect, Dead, Left)
        while gossip.len() < target_len && !priority_candidates.is_empty() {
            let idx = (self.next_rand() as usize) % priority_candidates.len();
            gossip.push(priority_candidates.swap_remove(idx));
        }

        // Fill remaining slots with healthy candidates
        while gossip.len() < target_len && !normal_candidates.is_empty() {
            let idx = (self.next_rand() as usize) % normal_candidates.len();
            gossip.push(normal_candidates.swap_remove(idx));
        }

        gossip
    }

    /// Authoritatively confirms a peer as Alive from a direct or indirect Ack response.
    ///
    /// Unlike passive gossip, a direct Ack response directly from the peer proves
    /// it is currently reachable, allowing clearance of `Suspect` status even within
    /// the same incarnation number.
    pub async fn confirm_alive(&self, update: Member) -> Option<MembershipChange> {
        let (local_id, local_cluster) = {
            let local = self.local_node.read().await;
            (local.node_id.id(), local.node_id.cluster_id)
        };

        if let Some(expected_cluster) = local_cluster
            && update.node_id.cluster_id != Some(expected_cluster)
        {
            return None;
        }

        if update.node_id.id() == local_id {
            return None;
        }

        let mut table = self.entries.write().await;
        let id = update.node_id.id();

        if let Some(entry) = table.get_mut(&id) {
            let current = &mut entry.member;
            if update.incarnation > current.incarnation {
                *current = update.clone();
                entry.state_updated_at = Instant::now();
                return match update.status {
                    MemberStatus::Alive => Some(MembershipChange::Alive(update)),
                    MemberStatus::Suspect => Some(MembershipChange::Suspect(update)),
                    MemberStatus::Dead => Some(MembershipChange::Dead(update)),
                    MemberStatus::Left => Some(MembershipChange::Left(update)),
                };
            } else if update.incarnation == current.incarnation
                && current.status == MemberStatus::Suspect
            {
                current.status = MemberStatus::Alive;
                entry.state_updated_at = Instant::now();
                return Some(MembershipChange::Alive(current.clone()));
            }
            None
        } else {
            let change = match update.status {
                MemberStatus::Alive => MembershipChange::Joined(update.clone()),
                MemberStatus::Suspect => MembershipChange::Suspect(update.clone()),
                MemberStatus::Dead => MembershipChange::Dead(update.clone()),
                MemberStatus::Left => MembershipChange::Left(update.clone()),
            };
            table.insert(
                id,
                TableEntry {
                    member: update,
                    state_updated_at: Instant::now(),
                    last_rtt: None,
                },
            );
            Some(change)
        }
    }

    /// Purges Dead and Left members that have been in that state for longer than `tombstone_timeout`.
    pub async fn reap_tombstones(&self, tombstone_timeout: Duration) -> usize {
        let mut table = self.entries.write().await;
        let now = Instant::now();
        let initial_count = table.len();
        table.retain(|_, entry| {
            if entry.member.status == MemberStatus::Dead
                || entry.member.status == MemberStatus::Left
            {
                now.duration_since(entry.state_updated_at) < tombstone_timeout
            } else {
                true
            }
        });
        initial_count.saturating_sub(table.len())
    }

    /// Scans for suspect members whose suspicion window has exceeded `suspect_timeout`
    /// and transitions them to `Dead`.
    pub async fn expire_suspects(&self, suspect_timeout: Duration) -> Vec<Member> {
        let mut table = self.entries.write().await;
        let now = Instant::now();
        let mut expired = Vec::new();

        for entry in table.values_mut() {
            if entry.member.status == MemberStatus::Suspect
                && now.duration_since(entry.state_updated_at) >= suspect_timeout
            {
                entry.member.status = MemberStatus::Dead;
                entry.state_updated_at = now;
                expired.push(entry.member.clone());
            }
        }

        expired
    }

    /// Lightweight thread-safe pseudo-random number generator (xorshift64).
    fn next_rand(&self) -> u64 {
        let _ = self
            .rng_state
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |mut x| {
                if x == 0 {
                    x = 0x853c49e6748fea9b;
                }
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                Some(x)
            });
        self.rng_state.load(Ordering::Relaxed)
    }

    /// Immediately deletes a member from the table (used when explicitly purging a removed node).
    pub async fn delete(&self, id: &Uuid) -> bool {
        let mut table = self.entries.write().await;
        table.remove(id).is_some()
    }
}
