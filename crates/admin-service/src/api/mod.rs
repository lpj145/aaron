use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_http::cors::CorsLayer;

use crate::state::AppState;

pub mod cluster;
pub mod config;
pub mod control_plane;
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
        .route("/node/shutdown", post(node::shutdown_node))
        // Cluster & Membership
        .route("/cluster", get(cluster::get_cluster_info))
        .route("/cluster/join", post(cluster::join_cluster))
        .route("/cluster/leave", post(cluster::leave_cluster))
        .route("/cluster/nodes/start", post(cluster::start_node))
        .route("/cluster/nodes/remove", post(cluster::remove_node))
        // Control Plane (Raft Consensus)
        .route("/control-plane/status", get(control_plane::get_control_plane_status))
        .route("/control-plane/init", post(control_plane::init_control_plane_cluster))
        .route("/control-plane/membership", post(control_plane::change_control_plane_membership))
        .route("/control-plane/learner", post(control_plane::add_control_plane_learner))
        .route("/control-plane/remove-node", post(control_plane::remove_control_plane_node))
        .route("/control-plane/write", post(control_plane::write_control_plane_state))
        .route("/control-plane/delete", post(control_plane::delete_control_plane_state))
        .route("/control-plane/state", post(control_plane::write_control_plane_state))
        .route("/control-plane/state", delete(control_plane::delete_control_plane_state))
        // Services
        .route("/services", get(services::get_services))
        // LSM Store Explorer
        .route("/store", get(store::get_store_info))
        .route("/store/benchmark", post(store::run_store_benchmark))
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
