use crate::config::ControlPlaneConfig;
use crate::handle::ControlPlaneHandle;
use crate::message::RaftMessage;
use crate::network::ControlPlaneNetworkFactory;
use crate::storage::ControlPlaneStorage;
use crate::types::Raft;
use membership_service::MembershipEvent;
use node::{read_frame_with_limit, write_frame_with_limit, DEFAULT_MAX_RAFT_FRAME_SIZE, BoxError, Context, Service, ServiceConfig};
use openraft::storage::Adaptor;
use openraft::Config as RaftConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, trace, warn};

/// Supervised Raft Control Plane Service providing cluster-wide strongly consistent state.
#[derive(Clone)]
pub struct ControlPlaneService {
    config_override: Option<ControlPlaneConfig>,
    handle: ControlPlaneHandle,
}

impl Default for ControlPlaneService {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlPlaneService {
    /// Creates a new `ControlPlaneService`.
    pub fn new() -> Self {
        Self {
            config_override: None,
            handle: ControlPlaneHandle::new(),
        }
    }

    /// Creates a new `ControlPlaneService` with an explicit configuration.
    pub fn with_config(config: ControlPlaneConfig) -> Self {
        Self {
            config_override: Some(config),
            handle: ControlPlaneHandle::new(),
        }
    }

    /// Creates a paired `(ControlPlaneService, ControlPlaneHandle)`.
    pub fn pair() -> (Self, ControlPlaneHandle) {
        let handle = ControlPlaneHandle::new();
        (
            Self {
                config_override: None,
                handle: handle.clone(),
            },
            handle,
        )
    }

    /// Creates a paired `(ControlPlaneService, ControlPlaneHandle)` with an explicit configuration.
    pub fn pair_with_config(config: ControlPlaneConfig) -> (Self, ControlPlaneHandle) {
        let handle = ControlPlaneHandle::new();
        (
            Self {
                config_override: Some(config),
                handle: handle.clone(),
            },
            handle,
        )
    }

    /// Returns a cloneable [`ControlPlaneHandle`] to interact with the replicated control plane.
    pub fn handle(&self) -> ControlPlaneHandle {
        self.handle.clone()
    }
}

impl Service for ControlPlaneService {
    type Config = ControlPlaneConfig;

    fn name(&self) -> &str {
        "control-plane-service"
    }

    fn capabilities(&self) -> Vec<&str> {
        vec!["control-plane"]
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        // 1. Resolve configuration
        let cfg = match &self.config_override {
            Some(c) => c.clone(),
            None => ControlPlaneConfig::from_env(&ctx.env)?,
        };

        // Determine local numeric Raft Node ID
        let node_id = cfg.node_id.unwrap_or_else(|| ctx.identity.id().low);

        info!(
            target: "control_plane",
            node_id = node_id,
            uuid = %ctx.identity.id(),
            bind_addr = %cfg.bind_addr,
            "Initializing Control Plane (Raft Consensus)"
        );

        // 2. Build OpenRaft configuration
        let raft_config = RaftConfig {
            cluster_name: "aaron-control-plane".to_string(),
            election_timeout_min: cfg.election_timeout_min_ms,
            election_timeout_max: cfg.election_timeout_max_ms,
            heartbeat_interval: cfg.heartbeat_interval_ms,
            snapshot_policy: openraft::SnapshotPolicy::LogsSinceLast(cfg.snapshot_threshold),
            max_in_snapshot_log_to_keep: 100,
            purge_batch_size: 100,
            ..Default::default()
        };

        let raft_config = Arc::new(
            raft_config
                .validate()
                .map_err(|e| std::io::Error::other(format!("invalid raft config: {e}")))?,
        );

        // 3. Instantiate Network and Storage implementations
        let network_factory = ControlPlaneNetworkFactory::new(ctx.network.quic.clone());
        let routing_table = network_factory.routing_table();
        let routing_table_sub = routing_table.clone();
        let mut membership_sub = ctx.event_hub.subscribe::<MembershipEvent>().await;
        let token_sub = ctx.token.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = token_sub.cancelled() => break,
                    event_res = membership_sub.recv() => {
                        match event_res {
                            Ok(MembershipEvent::Joined(m))
                            | Ok(MembershipEvent::Alive(m))
                            | Ok(MembershipEvent::Refuted(m)) => {
                                let node_id_u64 = m.node_id.id().low;
                                let cp_port = if m.addr.port() == 7946 || m.addr.port() == 17946 {
                                    18946
                                } else {
                                    m.addr.port() + 1000
                                };
                                let cp_addr = SocketAddr::new(m.addr.ip(), cp_port);
                                routing_table_sub.write().await.insert(node_id_u64, cp_addr);
                                trace!(
                                    target: "control_plane",
                                    node_id = node_id_u64,
                                    cp_addr = %cp_addr,
                                    "Updated dynamic Raft routing table from SWIM gossip"
                                );
                            }
                            Ok(_) => {}
                            Err(_) => {
                                tokio::time::sleep(Duration::from_millis(50)).await;
                            }
                        }
                    }
                }
            }
        });

        let storage = ControlPlaneStorage::new(ctx.clone(), "control-plane").await?;

        let (log_store, state_machine) = Adaptor::new(storage.clone());

        let raft = Raft::new(
            node_id,
            raft_config,
            network_factory,
            log_store,
            state_machine,
        )
        .await?;

        let metrics_rx = raft.metrics();

        self.handle.init(
            raft.clone(),
            storage.clone(),
            metrics_rx,
            node_id,
            ctx.identity.id(),
            ctx.network.quic.clone(),
            ctx.event_hub.clone(),
            routing_table.clone(),
        );

        // 4. Bind dedicated QUIC listener for Control Plane RPCs
        let endpoint: quinn::Endpoint = ctx
            .network
            .quic
            .listen_for_node(cfg.bind_addr, ctx.identity.id())
            .await?;

        info!(
            target: "control_plane",
            addr = %endpoint.local_addr()?,
            "Control Plane QUIC listener bound successfully"
        );

        let raft_clone = raft.clone();
        let token = ctx.token.clone();
        let handle_clone = self.handle.clone();

        // 5. Accept inbound Raft bi-streams
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!(target: "control_plane", "ControlPlaneService stopping on cancellation signal");
                    endpoint.close(0u32.into(), b"shutdown");
                    break;
                }
                incoming = endpoint.accept() => {
                    let connecting = match incoming {
                        Some(c) => c,
                        None => break,
                    };

                    let raft_conn = raft_clone.clone();
                    let conn_token = token.child_token();
                    let conn_event_hub = ctx.event_hub.clone();
                    let conn_handle = handle_clone.clone();

                    tokio::spawn(async move {
                        let connection: quinn::Connection = match connecting.await {
                            Ok(conn) => conn,
                            Err(e) => {
                                trace!(target: "control_plane", error = %e, "Failed to establish incoming QUIC connection");
                                return;
                            }
                        };

                        loop {
                            tokio::select! {
                                _ = conn_token.cancelled() => break,
                                stream_res = connection.accept_bi() => {
                                    let (mut send, mut recv): (quinn::SendStream, quinn::RecvStream) = match stream_res {
                                        Ok(s) => s,
                                        Err(_) => break, // Connection closed
                                    };

                                    let r = raft_conn.clone();
                                    let event_hub = conn_event_hub.clone();
                                    let handle = conn_handle.clone();
                                    tokio::spawn(async move {
                                        let rpc_task = async {
                                            let req_bytes: Vec<u8> = match read_frame_with_limit(&mut recv, DEFAULT_MAX_RAFT_FRAME_SIZE).await {
                                                Ok(Some(b)) => b,
                                                Ok(None) => return,
                                                Err(e) => {
                                                    trace!(target: "control_plane", error = %e, "Failed reading Raft frame");
                                                    return;
                                                }
                                            };

                                            let parsed = match RaftMessage::from_bytes(&req_bytes) {
                                                Ok(m) => m,
                                                Err(e) => {
                                                    warn!(target: "control_plane", error = %e, "Malformed Raft FlatBuffers message");
                                                    return;
                                                }
                                            };

                                            let resp_msg = match parsed {
                                                RaftMessage::Vote(req) => {
                                                    let resp = r.vote(req).await;
                                                    match resp {
                                                        Ok(v) => RaftMessage::VoteResp(v),
                                                        Err(e) => {
                                                            warn!(target: "control_plane", error = ?e, "Vote RPC failed");
                                                            return;
                                                        }
                                                    }
                                                }
                                                RaftMessage::Append(req) => {
                                                    let resp = r.append_entries(req).await;
                                                    match resp {
                                                        Ok(a) => RaftMessage::AppendResp(a),
                                                        Err(e) => {
                                                            warn!(target: "control_plane", error = ?e, "AppendEntries RPC failed");
                                                            return;
                                                        }
                                                    }
                                                }
                                                RaftMessage::Snapshot(req) => {
                                                    let resp = r.install_snapshot(req).await;
                                                    match resp {
                                                        Ok(s) => RaftMessage::SnapshotResp(s),
                                                        Err(e) => {
                                                            warn!(target: "control_plane", error = ?e, "InstallSnapshot RPC failed");
                                                            return;
                                                        }
                                                    }
                                                }
                                                RaftMessage::ShardCommand {
                                                    shard_id,
                                                    role: _,
                                                    primary_high,
                                                    primary_low,
                                                    replicas,
                                                    epoch,
                                                    op_type,
                                                    target_role,
                                                } => {
                                                    let primary = node::Uuid::new(primary_high, primary_low);
                                                    let replica_uuids: Vec<node::Uuid> = replicas
                                                        .into_iter()
                                                        .map(|(h, l)| node::Uuid::new(h, l))
                                                        .collect();

                                                    let member_role = match target_role {
                                                        0 => node::MemberRole::Learner,
                                                        1 => node::MemberRole::Voter,
                                                        _ => node::MemberRole::Leader,
                                                    };

                                                    match op_type {
                                                        1 => {
                                                            event_hub.publish(node::ShardEvent::RoleChanged {
                                                                shard_id,
                                                                role: member_role,
                                                            }).await;
                                                        }
                                                        2 => {
                                                            event_hub.publish(node::ShardEvent::Leave {
                                                                shard_id,
                                                            }).await;
                                                        }
                                                        _ => {
                                                            let mut members = vec![primary];
                                                            members.extend(&replica_uuids);
                                                             event_hub.publish(node::ShardEvent::Join {
                                                                shard_id,
                                                                members,
                                                                role: member_role,
                                                            }).await;
                                                        }
                                                    }

                                                    info!(
                                                        target: "control_plane",
                                                        shard_id = shard_id,
                                                        op_type = op_type,
                                                        role = ?member_role,
                                                        "Received ShardCommand frame from Control Plane, dispatching ShardEvent to EventHub"
                                                    );

                                                    RaftMessage::ShardCommandResp {
                                                        success: true,
                                                        shard_id,
                                                        current_role: target_role,
                                                        term: epoch,
                                                        reject_reason: 0,
                                                    }
                                                }
                                                RaftMessage::TelemetryHeartbeat {
                                                    node_id_high,
                                                    node_id_low,
                                                    current_wps,
                                                    error_rate,
                                                    timestamp,
                                                } => {
                                                    let node_id = node::Uuid::new(node_id_high, node_id_low);
                                                    handle.record_node_telemetry(node_id, current_wps, error_rate, timestamp).await;
                                                    RaftMessage::TelemetryHeartbeatResp { acknowledged: true }
                                                }
                                                _ => return,
                                            };

                                            let resp_bytes = resp_msg.to_bytes();
                                            if let Err(e) = write_frame_with_limit(&mut send, &resp_bytes, DEFAULT_MAX_RAFT_FRAME_SIZE).await {
                                                trace!(target: "control_plane", error = %e, "Failed writing Raft response frame");
                                                return;
                                            }

                                            let _ = send.finish();
                                        };

                                        // Apply a strict 15s timeout to avoid orphaned hung streams
                                        let _ = tokio::time::timeout(Duration::from_secs(15), rpc_task).await;
                                    });
                                }
                            }
                        }
                    });
                }
            }
        }

        let _ = raft.shutdown().await;
        Ok(())
    }
}
