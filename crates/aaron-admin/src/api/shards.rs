use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use aaron_core::Uuid;
use serde::{Deserialize, Serialize};
use aaron_shard::{ShardCoordinator, ShardPlacement};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_shards_overview))
        .route("/bootstrap", post(bootstrap_round_robin))
        .route("/rebalance", post(rebalance_shards))
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
        let shard_data = cp.prefix_data("shards/").await;
        if shard_data.contains_key("shards/system/bootstrapped") {
            is_bootstrapped = true;
        }
        for (k, v) in shard_data {
            if let Some(shard_id_str) = k.strip_prefix("shards/") {
                if let Ok(shard_id) = shard_id_str.parse::<u32>()
                    && let Ok(placement) = ShardPlacement::from_bytes(&v) {
                        if !placements.iter().any(|p| p.shard_id == shard_id && p.service_name == placement.service_name) {
                            placements.push(placement);
                        }
                } else {
                    let parts: Vec<&str> = shard_id_str.split('/').collect();
                    if parts.len() == 2 && parts[1].parse::<u32>().is_ok()
                        && let Ok(mut placement) = ShardPlacement::from_bytes(&v) {
                            if placement.service_name == "default" || placement.service_name.is_empty() {
                                placement.service_name = parts[0].to_string();
                            }
                            if !placements.iter().any(|p| p.shard_id == placement.shard_id && p.service_name == placement.service_name) {
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

#[derive(Serialize, Deserialize)]
struct BootstrapRequest {
    service: Option<String>,
    nodes: Option<Vec<Uuid>>,
    total_shards: Option<u32>,
}

async fn bootstrap_round_robin(
    State(state): State<AppState>,
    Json(req): Json<BootstrapRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let (shard_handle, cp_handle) = match (&state.shard, &state.control_plane) {
        (Some(s), Some(cp)) => (s, cp),
        _ => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Shard or Control Plane service not configured" })),
            ))
        }
    };

    // Se o nó local não for o líder Raft atual, redireciona transparentemente a requisição para o líder
    if !cp_handle.is_leader()
        && let Some(leader_id) = cp_handle.current_leader() {
            return crate::api::control_plane::proxy_request_to_leader(
                &state,
                leader_id,
                "/api/shards/bootstrap",
                reqwest::Method::POST,
                Some(&req),
            ).await;
        }

    let default_total = shard_handle.total_shards().await;
    let total_shards = req.total_shards.unwrap_or(if default_total > 0 { default_total } else { 1024 });
    shard_handle.set_total_shards(total_shards).await;
    let service_name = req.service.clone().unwrap_or_else(|| "default".to_string());
    let coord = ShardCoordinator::with_service(
        service_name.clone(),
        aaron_shard::ShardConfig {
            total_shards,
            replication_factor: 3,
            is_coordinator: true,
        },
        cp_handle.clone(),
        shard_handle.clone(),
    );

    // Se a lista de nós não for fornecida explicitamente, utiliza nós do serviço excluindo control-plane
    let target_nodes = if let Some(nodes) = req.nodes {
        nodes
    } else if let Some(membership) = &state.membership {
        let members = membership.all_members().await;
        let service_nodes = coord.filter_service_nodes(&service_name, &members);
        if !service_nodes.is_empty() {
            service_nodes
        } else {
            // Fallback para nós vivos excluindo explicitamente control-plane
            members
                .into_iter()
                .filter(|m| {
                    m.status == aaron_membership::MemberStatus::Alive
                        && !m.tags.iter().any(|t| t == "role:control-plane" || t.starts_with("role:control-plane"))
                })
                .map(|m| m.node_id.id())
                .collect()
        }
    } else {
        vec![state.ctx.identity.id()]
    };

    match coord.bootstrap_service_round_robin(&service_name, &target_nodes, Some(&state.ctx)).await {
        Ok(count) => Ok(Json(serde_json::json!({
            "success": true,
            "service": service_name,
            "assigned_count": count,
            "total_shards": total_shards,
            "nodes": target_nodes,
        }))),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("{e}") })),
        )),
    }
}

#[derive(Serialize, Deserialize)]
struct AssignManualRequest {
    service: Option<String>,
    shard_id: u32,
    primary: Uuid,
    replicas: Vec<Uuid>,
}

async fn assign_manual(
    State(state): State<AppState>,
    Json(req): Json<AssignManualRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let (shard_handle, cp_handle) = match (&state.shard, &state.control_plane) {
        (Some(s), Some(cp)) => (s, cp),
        _ => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Shard or Control Plane service not configured" })),
            ))
        }
    };

    // Se o nó local não for o líder Raft atual, redireciona transparentemente a requisição para o líder
    if !cp_handle.is_leader()
        && let Some(leader_id) = cp_handle.current_leader() {
            return crate::api::control_plane::proxy_request_to_leader(
                &state,
                leader_id,
                "/api/shards/assign",
                reqwest::Method::POST,
                Some(&req),
            ).await;
        }

    let total_shards = shard_handle.total_shards().await;
    let service_name = req.service.clone().unwrap_or_else(|| "default".to_string());
    let coord = ShardCoordinator::with_service(
        service_name.clone(),
        aaron_shard::ShardConfig {
            total_shards,
            replication_factor: 3,
            is_coordinator: true,
        },
        cp_handle.clone(),
        shard_handle.clone(),
    );

    match coord.assign_service_manual(&service_name, req.shard_id, req.primary, req.replicas, Some(&state.ctx)).await {
        Ok(placement) => Ok(Json(serde_json::json!({
            "success": true,
            "placement": placement,
        }))),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("{e}") })),
        )),
    }
}

#[derive(Serialize, Deserialize)]
struct RebalanceRequest {
    service: Option<String>,
}

async fn rebalance_shards(
    State(state): State<AppState>,
    Json(req): Json<RebalanceRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let (shard_handle, cp_handle) = match (&state.shard, &state.control_plane) {
        (Some(s), Some(cp)) => (s, cp),
        _ => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Shard or Control Plane service not configured" })),
            ))
        }
    };

    // Se o nó local não for o líder Raft atual, redireciona transparentemente a requisição para o líder
    if !cp_handle.is_leader()
        && let Some(leader_id) = cp_handle.current_leader() {
            return crate::api::control_plane::proxy_request_to_leader(
                &state,
                leader_id,
                "/api/shards/rebalance",
                reqwest::Method::POST,
                Some(&req),
            ).await;
        }

    let service_name = req.service.clone().unwrap_or_else(|| "default".to_string());
    let default_total = shard_handle.total_shards().await;
    let total_shards = if default_total > 0 { default_total } else { 1024 };

    let coord = ShardCoordinator::with_service(
        service_name.clone(),
        aaron_shard::ShardConfig {
            total_shards,
            replication_factor: 3,
            is_coordinator: true,
        },
        cp_handle.clone(),
        shard_handle.clone(),
    );

    // Coleta todos os nós vivos disponíveis para o serviço, incluindo os nós recém-chegados
    let target_nodes = if let Some(membership) = &state.membership {
        let members = membership.all_members().await;
        let service_nodes = coord.filter_service_nodes(&service_name, &members);
        if !service_nodes.is_empty() {
            service_nodes
        } else {
            members
                .into_iter()
                .filter(|m| {
                    m.status == aaron_membership::MemberStatus::Alive
                        && !m.tags.iter().any(|t| t == "role:control-plane" || t.starts_with("role:control-plane"))
                })
                .map(|m| m.node_id.id())
                .collect()
        }
    } else {
        vec![state.ctx.identity.id()]
    };

    if target_nodes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No active worker nodes found for rebalancing" })),
        ));
    }

    match coord.bootstrap_service_round_robin(&service_name, &target_nodes, Some(&state.ctx)).await {
        Ok(count) => Ok(Json(serde_json::json!({
            "success": true,
            "message": format!("Cluster rebalanced: {count} shards distributed across {} active nodes", target_nodes.len()),
            "service": service_name,
            "assigned_count": count,
            "node_count": target_nodes.len(),
            "nodes": target_nodes,
        }))),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("{e}") })),
        )),
    }
}
