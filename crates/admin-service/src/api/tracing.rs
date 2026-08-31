use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use tracing_service::ChangeLogLevel;

use crate::state::AppState;

#[derive(Serialize)]
pub struct TracingInfoResponse {
    pub filter: String,
}

pub async fn get_tracing_info(State(state): State<AppState>) -> Json<TracingInfoResponse> {
    let filter = state.ctx.env.get_raw("LOG_LEVEL").unwrap_or_else(|| "info".to_string());
    Json(TracingInfoResponse { filter })
}

#[derive(Deserialize)]
pub struct UpdateLogLevelRequest {
    pub filter: String,
}

#[derive(Serialize)]
pub struct UpdateLogLevelResponse {
    pub success: bool,
    pub filter: String,
}

pub async fn update_log_level(
    State(state): State<AppState>,
    Json(payload): Json<UpdateLogLevelRequest>,
) -> Result<Json<UpdateLogLevelResponse>, (StatusCode, Json<serde_json::Value>)> {
    let filter = payload.filter.trim();
    if filter.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Filter directive cannot be empty" })),
        ));
    }

    // Publish ChangeLogLevel event onto the Node's EventHub
    let event = ChangeLogLevel::new(filter);
    state.ctx.event_hub.publish(event).await;

    // Update in-memory env so subsequent reads reflect the new directive
    let _ = state.ctx.env.set("LOG_LEVEL", filter);

    Ok(Json(UpdateLogLevelResponse {
        success: true,
        filter: filter.to_string(),
    }))
}
