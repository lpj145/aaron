use axum::{extract::State, response::Json};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
pub struct NodeInfoResponse {
    pub id: String,
    pub incarnation: u64,
    pub hostname: String,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub dir_path: String,
    pub uptime_secs: u64,
    pub cluster_id: Option<String>,
    pub keyspaces_count: usize,
    pub services_count: usize,
}

pub async fn get_node_info(State(state): State<AppState>) -> Json<NodeInfoResponse> {
    let id_str = format!("{}", state.ctx.identity.id());
    let uptime_secs = state.start_time.elapsed().as_secs();

    let cluster_id = if let Some(ref handle) = state.membership {
        handle.cluster_id().await.map(|c| c.to_string())
    } else {
        state.ctx.identity.cluster_id.map(|c| c.to_string())
    };

    let keyspaces_count = state.ctx.store.list_keyspaces().len();
    let services_count = state.services.len();

    Json(NodeInfoResponse {
        id: id_str,
        incarnation: state.ctx.identity.incarnation,
        hostname: state.ctx.env.hostname.clone(),
        ipv4: state.ctx.env.ipv4.clone(),
        ipv6: state.ctx.env.ipv6.clone(),
        dir_path: state.ctx.store.path().display().to_string(),
        uptime_secs,
        cluster_id,
        keyspaces_count,
        services_count,
    })
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub node_id: String,
    pub uptime_secs: u64,
}

pub async fn get_health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        node_id: state.ctx.identity.id().to_string(),
        uptime_secs: state.start_time.elapsed().as_secs(),
    })
}
