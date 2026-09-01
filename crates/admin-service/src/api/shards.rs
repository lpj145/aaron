use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use node::Uuid;
use serde::{Deserialize, Serialize};
use shard_service::{ShardCoordinator, ShardPlacement};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_shards_overview))
        .route("/bootstrap", post(bootstrap_round_robin))
        .route("/assign", post(assign_manual))
}

#[derive(Serialize)]
struct ShardsOverviewResponse {
    total_shards: u32,
    assigned_count: usize,
    is_bootstrapped: bool,
    is_control_plane_ready: bool,
    is_leader: bool,
    current_leader: Option<u64>,
    placements: Vec<ShardPlacement>,
}

async fn get_shards_overview(State(state): State<AppState>) -> impl IntoResponse {
    let shard_handle = match &state.shard {
        Some(h) => h,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Shard service is not initialized on this node" })),
            )
        }
    };

    let total_shards = shard_handle.total_shards().await;
    let mut is_bootstrapped = shard_handle.is_bootstrapped().await;
    let mut placements = shard_handle.all_placements().await;

    // Se o Control Plane tiver placements no Raft ainda não sincronizados, mescla
    if let Some(cp) = &state.control_plane {
        let all_data = cp.all_data().await;
        if all_data.contains_key("shards/system/bootstrapped") {
            is_bootstrapped = true;
        }
        for (k, v) in all_data {
            if let Some(shard_id_str) = k.strip_prefix("shards/") {
                if let Ok(shard_id) = shard_id_str.parse::<u32>() {
                    if let Ok(placement) = serde_json::from_str::<ShardPlacement>(&v) {
                        if !placements.iter().any(|p| p.shard_id == shard_id) {
                            placements.push(placement);
                        }
                    }
                }
            }
        }
    }

    if !placements.is_empty() {
        is_bootstrapped = true;
    }

    placements.sort_by_key(|p| p.shard_id);

    let is_leader = state
        .control_plane
        .as_ref()
        .map(|cp| cp.is_leader())
        .unwrap_or(false);

    let current_leader = state
        .control_plane
        .as_ref()
        .and_then(|cp| cp.current_leader());

    let is_control_plane_ready = is_leader || current_leader.is_some();

    (
        StatusCode::OK,
        Json(serde_json::json!(ShardsOverviewResponse {
            total_shards,
            assigned_count: placements.len(),
            is_bootstrapped,
            is_control_plane_ready,
            is_leader,
            current_leader,
            placements,
        })),
    )
}

#[derive(Deserialize)]
struct BootstrapRequest {
    nodes: Option<Vec<Uuid>>,
}

async fn bootstrap_round_robin(
    State(state): State<AppState>,
    Json(req): Json<BootstrapRequest>,
) -> impl IntoResponse {
    let (shard_handle, cp_handle) = match (&state.shard, &state.control_plane) {
        (Some(s), Some(cp)) => (s, cp),
        _ => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Shard or Control Plane service not configured" })),
            )
        }
    };

    let total_shards = shard_handle.total_shards().await;
    let coord = ShardCoordinator::new(
        shard_service::ShardConfig {
            total_shards,
            replication_factor: 3,
            is_coordinator: true,
        },
        cp_handle.clone(),
        shard_handle.clone(),
    );

    // Se a lista de nós não for fornecida explicitamente, utiliza todos os nós Alive do SWIM
    let target_nodes = if let Some(nodes) = req.nodes {
        nodes
    } else if let Some(membership) = &state.membership {
        let members = membership.all_members().await;
        members
            .into_iter()
            .filter(|m| m.status == membership_service::MemberStatus::Alive)
            .map(|m| m.node_id.id())
            .collect()
    } else {
        vec![state.ctx.identity.id()]
    };

    match coord.bootstrap_round_robin(&target_nodes, Some(&state.ctx)).await {
        Ok(count) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "assigned_count": count,
                "total_shards": total_shards,
                "nodes": target_nodes,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("{e}") })),
        ),
    }
}

#[derive(Deserialize)]
struct AssignManualRequest {
    shard_id: u32,
    primary: Uuid,
    replicas: Vec<Uuid>,
}

async fn assign_manual(
    State(state): State<AppState>,
    Json(req): Json<AssignManualRequest>,
) -> impl IntoResponse {
    let (shard_handle, cp_handle) = match (&state.shard, &state.control_plane) {
        (Some(s), Some(cp)) => (s, cp),
        _ => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Shard or Control Plane service not configured" })),
            )
        }
    };

    let total_shards = shard_handle.total_shards().await;
    let coord = ShardCoordinator::new(
        shard_service::ShardConfig {
            total_shards,
            replication_factor: 3,
            is_coordinator: true,
        },
        cp_handle.clone(),
        shard_handle.clone(),
    );

    match coord.assign_manual(req.shard_id, req.primary, req.replicas, Some(&state.ctx)).await {
        Ok(placement) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "placement": placement,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("{e}") })),
        ),
    }
}
