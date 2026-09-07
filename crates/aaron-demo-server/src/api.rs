use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use crate::cluster::{ClusterError, DemoClusterManager};

const CLIENT_COOKIE_NAME: &str = "aaron_demo_client_id";

pub fn extract_or_generate_client_id(headers: &HeaderMap) -> (String, bool) {
    if let Some(cookie_hdr) = headers.get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_hdr.to_str() {
            for piece in cookie_str.split(';') {
                let mut parts = piece.trim().splitn(2, '=');
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    if k == CLIENT_COOKIE_NAME && !v.is_empty() {
                        return (v.to_string(), false);
                    }
                }
            }
        }
    }
    (format!("client-{}", uuid::Uuid::new_v4()), true)
}

pub fn make_cookie_header(client_id: &str) -> HeaderValue {
    let s = format!("{CLIENT_COOKIE_NAME}={client_id}; Path=/; Max-Age=2592000; SameSite=Lax; HttpOnly");
    HeaderValue::from_str(&s).unwrap()
}

#[derive(Serialize)]
pub struct StatsResponse {
    pub active_clusters: usize,
    pub max_clusters: usize,
    pub available_slots: usize,
    pub version: &'static str,
}

pub async fn get_demo_stats(
    State(manager): State<Arc<DemoClusterManager>>,
) -> Json<StatsResponse> {
    let active = manager.active_count().await;
    let max = manager.max_clusters();
    let available = max.saturating_sub(active);

    Json(StatsResponse {
        active_clusters: active,
        max_clusters: max,
        available_slots: available,
        version: "0.1.0",
    })
}

pub async fn get_current_client_session(
    State(manager): State<Arc<DemoClusterManager>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (client_id, is_new) = extract_or_generate_client_id(&headers);
    let mut resp_headers = HeaderMap::new();
    if is_new {
        resp_headers.insert(header::SET_COOKIE, make_cookie_header(&client_id));
    }

    if let Some(cluster) = manager.get_cluster_for_client(&client_id).await {
        let summary = cluster.status_summary().await;
        return (
            StatusCode::OK,
            resp_headers,
            Json(serde_json::json!({
                "client_id": client_id,
                "has_active_cluster": true,
                "in_cooldown": true,
                "cluster": summary,
                "dashboard_url": format!("/demo/{}/", cluster.session_id),
            })),
        );
    }

    // Check if client is in cooldown after stopping earlier
    if let Some(rec) = manager.get_client_record(&client_id).await {
        let now = Instant::now();
        if now < rec.expires_at {
            let remaining = rec.expires_at.saturating_duration_since(now).as_secs();
            return (
                StatusCode::OK,
                resp_headers,
                Json(serde_json::json!({
                    "client_id": client_id,
                    "has_active_cluster": false,
                    "in_cooldown": true,
                    "cooldown_remaining_secs": remaining,
                })),
            );
        }
    }

    (
        StatusCode::OK,
        resp_headers,
        Json(serde_json::json!({
            "client_id": client_id,
            "has_active_cluster": false,
            "in_cooldown": false,
            "cooldown_remaining_secs": 0,
        })),
    )
}

pub async fn start_demo_cluster(
    State(manager): State<Arc<DemoClusterManager>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (client_id, is_new) = extract_or_generate_client_id(&headers);
    let mut resp_headers = HeaderMap::new();
    if is_new {
        resp_headers.insert(header::SET_COOKIE, make_cookie_header(&client_id));
    }

    match manager.create_cluster_for_client(&client_id).await {
        Ok(cluster) => {
            let summary = cluster.status_summary().await;
            (
                StatusCode::OK,
                resp_headers,
                Json(serde_json::json!({
                    "success": true,
                    "cluster": summary,
                    "dashboard_url": format!("/demo/{}/", cluster.session_id),
                })),
            )
        }
        Err(ClusterError::AlreadyActive(cluster)) => {
            let summary = cluster.status_summary().await;
            let remaining = cluster.expires_at.saturating_duration_since(Instant::now()).as_secs();
            (
                StatusCode::OK,
                resp_headers,
                Json(serde_json::json!({
                    "success": true,
                    "already_active": true,
                    "cluster": summary,
                    "dashboard_url": format!("/demo/{}/", cluster.session_id),
                    "message": format!(
                        "You already have an active cluster. Cooldown expires in {}m {}s.",
                        remaining / 60,
                        remaining % 60
                    ),
                })),
            )
        }
        Err(ClusterError::Cooldown { remaining_secs }) => {
            (
                StatusCode::TOO_MANY_REQUESTS,
                resp_headers,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!(
                        "Cluster session cooldown active: you can request another cluster after your 15-minute usage window expires (in {}m {}s).",
                        remaining_secs / 60,
                        remaining_secs % 60
                    ),
                    "cooldown_remaining_secs": remaining_secs,
                })),
            )
        }
        Err(ClusterError::SlotsFull(max)) => {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                resp_headers,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!(
                        "All {} demo cluster slots are currently occupied. Please wait for an existing session to finish.",
                        max
                    ),
                })),
            )
        }
        Err(ClusterError::Other(err)) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                resp_headers,
                Json(serde_json::json!({
                    "success": false,
                    "error": err,
                })),
            )
        }
    }
}

pub async fn get_cluster_status(
    State(manager): State<Arc<DemoClusterManager>>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match manager.get_cluster(&session_id).await {
        Some(cluster) => Ok(Json(cluster.status_summary().await)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Cluster session not found or has expired",
            })),
        )),
    }
}

pub async fn kill_cluster_node(
    State(manager): State<Arc<DemoClusterManager>>,
    Path((session_id, node_idx)): Path<(String, usize)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match manager.kill_node(&session_id, node_idx).await {
        Ok(msg) => Ok(Json(serde_json::json!({
            "success": true,
            "message": msg,
        }))),
        Err(err) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )),
    }
}

pub async fn revive_cluster_node(
    State(manager): State<Arc<DemoClusterManager>>,
    Path((session_id, node_idx)): Path<(String, usize)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match manager.revive_node(&session_id, node_idx).await {
        Ok(msg) => Ok(Json(serde_json::json!({
            "success": true,
            "message": msg,
        }))),
        Err(err) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )),
    }
}

#[derive(Deserialize)]
pub struct BenchmarkPayload {
    pub operations: Option<usize>,
}

pub async fn run_cluster_benchmark(
    State(manager): State<Arc<DemoClusterManager>>,
    Path(session_id): Path<String>,
    payload: Option<Json<BenchmarkPayload>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let ops = payload.map(|p| p.operations.unwrap_or(1000)).unwrap_or(1000);
    match manager.run_benchmark(&session_id, ops).await {
        Ok(result) => Ok(Json(result)),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )),
    }
}

pub async fn stop_demo_cluster(
    State(manager): State<Arc<DemoClusterManager>>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match manager.terminate_cluster(&session_id).await {
        Ok(()) => Ok(Json(serde_json::json!({
            "success": true,
            "message": "Demo cluster stopped and resources released",
        }))),
        Err(err) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )),
    }
}
