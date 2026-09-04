use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use membership_service::{Member, MemberStatus};
use node::Uuid;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;

use crate::state::AppState;

#[derive(Serialize)]
pub struct MemberInfoResponse {
    pub id: String,
    pub addr: String,
    pub hostname: Option<String>,
    pub tags: Vec<String>,
    pub status: String,
    pub incarnation: u64,
    pub is_local: bool,
    pub rtt_us: Option<u64>,
    pub rtt_ms: Option<f64>,
    pub raft_node_id: Option<u64>,
    pub raft_role: String,
    pub raft_addr: String,
    pub wps: Option<u32>,
    pub nominal_wps: Option<u32>,
    pub error_rate: Option<u32>,
}

pub fn derive_cp_port(swim_port: u16) -> u16 {
    match swim_port {
        7946 | 17946 => 18946,
        other if other < 10000 => other + 11000,
        other => other + 1000,
    }
}

impl MemberInfoResponse {
    #[allow(clippy::too_many_arguments)]
    fn from_member_with_rtt_and_raft(
        m: Member,
        local_id: Uuid,
        fallback_node_name: Option<String>,
        rtt: Option<std::time::Duration>,
        voter_ids: &BTreeSet<u64>,
        learner_ids: &BTreeSet<u64>,
        uuid_to_raft: &BTreeMap<Uuid, (u64, String)>,
        current_leader: Option<u64>,
        is_local_leader: bool,
        live_telemetry: Option<&control_plane_service::NodeTelemetrySnapshot>,
        local_telemetry: Option<&node::NodeTelemetry>,
    ) -> Self {
        let is_local = m.node_id.id() == local_id;
        let status = match m.status {
            MemberStatus::Alive => "Alive",
            MemberStatus::Suspect => "Suspect",
            MemberStatus::Dead => "Dead",
            MemberStatus::Left => "Left",
        }
        .to_string();

        let (rtt_us, rtt_ms) = if is_local {
            (Some(0), Some(0.0))
        } else if let Some(d) = rtt {
            let us = d.as_micros() as u64;
            let ms = d.as_secs_f64() * 1000.0;
            (Some(us), Some(ms))
        } else {
            (None, None)
        };

        let member_uuid = m.node_id.id();
        let (raft_node_id, raft_addr) = if let Some((nid, addr)) = uuid_to_raft.get(&member_uuid) {
            (*nid, addr.clone())
        } else {
            let nid = member_uuid.low;
            let port = derive_cp_port(m.addr.port());
            (nid, format!("{}:{}", m.addr.ip(), port))
        };

        let raft_role = if Some(raft_node_id) == current_leader || (is_local && is_local_leader) {
            "leader".to_string()
        } else if voter_ids.contains(&raft_node_id) {
            "voter".to_string()
        } else if learner_ids.contains(&raft_node_id) {
            "learner".to_string()
        } else {
            "member".to_string()
        };

        let hostname = m
            .tags
            .iter()
            .find(|t| t.starts_with("host:"))
            .map(|t| t.trim_start_matches("host:").to_string())
            .or(fallback_node_name);

        let nominal_wps = if is_local {
            local_telemetry.map(|lt| lt.nominal_wps())
        } else {
            m.tags
                .iter()
                .find(|t| t.starts_with("wps:"))
                .and_then(|t| t.trim_start_matches("wps:").parse::<u32>().ok())
        };

        let idle_baseline = nominal_wps.map(|n| (n / 10).max(40));
        let (wps, error_rate) = if is_local {
            if let Some(lt) = local_telemetry {
                (Some(lt.current_wps()), Some(lt.error_rate()))
            } else {
                (idle_baseline, Some(0))
            }
        } else if let Some(snap) = live_telemetry {
            (Some(snap.current_wps), Some(snap.error_rate))
        } else {
            let e = m
                .tags
                .iter()
                .find(|t| t.starts_with("err:"))
                .and_then(|t| t.trim_start_matches("err:").parse::<u32>().ok())
                .or(Some(0));
            (idle_baseline, e)
        };

        Self {
            id: member_uuid.to_string(),
            addr: m.addr.to_string(),
            hostname,
            tags: m.tags,
            status,
            incarnation: m.incarnation,
            is_local,
            rtt_us,
            rtt_ms,
            raft_node_id: Some(raft_node_id),
            raft_role,
            raft_addr,
            wps,
            nominal_wps,
            error_rate,
        }
    }
}

#[derive(Serialize)]
pub struct ClusterInfoResponse {
    pub cluster_id: Option<String>,
    pub local_member: Option<MemberInfoResponse>,
    pub members: Vec<MemberInfoResponse>,
    pub active_count: usize,
    pub total_count: usize,
}

pub async fn get_cluster_info(State(state): State<AppState>) -> Json<ClusterInfoResponse> {
    let local_id = state.ctx.identity.id();

    // Extract OpenRaft cluster state if control_plane is attached
    let mut voter_ids = BTreeSet::new();
    let mut learner_ids = BTreeSet::new();
    let mut uuid_to_raft = BTreeMap::new();
    let mut current_leader = None;
    let mut is_local_leader = false;

    if let Some(ref cp) = state.control_plane {
        is_local_leader = cp.is_leader();
        current_leader = cp.current_leader();

        if let Some(m) = cp.metrics() {
            for vid in m.membership_config.membership().voter_ids() {
                voter_ids.insert(vid);
            }
            for (nid, node) in m.membership_config.membership().nodes() {
                if !voter_ids.contains(nid) {
                    learner_ids.insert(*nid);
                }
                uuid_to_raft.insert(node.node_uuid(), (*nid, node.addr.clone()));
            }
        }
    }

    let live_telemetry = if let Some(ref cp) = state.control_plane {
        cp.all_node_telemetry().await
    } else {
        std::collections::HashMap::new()
    };

    let pod_names = if let Some(ref k) = state.kube {
        k.resolve_all().await
    } else {
        std::collections::HashMap::new()
    };
    let local_pod_name = state.ctx.env.get::<String>("POD_NAME");

    if let Some(ref handle) = state.membership {
        let cluster_id = handle.cluster_id().await.map(|c| c.to_string());
        let local_member = handle
            .local_member()
            .await
            .map(|m| {
                let name = local_pod_name.clone().or_else(|| pod_names.get(&m.addr.ip().to_string()).cloned());
                let snap = live_telemetry.get(&m.node_id.id());
                MemberInfoResponse::from_member_with_rtt_and_raft(
                    m,
                    local_id,
                    name,
                    Some(std::time::Duration::ZERO),
                    &voter_ids,
                    &learner_ids,
                    &uuid_to_raft,
                    current_leader,
                    is_local_leader,
                    snap,
                    Some(&state.ctx.telemetry),
                )
            });

        let all_with_rtt = handle.all_members_with_rtt().await;
        let active = handle.active_members().await;

        let members: Vec<MemberInfoResponse> = all_with_rtt
            .into_iter()
            .map(|(m, rtt)| {
                let is_local = m.node_id.id() == local_id;
                let name = if is_local && local_pod_name.is_some() {
                    local_pod_name.clone()
                } else {
                    pod_names.get(&m.addr.ip().to_string()).cloned()
                };
                let snap = live_telemetry.get(&m.node_id.id());
                MemberInfoResponse::from_member_with_rtt_and_raft(
                    m,
                    local_id,
                    name,
                    rtt,
                    &voter_ids,
                    &learner_ids,
                    &uuid_to_raft,
                    current_leader,
                    is_local_leader,
                    snap,
                    Some(&state.ctx.telemetry),
                )
            })
            .collect();

        Json(ClusterInfoResponse {
            cluster_id,
            local_member,
            active_count: active.len(),
            total_count: members.len(),
            members,
        })
    } else {
        Json(ClusterInfoResponse {
            cluster_id: state.ctx.identity.cluster_id.map(|c| c.to_string()),
            local_member: None,
            members: Vec::new(),
            active_count: 1,
            total_count: 1,
        })
    }
}

#[derive(Deserialize)]
pub struct JoinRequest {
    pub seed: String,
}

#[derive(Serialize)]
pub struct JoinResponse {
    pub success: bool,
    pub discovered_peers: usize,
    pub message: String,
}

pub async fn join_cluster(
    State(state): State<AppState>,
    Json(payload): Json<JoinRequest>,
) -> Result<Json<JoinResponse>, (StatusCode, Json<serde_json::Value>)> {
    let handle = state.membership.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "MembershipService is not attached to this Node" })),
        )
    })?;

    let seed_trim = payload.seed.trim();
    let seed_addrs: Vec<SocketAddr> = if let Ok(addr) = seed_trim.parse::<SocketAddr>() {
        vec![addr]
    } else {
        match tokio::net::lookup_host(seed_trim).await {
            Ok(addrs) => {
                let list: Vec<SocketAddr> = addrs.collect();
                if list.is_empty() {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": format!("DNS resolved zero socket addresses for seed host '{}'", seed_trim) })),
                    ));
                }
                list
            }
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Could not resolve seed host '{}': {}", seed_trim, e) })),
                ));
            }
        }
    };

    let mut last_error = None;
    for seed_addr in seed_addrs {
        match handle.join(seed_addr).await {
            Ok(peers) => {
                return Ok(Json(JoinResponse {
                    success: true,
                    discovered_peers: peers.len(),
                    message: format!("Successfully joined cluster via seed '{}'", seed_addr),
                }));
            }
            Err(err) => {
                last_error = Some(err);
            }
        }
    }

    let err_msg = last_error.map(|e| e.to_string()).unwrap_or_else(|| "No reachable seed addresses".to_string());
    Err((
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": format!("Failed to join cluster: {}", err_msg) })),
    ))
}

pub async fn leave_cluster(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Some(ref handle) = state.membership
        && let Some(mut local) = handle.local_member().await
    {
        local.status = MemberStatus::Left;
        local.incarnation = local.incarnation.saturating_add(1);
        state
            .ctx
            .event_hub
            .publish(membership_service::MembershipEvent::Left(local))
            .await;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Node broadcasted Left status"
    })))
}

#[derive(Deserialize, Default)]
pub struct StartNodeRequest {
    pub service_name: Option<String>,
    pub node_id: Option<String>,
    pub addr: Option<String>,
}

pub async fn start_node(
    State(state): State<AppState>,
    Json(payload): Json<StartNodeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let service_name = payload
        .service_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| state.ctx.service_name.clone());

    let uuid = if let Some(ref nid) = payload.node_id
        && !nid.trim().is_empty()
    {
        nid.trim().parse::<node::Uuid>().map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid Node UUID '{nid}': {e}") })),
            )
        })?
    } else {
        node::Uuid::random()
    };

    let addr = payload.addr.filter(|s| !s.trim().is_empty());

    state
        .ctx
        .event_hub
        .publish(node::NodeEvent::StartNode {
            service_name: service_name.clone(),
            node_id: uuid,
            addr,
        })
        .await;

    Ok(Json(serde_json::json!({
        "success": true,
        "node_id": uuid.to_string(),
        "message": format!("StartNode event published for node {uuid}")
    })))
}

#[derive(Deserialize)]
pub struct RemoveNodeRequest {
    pub node_id: String,
}

pub async fn remove_node(
    State(state): State<AppState>,
    Json(payload): Json<RemoveNodeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let uuid = payload.node_id.trim().parse::<node::Uuid>().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid Node UUID '{}': {}", payload.node_id, e) })),
        )
    })?;

    // Verify node is not part of the active Raft consensus
    if let Some(ref cp) = state.control_plane
        && let Some(metrics) = cp.metrics() {
            let nid = uuid.low;
            if metrics.membership_config.membership().voter_ids().any(|v| v == nid) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("Node '{uuid}' is still an active Raft voter. Demote from Raft quorum first before removing from cluster.")
                    })),
                ));
            }
            if metrics.membership_config.membership().nodes().any(|(n, _)| *n == nid) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("Node '{uuid}' is still registered in Raft control plane. Remove it from Raft first.")
                    })),
                ));
            }
        }

    if let Some(ref handle) = state.membership {
        let _ = handle.remove_member(uuid).await;
    }

    state
        .ctx
        .event_hub
        .publish(node::NodeEvent::RemoveNode { node_id: uuid })
        .await;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Node {uuid} removed from cluster")
    })))
}
