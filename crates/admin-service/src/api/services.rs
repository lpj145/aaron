use axum::{extract::State, response::Json};
use serde::Serialize;

use crate::state::{AppState, ServiceMetadata};

#[derive(Serialize)]
pub struct ServicesListResponse {
    pub services: Vec<ServiceMetadata>,
}

pub async fn get_services(State(state): State<AppState>) -> Json<ServicesListResponse> {
    let mut enriched = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Read all services and declared schemas directly from Node Context
    let node_services = state.ctx.services().await;
    for svc in node_services {
        seen.insert(svc.name.clone());
        let mut fields = Vec::new();
        for f in svc.schema {
            let current = state.ctx.env.get_raw(&f.name);
            fields.push(crate::state::ConfigFieldMetadata {
                name: f.name,
                type_name: f.type_name,
                required: f.required,
                default: f.default,
                description: f.description,
                current_value: current,
            });
        }
        enriched.push(ServiceMetadata {
            name: svc.name,
            schema: fields,
        });
    }

    // 2. Fallback to any explicitly registered schemas on AdminService builder if not already present
    for svc in state.services.iter() {
        if seen.insert(svc.name.clone()) {
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
    }

    enriched.sort_by(|a, b| a.name.cmp(&b.name));

    Json(ServicesListResponse { services: enriched })
}
