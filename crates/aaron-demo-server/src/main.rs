use axum::{
    response::Html,
    routing::{get, post},
    Router,
};
use clap::Parser;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

mod api;
mod cluster;
mod proxy;

use cluster::DemoClusterManager;

#[derive(Parser, Debug)]
#[command(name = "aaron-demo-server", about = "Aaron Landing Page & Interactive Live Demo Server")]
struct Args {
    #[arg(long, env = "HOST", default_value = "0.0.0.0")]
    host: String,

    #[arg(short, long, env = "PORT", default_value_t = 8080)]
    port: u16,

    #[arg(long, env = "MAX_CLUSTERS", default_value_t = 6)]
    max_clusters: usize,

    #[arg(long, env = "TTL_MINUTES", default_value_t = 15)]
    ttl_minutes: u64,
}

static LANDING_HTML: &str = include_str!("../static/index.html");

async fn serve_landing() -> Html<&'static str> {
    Html(LANDING_HTML)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,aaron_demo_server=debug".into()),
        )
        .init();

    let args = Args::parse();
    info!(
        host = %args.host,
        port = %args.port,
        max_clusters = %args.max_clusters,
        ttl_minutes = %args.ttl_minutes,
        "Initializing Aaron Demo Server"
    );

    let manager = Arc::new(DemoClusterManager::new(
        args.max_clusters,
        Duration::from_secs(args.ttl_minutes * 60),
    ));

    // Spawn background reaper for expired demo clusters
    let reaper_mgr = manager.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            reaper_mgr.reap_expired().await;
        }
    });

    let demo_api = Router::new()
        .route("/stats", get(api::get_demo_stats))
        .route("/start", post(api::start_demo_cluster))
        .route("/{session_id}/status", get(api::get_cluster_status))
        .route("/{session_id}/kill/{node_idx}", post(api::kill_cluster_node))
        .route("/{session_id}/revive/{node_idx}", post(api::revive_cluster_node))
        .route("/{session_id}/benchmark", post(api::run_cluster_benchmark))
        .route("/{session_id}/stop", post(api::stop_demo_cluster))
        .with_state(manager.clone());

    let app = Router::new()
        .route("/", get(serve_landing))
        .nest("/api/demo", demo_api)
        // Proxy routes for Aaron embedded Admin UI and cluster REST endpoints
        .route("/demo/{*path}", get(proxy::handle_proxy_admin).post(proxy::handle_proxy_admin))
        .route("/assets/{*path}", get(proxy::handle_proxy_admin))
        .route("/favicon.svg", get(proxy::handle_proxy_admin))
        .fallback(proxy::handle_proxy_admin)
        .with_state(manager)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    info!("Aaron Demo Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
