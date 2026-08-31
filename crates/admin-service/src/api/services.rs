use axum::{extract::State, response::Json};
use serde::Serialize;

use crate::state::{AppState, ServiceMetadata};

#[derive(Serialize)]
pub struct ServicesListResponse {
    pub services: Vec<ServiceMetadata>,
}

pub async fn get_services(State(state): State<AppState>) -> Json<ServicesListResponse> {
    let mut enriched = Vec::new();

    for svc in state.services.iter() {
        let mut fields = Vec::new();
        for f in &svc.schema {
            let current = state.ctx.env.get_raw(&f.name);
            fields.push(crate::state::ConfigFieldMetadata {
                name: f.name.clone(),
                type_name: f.type_name.clone(),
                required: f.required,
                default: f.default.clone(),
                description: f.description.clone(),
                current_value: current,
            });
        }
        enriched.push(ServiceMetadata {
            name: svc.name.clone(),
            schema: fields,
        });
    }

    Json(ServicesListResponse { services: enriched })
}
