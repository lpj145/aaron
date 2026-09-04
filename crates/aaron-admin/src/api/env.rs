use axum::{extract::State, http::StatusCode, response::Json};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Serialize)]
pub struct EnvVarItem {
    pub name: String,
    pub value: String,
    pub is_secret: bool,
    pub tracked: bool,
    pub type_name: Option<String>,
}

#[derive(Serialize)]
pub struct EnvListResponse {
    pub envs: Vec<EnvVarItem>,
}

pub async fn get_env_vars(State(state): State<AppState>) -> Json<EnvListResponse> {
    let all = state.ctx.env.all_vars();
    let tracked = state.ctx.env.tracked();

    let mut list = Vec::new();

    for (k, v) in all {
        let is_secret = is_secret_var(&k);
        let tracked_var = tracked.iter().find(|t| t.name == k);

        list.push(EnvVarItem {
            name: k,
            value: v,
            is_secret,
            tracked: tracked_var.is_some(),
            type_name: tracked_var.map(|t| t.type_name.to_string()),
        });
    }

    list.sort_by(|a, b| a.name.cmp(&b.name));

    Json(EnvListResponse { envs: list })
}

#[derive(Deserialize)]
pub struct SetEnvVarRequest {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub propagate_cluster: bool,
}

#[derive(Serialize)]
pub struct SetEnvVarResponse {
    pub success: bool,
    pub message: String,
    pub local_applied: bool,
    pub propagated_nodes: usize,
    pub failed_nodes: usize,
}

pub async fn set_env_var(
    State(state): State<AppState>,
    Json(payload): Json<SetEnvVarRequest>,
) -> Result<Json<SetEnvVarResponse>, (StatusCode, Json<serde_json::Value>)> {
    let key = payload.key.trim();
    if key.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Variable name cannot be empty" })),
        ));
    }

    // 1. Set locally in node Env
    let _ = state.ctx.env.set(key, &payload.value);

    // Also publish SetEnvVar on local EventHub
    state
        .ctx
        .event_hub
        .publish(aaron_core::SetEnvVar {
            key: key.to_string(),
            value: payload.value.clone(),
        })
        .await;

    let mut propagated_nodes = 0;
    let mut failed_nodes = 0;

    // 2. Propagate to cluster over QUIC if requested
    if payload.propagate_cluster && let Some(ref handle) = state.membership {
        let (p, f) = handle
            .broadcast_config_update(None, None, Some((key.to_string(), payload.value.clone())))
            .await;
        propagated_nodes = p;
        failed_nodes = f;
    }

    Ok(Json(SetEnvVarResponse {
        success: true,
        message: if payload.propagate_cluster {
            format!("Environment variable '{key}' set locally and propagated to {propagated_nodes} peer(s)")
        } else {
            format!("Environment variable '{key}' set locally")
        },
        local_applied: true,
        propagated_nodes,
        failed_nodes,
    }))
}

fn is_secret_var(name: &str) -> bool {
    let upper = name.to_uppercase();
    upper.contains("SECRET")
        || upper.contains("TOKEN")
        || upper.contains("KEY")
        || upper.contains("PASSWORD")
        || upper.contains("PASS")
        || upper.contains("AUTH")
}
