use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use membership_service::{Member, MemberStatus};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::state::AppState;

#[derive(Serialize)]
pub struct MemberInfoResponse {
    pub id: String,
    pub addr: String,
    pub status: String,
    pub incarnation: u64,
    pub is_local: bool,
    pub rtt_us: Option<u64>,
    pub rtt_ms: Option<f64>,
}

impl MemberInfoResponse {
    fn from_member_with_rtt(
        m: Member,
        local_id: node::Uuid,
        rtt: Option<std::time::Duration>,
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

        Self {
            id: m.node_id.id().to_string(),
            addr: m.addr.to_string(),
            status,
            incarnation: m.incarnation,
            is_local,
            rtt_us,
            rtt_ms,
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

    if let Some(ref handle) = state.membership {
        let cluster_id = handle.cluster_id().await.map(|c| c.to_string());
        let local_member = handle
            .local_member()
            .await
            .map(|m| MemberInfoResponse::from_member_with_rtt(m, local_id, Some(std::time::Duration::ZERO)));

        let all_with_rtt = handle.all_members_with_rtt().await;
        let active = handle.active_members().await;

        let members: Vec<MemberInfoResponse> = all_with_rtt
            .into_iter()
            .map(|(m, rtt)| MemberInfoResponse::from_member_with_rtt(m, local_id, rtt))
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

    let seed_addr: SocketAddr = payload.seed.trim().parse().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid seed address '{}': {}", payload.seed, e) })),
        )
    })?;

    match handle.join(seed_addr).await {
        Ok(peers) => Ok(Json(JoinResponse {
            success: true,
            discovered_peers: peers.len(),
            message: format!("Successfully joined cluster via seed '{}'", seed_addr),
        })),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to join cluster: {}", err) })),
        )),
    }
}

pub async fn leave_cluster(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // If membership handle is active, emit left
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
