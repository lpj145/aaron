use node::{CancellationToken, EventHub, QuicManager};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tracing::{debug, error, info, trace, warn};

use crate::config::MembershipConfig;
use crate::event::MembershipEvent;
use crate::member::{Member, MemberStatus};
use crate::message::Message;
use crate::stage::egress::EgressTransport;
use crate::table::{MembershipChange, MembershipTable};

/// Failure detector probe loop executing periodic SWIM probes and suspect expirations.
pub struct ProbeLoop {
    table: MembershipTable,
    event_hub: EventHub,
    quic: QuicManager,
    config: MembershipConfig,
    seq: AtomicU64,
}

impl ProbeLoop {
    /// Creates a new `ProbeLoop` instance.
    pub fn new(
        table: MembershipTable,
        event_hub: EventHub,
        quic: QuicManager,
        config: MembershipConfig,
    ) -> Self {
        Self {
            table,
            event_hub,
            quic,
            config,
            seq: AtomicU64::new(1),
        }
    }

    /// Runs the periodic failure detector loop until the cancellation token is triggered.
    pub async fn run(&self, token: CancellationToken) {
        let mut interval = tokio::time::interval(self.config.probe_interval);
        // Skip first immediate tick to allow initial network stabilization
        interval.tick().await;

        let mut tombstone_interval = tokio::time::interval(Duration::from_secs(300));
        tombstone_interval.tick().await;

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!(target: "membership", "ProbeLoop stopping on cancellation signal");
                    break;
                }
                _ = tombstone_interval.tick() => {
                    // Purge dead members older than 24 hours
                    let reaped = self.table.reap_tombstones(Duration::from_secs(86400)).await;
                    if reaped > 0 {
                        info!(target: "membership", reaped = reaped, "Purged old Dead/Left tombstones from membership table");
                    }
                }
                _ = interval.tick() => {
                    self.tick().await;
                }
            }
        }
    }

    /// Executes a single probe cycle.
    pub async fn tick(&self) {
        // 1. Expire suspects whose suspicion window has passed
        let expired = self
            .table
            .expire_suspects(self.config.suspect_timeout)
            .await;
        for dead_member in expired {
            error!(target: "membership::probe", member = %dead_member, "Suspect timeout expired: Node declared Dead");
            self.event_hub
                .publish(MembershipEvent::Dead(dead_member))
                .await;
        }

        // 2. Select random active member to probe
        let local = self.table.local_member().await;
        let target = match self.table.random_probe_target(&local.node_id.id()).await {
            Some(t) => t,
            None => {
                trace!(target: "membership::probe", "No other active members to probe");
                return;
            }
        };

        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let gossip = self
            .table
            .collect_gossip_payload(self.config.gossip_fanout)
            .await;

        let ping = Message::Ping {
            seq,
            sender: local.clone(),
            gossip,
        };

        // 3. Direct probe over QUIC
        let direct_res = EgressTransport::ping(
            &self.quic,
            target.addr,
            ping.clone(),
            self.config.probe_timeout,
        )
        .await;

        match direct_res {
            Ok(Message::Ack {
                seq: ack_seq,
                sender: ack_sender,
                gossip: ack_gossip,
            }) if ack_seq == seq => {
                trace!(target: "membership::probe", target = %target.addr, "Direct probe succeeded (Ack received)");
                self.confirm_member_alive(ack_sender).await;
                for update in ack_gossip {
                    self.process_member_update(update).await;
                }
                return;
            }
            Ok(other) => {
                debug!(target: "membership::probe", unexpected = ?other, "Received unexpected response to Ping");
            }
            Err(err) => {
                debug!(target: "membership::probe", target = %target.addr, error = %err, "Direct probe failed or timed out, attempting indirect probes");
            }
        }

        // 4. Indirect Probe (PingReq) via k random intermediaries
        let intermediaries = self
            .table
            .random_k_members(
                self.config.indirect_ping_targets,
                &[local.node_id.id(), target.node_id.id()],
            )
            .await;

        if intermediaries.is_empty() {
            // No intermediaries available -> immediately transition to Suspect
            self.mark_suspect(target).await;
            return;
        }

        let mut indirect_success = false;
        let mut tasks = Vec::new();

        for mediator in intermediaries {
            let quic_clone = self.quic.clone();
            let target_clone = target.clone();
            let local_clone = local.clone();
            let gossip_payload = self
                .table
                .collect_gossip_payload(self.config.gossip_fanout)
                .await;
            let timeout = self.config.probe_timeout;

            tasks.push(tokio::spawn(async move {
                let ping_req = Message::PingReq {
                    seq,
                    target: target_clone,
                    sender: local_clone,
                    gossip: gossip_payload,
                };

                EgressTransport::ping_req(&quic_clone, mediator.addr, ping_req, timeout).await
            }));
        }

        for task in tasks {
            if let Ok(Ok(Message::Ack {
                seq: ack_seq,
                sender: ack_sender,
                gossip: ack_gossip,
            })) = task.await
                && ack_seq == seq
            {
                indirect_success = true;
                self.confirm_member_alive(ack_sender).await;
                for u in ack_gossip {
                    self.process_member_update(u).await;
                }
                break;
            }
        }

        if indirect_success {
            trace!(target: "membership::probe", target = %target.addr, "Indirect probe succeeded");
        } else {
            // All indirect probes failed -> declare Suspect
            self.mark_suspect(target).await;
        }
    }

    async fn mark_suspect(&self, mut target: Member) {
        target.status = MemberStatus::Suspect;
        warn!(target: "membership::probe", target = %target, "Node failed direct and indirect probes: Transitioning to Suspect");
        self.process_member_update(target).await;
    }

    async fn confirm_member_alive(&self, update: Member) {
        if let Some(change) = self.table.confirm_alive(update).await {
            let event = match change {
                MembershipChange::Joined(m) => {
                    info!(target: "membership", member = %m, "Node joined cluster");
                    MembershipEvent::Joined(m)
                }
                MembershipChange::Alive(m) => {
                    debug!(target: "membership", member = %m, "Node reaffirmed Alive");
                    MembershipEvent::Alive(m)
                }
                MembershipChange::Suspect(m) => {
                    warn!(target: "membership", member = %m, "Node Suspect");
                    MembershipEvent::Suspect(m)
                }
                MembershipChange::Dead(m) => {
                    error!(target: "membership", member = %m, "Node declared Dead");
                    MembershipEvent::Dead(m)
                }
                MembershipChange::Left(m) => {
                    info!(target: "membership", member = %m, "Node Left cluster");
                    MembershipEvent::Left(m)
                }
                MembershipChange::Refuted(m) => {
                    info!(
                        target: "membership",
                        new_incarnation = m.incarnation,
                        "Local node refuted false suspicion"
                    );
                    MembershipEvent::Refuted(m)
                }
            };
            self.event_hub.publish(event).await;
        }
    }

    async fn process_member_update(&self, update: Member) {
        if let Some(change) = self.table.upsert(update).await {
            let event = match change {
                MembershipChange::Joined(m) => {
                    info!(target: "membership", member = %m, "Node joined cluster");
                    MembershipEvent::Joined(m)
                }
                MembershipChange::Alive(m) => {
                    debug!(target: "membership", member = %m, "Node reaffirmed Alive");
                    MembershipEvent::Alive(m)
                }
                MembershipChange::Suspect(m) => {
                    warn!(target: "membership", member = %m, "Node Suspect");
                    MembershipEvent::Suspect(m)
                }
                MembershipChange::Dead(m) => {
                    error!(target: "membership", member = %m, "Node declared Dead");
                    MembershipEvent::Dead(m)
                }
                MembershipChange::Left(m) => {
                    info!(target: "membership", member = %m, "Node Left cluster");
                    MembershipEvent::Left(m)
                }
                MembershipChange::Refuted(m) => {
                    info!(
                        target: "membership",
                        new_incarnation = m.incarnation,
                        "Local node refuted false suspicion"
                    );
                    MembershipEvent::Refuted(m)
                }
            };
            self.event_hub.publish(event).await;
        }
    }
}
