use axum::{extract::State, response::Json};
use serde::Serialize;

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

fn is_secret_var(name: &str) -> bool {
    let upper = name.to_uppercase();
    upper.contains("SECRET")
        || upper.contains("TOKEN")
        || upper.contains("KEY")
        || upper.contains("PASSWORD")
        || upper.contains("PASS")
        || upper.contains("AUTH")
}
