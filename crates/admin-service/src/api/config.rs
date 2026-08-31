use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use membership_service::UpdateSwimConfig;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;
use tracing_service::ChangeLogLevel;

use crate::state::AppState;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SwimConfigResponse {
    pub probe_interval_ms: u64,
    pub probe_timeout_ms: u64,
    pub suspect_timeout_ms: u64,
    pub indirect_ping_targets: usize,
    pub gossip_fanout: usize,
}

#[derive(Deserialize)]
pub struct UpdateSwimConfigRequest {
    pub probe_interval_ms: Option<u64>,
    pub probe_timeout_ms: Option<u64>,
    pub suspect_timeout_ms: Option<u64>,
    pub indirect_ping_targets: Option<usize>,
    pub gossip_fanout: Option<usize>,
    #[serde(default)]
    pub propagate_cluster: bool,
}

#[derive(Serialize)]
pub struct ConfigUpdateResponse {
    pub success: bool,
    pub message: String,
    pub local_applied: bool,
    pub propagated_nodes: usize,
    pub failed_nodes: usize,
}

pub async fn get_swim_config(State(state): State<AppState>) -> Json<SwimConfigResponse> {
    if let Some(ref handle) = state.membership
        && let Some(cfg) = handle.config().await
    {
        return Json(SwimConfigResponse {
            probe_interval_ms: cfg.probe_interval.as_millis() as u64,
            probe_timeout_ms: cfg.probe_timeout.as_millis() as u64,
            suspect_timeout_ms: cfg.suspect_timeout.as_millis() as u64,
            indirect_ping_targets: cfg.indirect_ping_targets,
            gossip_fanout: cfg.gossip_fanout,
        });
    }

    // Default fallback if membership handle not loaded
    Json(SwimConfigResponse {
        probe_interval_ms: 1000,
        probe_timeout_ms: 200,
        suspect_timeout_ms: 1000,
        indirect_ping_targets: 3,
        gossip_fanout: 3,
    })
}

pub async fn update_swim_config(
    State(state): State<AppState>,
    Json(payload): Json<UpdateSwimConfigRequest>,
) -> Result<Json<ConfigUpdateResponse>, (StatusCode, Json<serde_json::Value>)> {
    let update = UpdateSwimConfig {
        probe_interval: payload.probe_interval_ms.map(Duration::from_millis),
        probe_timeout: payload.probe_timeout_ms.map(Duration::from_millis),
        suspect_timeout: payload.suspect_timeout_ms.map(Duration::from_millis),
        indirect_ping_targets: payload.indirect_ping_targets,
        gossip_fanout: payload.gossip_fanout,
    };

    // 1. Apply to local node via EventHub
    state.ctx.event_hub.publish(update.clone()).await;

    let mut propagated_nodes = 0;
    let mut failed_nodes = 0;

    // 2. Propagate to cluster if requested
    if payload.propagate_cluster && let Some(ref handle) = state.membership {
        let local_id = state.ctx.identity.id();
        let active_peers = handle.active_members().await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(800))
            .build()
            .unwrap_or_default();

        for peer in active_peers {
            if peer.node_id.id() == local_id {
                continue;
            }

            let peer_ip = peer.addr.ip();
            // Default admin port assumption for cluster peer admin endpoints
            let peer_url = format!("http://{}:8080/api/config/swim", peer_ip);
            let body = serde_json::json!({
                "probe_interval_ms": payload.probe_interval_ms,
                "probe_timeout_ms": payload.probe_timeout_ms,
                "suspect_timeout_ms": payload.suspect_timeout_ms,
                "indirect_ping_targets": payload.indirect_ping_targets,
                "gossip_fanout": payload.gossip_fanout,
                "propagate_cluster": false
            });

            match client.post(&peer_url).json(&body).send().await {
                Ok(res) if res.status().is_success() => {
                    propagated_nodes += 1;
                }
                Ok(res) => {
                    debug!(target: "admin", status = %res.status(), peer = %peer_ip, "Cluster SWIM config propagation returned non-200");
                    failed_nodes += 1;
                }
                Err(err) => {
                    debug!(target: "admin", error = %err, peer = %peer_ip, "Could not reach peer for cluster SWIM config propagation");
                    failed_nodes += 1;
                }
            }
        }
    }

    Ok(Json(ConfigUpdateResponse {
        success: true,
        message: if payload.propagate_cluster {
            format!("SWIM config applied locally and broadcasted to {propagated_nodes} peer(s)")
        } else {
            "SWIM config applied locally".to_string()
        },
        local_applied: true,
        propagated_nodes,
        failed_nodes,
    }))
}

#[derive(Deserialize)]
pub struct UpdateTracingConfigRequest {
    pub filter: String,
    #[serde(default)]
    pub propagate_cluster: bool,
}

pub async fn update_tracing_config(
    State(state): State<AppState>,
    Json(payload): Json<UpdateTracingConfigRequest>,
) -> Result<Json<ConfigUpdateResponse>, (StatusCode, Json<serde_json::Value>)> {
    let filter = payload.filter.trim();
    if filter.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Filter directive cannot be empty" })),
        ));
    }

    // 1. Publish ChangeLogLevel event onto the Node's local EventHub
    let event = ChangeLogLevel::new(filter);
    state.ctx.event_hub.publish(event).await;

    // Update in-memory env so subsequent reads reflect the new directive
    let _ = state.ctx.env.set("LOG_LEVEL", filter);

    let mut propagated_nodes = 0;
    let mut failed_nodes = 0;

    // 2. Propagate to cluster if requested
    if payload.propagate_cluster && let Some(ref handle) = state.membership {
        let local_id = state.ctx.identity.id();
        let active_peers = handle.active_members().await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(800))
            .build()
            .unwrap_or_default();

        for peer in active_peers {
            if peer.node_id.id() == local_id {
                continue;
            }

            let peer_ip = peer.addr.ip();
            let peer_url = format!("http://{}:8080/api/config/tracing", peer_ip);
            let body = serde_json::json!({
                "filter": filter,
                "propagate_cluster": false
            });

            match client.post(&peer_url).json(&body).send().await {
                Ok(res) if res.status().is_success() => {
                    propagated_nodes += 1;
                }
                Ok(res) => {
                    debug!(target: "admin", status = %res.status(), peer = %peer_ip, "Cluster tracing config propagation returned non-200");
                    failed_nodes += 1;
                }
                Err(err) => {
                    debug!(target: "admin", error = %err, peer = %peer_ip, "Could not reach peer for cluster tracing config propagation");
                    failed_nodes += 1;
                }
            }
        }
    }

    Ok(Json(ConfigUpdateResponse {
        success: true,
        message: if payload.propagate_cluster {
            format!("Tracing log level applied locally and broadcasted to {propagated_nodes} peer(s)")
        } else {
            "Tracing log level applied locally".to_string()
        },
        local_applied: true,
        propagated_nodes,
        failed_nodes,
    }))
}
