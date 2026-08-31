use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_http::cors::CorsLayer;

use crate::state::AppState;

pub mod cluster;
pub mod config;
pub mod env;
pub mod node;
pub mod services;
pub mod store;
pub mod tracing;

pub fn create_api_router() -> Router<AppState> {
    Router::new()
        // Node & Health
        .route("/health", get(node::get_health))
        .route("/node", get(node::get_node_info))
        // Cluster & Membership
        .route("/cluster", get(cluster::get_cluster_info))
        .route("/cluster/join", post(cluster::join_cluster))
        .route("/cluster/leave", post(cluster::leave_cluster))
        // Services
        .route("/services", get(services::get_services))
        // LSM Store Explorer
        .route("/store", get(store::get_store_info))
        .route("/store/keyspaces", post(store::create_keyspace))
        .route("/store/{keyspace}/scan", get(store::scan_keyspace))
        .route("/store/{keyspace}/get", get(store::get_key))
        .route("/store/{keyspace}/set", post(store::set_key))
        .route("/store/{keyspace}/delete", delete(store::delete_key))
        // Dynamic Tracing & Config
        .route("/tracing", get(tracing::get_tracing_info))
        .route("/tracing/level", post(tracing::update_log_level))
        .route("/config/tracing", post(config::update_tracing_config))
        .route("/config/swim", get(config::get_swim_config))
        .route("/config/swim", post(config::update_swim_config))
        // Environment
        .route("/env", get(env::get_env_vars))
        .route("/env", post(env::set_env_var))
        .layer(CorsLayer::permissive())
}
