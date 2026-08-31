use node::{BoxError, EventHub, QuicManager, Uuid};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, watch};
use crate::config::MembershipConfig;
use crate::event::{MembershipEvent, UpdateSwimConfig};
use crate::member::Member;
use crate::stage::egress::EgressTransport;
use crate::table::{MembershipChange, MembershipTable};

#[derive(Clone)]
pub(crate) struct MembershipHandleInner {
    pub table: MembershipTable,
    pub quic: QuicManager,
    pub event_hub: EventHub,
    pub config: Arc<RwLock<MembershipConfig>>,
}

/// A cloneable, thread-safe handle for querying cluster topology and performing dynamic operations.
#[derive(Clone)]
pub struct MembershipHandle {
    inner: Arc<RwLock<Option<MembershipHandleInner>>>,
    ready_tx: Arc<watch::Sender<bool>>,
    ready_rx: watch::Receiver<bool>,
}

impl Default for MembershipHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl MembershipHandle {
    /// Creates a new uninitialized `MembershipHandle`.
    pub fn new() -> Self {
        let (ready_tx, ready_rx) = watch::channel(false);
        Self {
            inner: Arc::new(RwLock::new(None)),
            ready_tx: Arc::new(ready_tx),
            ready_rx,
        }
    }

    /// Internal method to initialize the handle when `MembershipService` starts running.
    pub(crate) async fn initialize(
        &self,
        table: MembershipTable,
        quic: QuicManager,
        event_hub: EventHub,
        config: Arc<RwLock<MembershipConfig>>,
    ) {
        let mut guard = self.inner.write().await;
        *guard = Some(MembershipHandleInner {
            table,
            quic,
            event_hub,
            config,
        });
        let _ = self.ready_tx.send(true);
    }

    /// Waits until the underlying `MembershipService` is running and initialized.
    pub async fn wait_ready(&self) {
        let mut rx = self.ready_rx.clone();
        if *rx.borrow() {
            return;
        }
        while rx.changed().await.is_ok() {
            if *rx.borrow() {
                return;
            }
        }
    }

    /// Returns a list of all active (non-Dead and non-Left) cluster members.
    pub async fn active_members(&self) -> Vec<Member> {
        let guard = self.inner.read().await;
        if let Some(inner) = &*guard {
            inner.table.all_active_members().await
        } else {
            Vec::new()
        }
    }

    /// Returns a list of all known cluster members in the topology table (including Dead and Left).
    pub async fn all_members(&self) -> Vec<Member> {
        let guard = self.inner.read().await;
        if let Some(inner) = &*guard {
            inner.table.all_members().await
        } else {
            Vec::new()
        }
    }

    /// Returns a list of all known cluster members with their last measured probe RTT latency.
    pub async fn all_members_with_rtt(&self) -> Vec<(Member, Option<Duration>)> {
        let guard = self.inner.read().await;
        if let Some(inner) = &*guard {
            inner.table.all_members_with_rtt().await
        } else {
            Vec::new()
        }
    }

    /// Returns the last measured RTT latency for a specific member UUID.
    pub async fn get_rtt(&self, id: &Uuid) -> Option<Duration> {
        let guard = self.inner.read().await;
        if let Some(inner) = &*guard {
            inner.table.get_rtt(id).await
        } else {
            None
        }
    }

    /// Returns the local node's member representation if initialized.
    pub async fn local_member(&self) -> Option<Member> {
        let guard = self.inner.read().await;
        if let Some(inner) = &*guard {
            Some(inner.table.local_member().await)
        } else {
            None
        }
    }

    /// Returns the cluster ID if initialized and established.
    pub async fn cluster_id(&self) -> Option<Uuid> {
        let guard = self.inner.read().await;
        if let Some(inner) = &*guard {
            inner.table.cluster_id().await
        } else {
            None
        }
    }

    /// Returns the active SWIM protocol configuration.
    pub async fn config(&self) -> Option<MembershipConfig> {
        let guard = self.inner.read().await;
        if let Some(inner) = &*guard {
            Some(inner.config.read().await.clone())
        } else {
            None
        }
    }

    /// Dynamically updates the active SWIM configuration parameters via EventHub.
    pub async fn update_config(&self, update: UpdateSwimConfig) {
        let guard = self.inner.read().await;
        if let Some(inner) = &*guard {
            inner.event_hub.publish(update).await;
        }
    }

    /// Looks up a specific member by its node UUID.
    pub async fn get(&self, id: &Uuid) -> Option<Member> {
        let guard = self.inner.read().await;
        if let Some(inner) = &*guard {
            inner.table.get(id).await
        } else {
            None
        }
    }

    /// Checks if a specific node is currently known and in the `Alive` state.
    pub async fn is_alive(&self, id: &Uuid) -> bool {
        self.get(id).await.is_some_and(|m| m.is_alive())
    }

    /// Triggers an on-demand cluster join to a seed node over QUIC.
    pub async fn join(&self, seed_addr: SocketAddr) -> Result<Vec<Member>, BoxError> {
        let inner = {
            let guard = self.inner.read().await;
            guard.clone().ok_or_else(|| {
                Box::new(std::io::Error::other(
                    "MembershipService is not yet running",
                )) as BoxError
            })?
        };

        let local_member = inner.table.local_member().await;
        let start_time = std::time::Instant::now();
        let (seed_cluster_id, members) = EgressTransport::join(
            &inner.quic,
            seed_addr,
            local_member,
            Duration::from_millis(2000),
        )
        .await?;
        let join_rtt = start_time.elapsed();

        if let Some(expected_cid) = inner.table.cluster_id().await {
            if seed_cluster_id != expected_cid {
                return Err(Box::new(std::io::Error::other(format!(
                    "Cluster ID mismatch: expected {expected_cid}, seed returned {seed_cluster_id}"
                ))) as BoxError);
            }
        } else {
            inner.table.set_cluster_id(seed_cluster_id).await;
        }

        for m in &members {
            if m.addr == seed_addr {
                inner.table.record_rtt(&m.node_id.id(), join_rtt).await;
            }
            if let Some(change) = inner.table.upsert(m.clone()).await {
                let event = match change {
                    MembershipChange::Joined(m) => MembershipEvent::Joined(m),
                    MembershipChange::Alive(m) => MembershipEvent::Alive(m),
                    MembershipChange::Suspect(m) => MembershipEvent::Suspect(m),
                    MembershipChange::Dead(m) => MembershipEvent::Dead(m),
                    MembershipChange::Left(m) => MembershipEvent::Left(m),
                    MembershipChange::Refuted(m) => MembershipEvent::Refuted(m),
                };
                inner.event_hub.publish(event).await;
            }
        }

        Ok(members)
    }

    /// Broadcasts a runtime configuration update (tracing, SWIM parameters, or environment variables) to all active cluster peers over QUIC.
    ///
    /// Returns `(propagated_count, failed_count)`.
    pub async fn broadcast_config_update(
        &self,
        tracing_filter: Option<String>,
        swim_config: Option<UpdateSwimConfig>,
        env_var: Option<(String, String)>,
    ) -> (usize, usize) {
        let inner = {
            let guard = self.inner.read().await;
            match &*guard {
                Some(i) => i.clone(),
                None => return (0, 0),
            }
        };

        let local_member = inner.table.local_member().await;
        let active_peers = inner.table.all_active_members().await;

        let tracing_str = tracing_filter.unwrap_or_default();
        let (pi_ms, pt_ms, st_ms, k, fanout) = match swim_config {
            Some(cfg) => (
                cfg.probe_interval.map(|d| d.as_millis() as u64).unwrap_or(0),
                cfg.probe_timeout.map(|d| d.as_millis() as u64).unwrap_or(0),
                cfg.suspect_timeout.map(|d| d.as_millis() as u64).unwrap_or(0),
                cfg.indirect_ping_targets.unwrap_or(0) as u32,
                cfg.gossip_fanout.unwrap_or(0) as u32,
            ),
            None => (0, 0, 0, 0, 0),
        };

        let (env_key, env_val) = match env_var {
            Some((k, v)) => (k, v),
            None => (String::new(), String::new()),
        };

        let msg = crate::message::Message::ConfigUpdate {
            tracing_filter: tracing_str,
            probe_interval_ms: pi_ms,
            probe_timeout_ms: pt_ms,
            suspect_timeout_ms: st_ms,
            indirect_ping_targets: k,
            gossip_fanout: fanout,
            env_key,
            env_val,
            sender: local_member.clone(),
        };

        let mut tasks = Vec::new();
        for peer in active_peers {
            if peer.node_id.id() == local_member.node_id.id() {
                continue;
            }

            let quic_clone = inner.quic.clone();
            let peer_addr = peer.addr;
            let msg_clone = msg.clone();

            tasks.push(tokio::spawn(async move {
                EgressTransport::send_config_update(
                    &quic_clone,
                    peer_addr,
                    msg_clone,
                    Duration::from_millis(1500),
                )
                .await
            }));
        }

        let mut propagated = 0;
        let mut failed = 0;

        for task in tasks {
            if let Ok(Ok(())) = task.await {
                propagated += 1;
            } else {
                failed += 1;
            }
        }

        (propagated, failed)
    }
}
