use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::cluster::DemoClusterManager;

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

pub async fn start_demo_cluster(
    State(manager): State<Arc<DemoClusterManager>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match manager.create_cluster().await {
        Ok(cluster) => {
            let summary = cluster.status_summary().await;
            Ok(Json(serde_json::json!({
                "success": true,
                "cluster": summary,
                "dashboard_url": format!("/demo/{}/", cluster.session_id),
            })))
        }
        Err(err) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )),
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
