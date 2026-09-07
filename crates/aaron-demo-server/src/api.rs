use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::cluster::{ClusterError, DemoClusterManager};

const CLIENT_COOKIE_NAME: &str = "aaron_demo_client_id";

pub fn extract_or_generate_client_id(headers: &HeaderMap) -> (String, bool) {
    if let Some(cookie_hdr) = headers.get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_hdr.to_str() {
            for piece in cookie_str.split(';') {
                let mut parts = piece.trim().splitn(2, '=');
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    if k == CLIENT_COOKIE_NAME && !v.is_empty() {
                        return (v.to_string(), false);
                    }
                }
            }
        }
    }
    (format!("client-{}", uuid::Uuid::new_v4()), true)
}

pub fn make_cookie_header(client_id: &str) -> HeaderValue {
    let s = format!("{CLIENT_COOKIE_NAME}={client_id}; Path=/; Max-Age=2592000; SameSite=Lax; HttpOnly");
    HeaderValue::from_str(&s).unwrap()
}

#[derive(Serialize)]
pub struct StatsResponse {
    pub active_clusters: usize,
    pub max_clusters: usize,
    pub available_slots: usize,
    pub version: &'static str,
}

pub async fn get_demo_stats(
    State(manager): State<Arc<DemoClusterManager>>,
) -> Json<StatsResponse> {
    let active = manager.active_count().await;
    let max = manager.max_clusters();
    let available = max.saturating_sub(active);

    Json(StatsResponse {
        active_clusters: active,
        max_clusters: max,
        available_slots: available,
        version: "0.1.0",
    })
}

pub async fn get_current_client_session(
    State(manager): State<Arc<DemoClusterManager>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (client_id, is_new) = extract_or_generate_client_id(&headers);
    let mut resp_headers = HeaderMap::new();
    if is_new {
        resp_headers.insert(header::SET_COOKIE, make_cookie_header(&client_id));
    }

    if let Some(cluster) = manager.get_cluster_for_client(&client_id).await {
        let summary = cluster.status_summary().await;
        return (
            StatusCode::OK,
            resp_headers,
            Json(serde_json::json!({
                "client_id": client_id,
                "has_active_cluster": true,
                "in_cooldown": true,
                "cluster": summary,
                "dashboard_url": format!("/demo/{}/", cluster.session_id),
            })),
        );
    }

    // Check if client is in cooldown after stopping earlier
    if let Some(rec) = manager.get_client_record(&client_id).await {
        let now = Instant::now();
        if now < rec.expires_at {
            let remaining = rec.expires_at.saturating_duration_since(now).as_secs();
            return (
                StatusCode::OK,
                resp_headers,
                Json(serde_json::json!({
                    "client_id": client_id,
                    "has_active_cluster": false,
                    "in_cooldown": true,
                    "cooldown_remaining_secs": remaining,
                })),
            );
        }
    }

    (
        StatusCode::OK,
        resp_headers,
        Json(serde_json::json!({
            "client_id": client_id,
            "has_active_cluster": false,
            "in_cooldown": false,
            "cooldown_remaining_secs": 0,
        })),
    )
}

pub async fn start_demo_cluster(
    State(manager): State<Arc<DemoClusterManager>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (client_id, is_new) = extract_or_generate_client_id(&headers);
    let mut resp_headers = HeaderMap::new();
    if is_new {
        resp_headers.insert(header::SET_COOKIE, make_cookie_header(&client_id));
    }

    match manager.create_cluster_for_client(&client_id).await {
        Ok(cluster) => {
            let summary = cluster.status_summary().await;
            (
                StatusCode::OK,
                resp_headers,
                Json(serde_json::json!({
                    "success": true,
                    "cluster": summary,
                    "dashboard_url": format!("/demo/{}/", cluster.session_id),
                })),
            )
        }
        Err(ClusterError::AlreadyActive(cluster)) => {
            let summary = cluster.status_summary().await;
            let remaining = cluster.expires_at.saturating_duration_since(Instant::now()).as_secs();
            (
                StatusCode::OK,
                resp_headers,
                Json(serde_json::json!({
                    "success": true,
                    "already_active": true,
                    "cluster": summary,
                    "dashboard_url": format!("/demo/{}/", cluster.session_id),
                    "message": format!(
                        "You already have an active cluster. Cooldown expires in {}m {}s.",
                        remaining / 60,
                        remaining % 60
                    ),
                })),
            )
        }
        Err(ClusterError::Cooldown { remaining_secs }) => {
            (
                StatusCode::TOO_MANY_REQUESTS,
                resp_headers,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!(
                        "Cluster session cooldown active: you can request another cluster after your 15-minute usage window expires (in {}m {}s).",
                        remaining_secs / 60,
                        remaining_secs % 60
                    ),
                    "cooldown_remaining_secs": remaining_secs,
                })),
            )
        }
        Err(ClusterError::SlotsFull(max)) => {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                resp_headers,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!(
                        "All {} demo cluster slots are currently occupied. Please wait for an existing session to finish.",
                        max
                    ),
                })),
            )
        }
        Err(ClusterError::Other(err)) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                resp_headers,
                Json(serde_json::json!({
                    "success": false,
                    "error": err,
                })),
            )
        }
    }
}

pub async fn get_cluster_status(
    State(manager): State<Arc<DemoClusterManager>>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match manager.get_cluster(&session_id).await {
        Some(cluster) => Ok(Json(cluster.status_summary().await)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Cluster session not found or has expired",
            })),
        )),
    }
}

pub async fn kill_cluster_node(
    State(manager): State<Arc<DemoClusterManager>>,
    Path((session_id, node_idx)): Path<(String, usize)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match manager.kill_node(&session_id, node_idx).await {
        Ok(msg) => Ok(Json(serde_json::json!({
            "success": true,
            "message": msg,
        }))),
        Err(err) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )),
    }
}

pub async fn revive_cluster_node(
    State(manager): State<Arc<DemoClusterManager>>,
    Path((session_id, node_idx)): Path<(String, usize)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match manager.revive_node(&session_id, node_idx).await {
        Ok(msg) => Ok(Json(serde_json::json!({
            "success": true,
            "message": msg,
        }))),
        Err(err) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )),
    }
}

#[derive(Deserialize)]
pub struct BenchmarkPayload {
    pub operations: Option<usize>,
}

pub async fn run_cluster_benchmark(
    State(manager): State<Arc<DemoClusterManager>>,
    Path(session_id): Path<String>,
    payload: Option<Json<BenchmarkPayload>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let ops = payload.map(|p| p.operations.unwrap_or(1000)).unwrap_or(1000);
    match manager.run_benchmark(&session_id, ops).await {
        Ok(result) => Ok(Json(result)),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )),
    }
}

pub async fn stop_demo_cluster(
    State(manager): State<Arc<DemoClusterManager>>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match manager.terminate_cluster(&session_id).await {
        Ok(()) => Ok(Json(serde_json::json!({
            "success": true,
            "message": "Demo cluster stopped and resources released",
        }))),
        Err(err) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )),
    }
}

pub async fn init_demo_control_plane(
    State(manager): State<Arc<DemoClusterManager>>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match manager.init_control_plane(&session_id).await {
        Ok(result) => Ok(Json(result)),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )),
    }
}

pub async fn get_internal_metrics_dashboard(
    State(manager): State<Arc<DemoClusterManager>>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let metrics = manager.get_metrics_summary().await;

    // Check if client explicitly requests JSON format
    let wants_json = params.get("format").map(|s| s == "json").unwrap_or(false)
        || params.contains_key("json")
        || headers
            .get(header::ACCEPT)
            .and_then(|h| h.to_str().ok())
            .map(|a| a.contains("application/json") && !a.contains("text/html"))
            .unwrap_or(false);

    if wants_json {
        let json_str = serde_json::to_string_pretty(&metrics).unwrap_or_default();
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
            .body(axum::body::Body::from(json_str))
            .unwrap()
    } else {
        let html = render_metrics_html(&metrics);
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(axum::body::Body::from(html))
            .unwrap()
    }
}

fn render_metrics_html(metrics: &serde_json::Value) -> String {
    let uptime = metrics["server"]["uptime_human"].as_str().unwrap_or("0s");
    let memory_rss = metrics["server"]["memory_rss_mb"].as_f64().unwrap_or(0.0);
    let active_ports = metrics["server"]["active_ports_count"].as_u64().unwrap_or(0);
    let unique_clients = metrics["server"]["unique_clients_seen"].as_u64().unwrap_or(0);

    let total_created = metrics["clusters"]["total_created"].as_u64().unwrap_or(0);
    let total_reaped = metrics["clusters"]["total_reaped"].as_u64().unwrap_or(0);
    let active_count = metrics["clusters"]["active_count"].as_u64().unwrap_or(0);
    let max_capacity = metrics["clusters"]["max_capacity"].as_u64().unwrap_or(6);
    let available_slots = metrics["clusters"]["available_slots"].as_u64().unwrap_or(6);
    let total_benchmarks = metrics["clusters"]["total_benchmarks_run"].as_u64().unwrap_or(0);

    let mut active_clusters_html = String::new();
    if let Some(active_list) = metrics["clusters"]["active"].as_array() {
        if active_list.is_empty() {
            active_clusters_html.push_str(r#"
            <div class="p-10 rounded-2xl bg-slate-900/60 border border-white/10 text-center">
              <div class="w-12 h-12 rounded-xl bg-slate-800/80 mx-auto flex items-center justify-center text-slate-400 mb-3">
                <svg class="w-6 h-6 text-cyan-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"/>
                </svg>
              </div>
              <h3 class="text-base font-semibold text-white">No Active Sandbox Clusters</h3>
              <p class="text-xs text-slate-400 mt-1">All sandbox slots are idle and ready for incoming visitors.</p>
            </div>
            "#);
        } else {
            for cluster in active_list {
                let session_id = cluster["session_id"].as_str().unwrap_or("unknown");
                let client_id = cluster["client_id"].as_str().unwrap_or("unknown");
                let cluster_id = cluster["cluster_id"].as_str().unwrap_or("unknown");
                let admin_port = cluster["admin_port"].as_u64().unwrap_or(0);
                let created_ago = cluster["created_secs_ago"].as_u64().unwrap_or(0);
                let remaining_secs = cluster["ttl_remaining_secs"].as_u64().unwrap_or(0);
                let remaining_human = cluster["ttl_remaining_human"].as_str().unwrap_or("00m 00s");
                let is_raft = cluster["is_raft_initialized"].as_bool().unwrap_or(false);
                let leader = cluster["current_leader"].as_u64();
                let alive_count = cluster["alive_nodes_count"].as_u64().unwrap_or(0);
                let total_nodes = cluster["total_nodes_count"].as_u64().unwrap_or(6);

                let progress_pct = (remaining_secs as f64 / 900.0 * 100.0).clamp(0.0, 100.0);
                let (ttl_badge_class, progress_color) = if remaining_secs > 300 {
                    ("bg-emerald-500/10 text-emerald-400 border-emerald-500/20", "bg-emerald-500")
                } else if remaining_secs > 60 {
                    ("bg-amber-500/10 text-amber-400 border-amber-500/20", "bg-amber-500")
                } else {
                    ("bg-rose-500/10 text-rose-400 border-rose-500/20", "bg-rose-500")
                };

                let raft_badge = if is_raft {
                    let leader_text = leader.map(|l| format!("Leader: {l}")).unwrap_or_else(|| "Elected".to_string());
                    format!(r#"<span class="text-xs font-mono px-2 py-0.5 rounded-full bg-emerald-500/10 text-emerald-400 border border-emerald-500/20">RAFT QUORUM ONLINE ({leader_text})</span>"#)
                } else {
                    r#"<span class="text-xs font-mono px-2 py-0.5 rounded-full bg-amber-500/10 text-amber-400 border border-amber-500/20">AWAITING RAFT BOOTSTRAP</span>"#.to_string()
                };

                let mut nodes_pills = String::new();
                if let Some(nodes) = cluster["nodes"].as_array() {
                    for n in nodes {
                        let id = n["id"].as_u64().unwrap_or(0);
                        let status = n["status"].as_str().unwrap_or("unknown");
                        let is_cp = n["is_control_plane"].as_bool().unwrap_or(false);
                        let quic = n["quic_port"].as_u64().unwrap_or(0);
                        let is_running = status == "running";

                        let (dot_color, border_color) = if is_running {
                            ("bg-emerald-400", "border-emerald-500/30")
                        } else {
                            ("bg-rose-500", "border-rose-500/30")
                        };

                        let short_name = if is_cp { format!("CP-{id}") } else { format!("W-{}", id.saturating_sub(3)) };

                        nodes_pills.push_str(&format!(
                            r#"<div class="px-2.5 py-1 rounded-lg bg-slate-900 border {border_color} flex items-center justify-between text-[11px] font-mono">
                                <div class="flex items-center gap-1.5">
                                  <span class="w-2 h-2 rounded-full {dot_color}"></span>
                                  <span class="text-slate-200 font-semibold">{short_name}</span>
                                </div>
                                <span class="text-slate-400">:{quic}</span>
                            </div>"#
                        ));
                    }
                }

                active_clusters_html.push_str(&format!(
                    r#"
                    <div class="p-6 rounded-2xl bg-slate-900/80 border border-white/10 space-y-4">
                      <div class="flex flex-wrap items-center justify-between gap-3 pb-3 border-b border-white/5">
                        <div class="flex items-center gap-3">
                          <a href="/demo/{session_id}/" target="_blank" class="font-mono text-sm font-bold text-cyan-400 hover:underline flex items-center gap-1">
                            {session_id} ↗
                          </a>
                          <span class="text-xs font-mono text-slate-400 px-2 py-0.5 rounded bg-white/5 border border-white/10">
                            {client_id}
                          </span>
                          <span class="text-xs font-mono text-slate-500 hidden sm:inline">
                            UUID: {cluster_id}
                          </span>
                        </div>

                        <div class="flex items-center gap-3">
                          {raft_badge}
                          <button onclick="terminateCluster('{session_id}')" class="px-3 py-1 rounded-lg text-xs font-mono font-medium bg-rose-500/10 hover:bg-rose-500/20 text-rose-400 border border-rose-500/30 transition">
                            Terminate
                          </button>
                        </div>
                      </div>

                      <div class="grid grid-cols-1 md:grid-cols-3 gap-4 text-xs font-mono">
                        <div class="p-3 rounded-xl bg-slate-950/60 border border-white/5 space-y-1">
                          <div class="text-slate-400 text-[10px] uppercase">Session Lifetime / Deadline</div>
                          <div class="flex items-center justify-between">
                            <span class="font-bold text-sm text-white">{remaining_human} remaining</span>
                            <span class="text-[10px] px-1.5 py-0.5 rounded border {ttl_badge_class}">TTL 15m</span>
                          </div>
                          <div class="w-full bg-slate-800 h-1.5 rounded-full overflow-hidden mt-2">
                            <div class="{progress_color} h-1.5 rounded-full transition-all duration-500" style="width: {progress_pct}%"></div>
                          </div>
                          <div class="text-[10px] text-slate-500 pt-1">Created {created_ago}s ago</div>
                        </div>

                        <div class="p-3 rounded-xl bg-slate-950/60 border border-white/5 space-y-1">
                          <div class="text-slate-400 text-[10px] uppercase">Nodes Alive</div>
                          <div class="font-bold text-sm text-white">{alive_count} / {total_nodes} Running</div>
                          <div class="text-[10px] text-slate-400 pt-1">Admin Console: <span class="text-cyan-400">127.0.0.1:{admin_port}</span></div>
                        </div>

                        <div class="p-3 rounded-xl bg-slate-950/60 border border-white/5 space-y-1">
                          <div class="text-slate-400 text-[10px] uppercase">Cluster Topology</div>
                          <div class="text-[11px] text-slate-300">3 Control Plane + 3 Data Workers</div>
                          <div class="text-[10px] text-slate-500 pt-1">Protocol: SWIM Gossip + QUIC Mesh</div>
                        </div>
                      </div>

                      <div class="pt-1">
                        <div class="text-[11px] font-mono text-slate-400 mb-2">NODES STATUS & PORTS:</div>
                        <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-6 gap-2">
                          {nodes_pills}
                        </div>
                      </div>
                    </div>
                    "#
                ));
            }
        }
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en" class="dark">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Aaron Demo Server — Internal Metrics</title>
  <script src="https://cdn.tailwindcss.com"></script>
  <link rel="icon" type="image/svg+xml" href="/favicon.svg">
  <script>
    tailwind.config = {{
      darkMode: 'class',
      theme: {{
        extend: {{
          colors: {{
            slate: {{
              950: '#070b14',
            }}
          }}
        }}
      }}
    }}
  </script>
</head>
<body class="bg-[#070b14] text-slate-100 min-h-screen font-sans antialiased p-4 sm:p-8">
  <div class="max-w-6xl mx-auto space-y-8">
    
    <!-- Top Nav / Header -->
    <header class="flex flex-col sm:flex-row sm:items-center justify-between gap-4 p-6 rounded-2xl bg-slate-900/60 border border-white/10">
      <div class="flex items-center gap-3">
        <div class="w-10 h-10 rounded-xl bg-cyan-500/10 border border-cyan-500/30 flex items-center justify-center text-cyan-400">
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z" />
          </svg>
        </div>
        <div>
          <div class="flex items-center gap-2">
            <h1 class="text-xl font-bold tracking-tight text-white">Aaron Internal Metrics</h1>
            <span class="text-[10px] font-mono px-2 py-0.5 rounded bg-cyan-500/10 text-cyan-400 border border-cyan-500/20 font-semibold">SECRET</span>
          </div>
          <p class="text-xs text-slate-400">Live sandbox supervisor, capacity telemetry & deadline observer</p>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-3 text-xs font-mono">
        <div class="flex items-center gap-2 px-3 py-1.5 rounded-xl bg-slate-950/80 border border-white/10">
          <span class="w-2 h-2 rounded-full bg-emerald-400 animate-pulse"></span>
          <span class="text-slate-300">Live</span>
          <span id="update-time" class="text-slate-500 text-[10px]"></span>
        </div>

        <label class="flex items-center gap-1.5 cursor-pointer px-3 py-1.5 rounded-xl bg-slate-950/80 border border-white/10 text-slate-300 hover:text-white transition">
          <input type="checkbox" id="autoRefresh" checked class="rounded bg-slate-800 border-white/20 text-cyan-500 focus:ring-0">
          <span>Auto-refresh (3s)</span>
        </label>

        <a href="?format=json" target="_blank" class="px-3 py-1.5 rounded-xl bg-slate-800 hover:bg-slate-700 border border-white/10 text-slate-200 transition">
          JSON ↗
        </a>

        <a href="/" class="px-3 py-1.5 rounded-xl bg-cyan-600 hover:bg-cyan-500 text-white font-semibold transition">
          Landing Page
        </a>
      </div>
    </header>

    <!-- Key Metrics Grid -->
    <div class="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-4">
      
      <!-- Total Spawned -->
      <div class="p-5 rounded-2xl bg-slate-900/60 border border-white/10 space-y-1">
        <div class="text-[11px] font-mono uppercase tracking-wider text-slate-400">Total Created</div>
        <div class="text-2xl sm:text-3xl font-bold font-mono text-cyan-400">{total_created}</div>
        <div class="text-[10px] text-slate-500">Clusters launched</div>
      </div>

      <!-- Active Now -->
      <div class="p-5 rounded-2xl bg-slate-900/60 border border-white/10 space-y-1">
        <div class="text-[11px] font-mono uppercase tracking-wider text-slate-400">Active Now</div>
        <div class="text-2xl sm:text-3xl font-bold font-mono text-emerald-400">{active_count} <span class="text-sm font-normal text-slate-500">/ {max_capacity}</span></div>
        <div class="text-[10px] text-slate-500">{available_slots} slots available</div>
      </div>

      <!-- Server Uptime -->
      <div class="p-5 rounded-2xl bg-slate-900/60 border border-white/10 space-y-1">
        <div class="text-[11px] font-mono uppercase tracking-wider text-slate-400">Server Uptime</div>
        <div class="text-lg sm:text-xl font-bold font-mono text-amber-400 truncate">{uptime}</div>
        <div class="text-[10px] text-slate-500">Process running</div>
      </div>

      <!-- Memory RSS -->
      <div class="p-5 rounded-2xl bg-slate-900/60 border border-white/10 space-y-1">
        <div class="text-[11px] font-mono uppercase tracking-wider text-slate-400">Memory RSS</div>
        <div class="text-2xl sm:text-3xl font-bold font-mono text-violet-400">{memory_rss:.1} <span class="text-xs font-normal text-slate-500">MB</span></div>
        <div class="text-[10px] text-slate-500">Resident memory</div>
      </div>

      <!-- Benchmarks Run -->
      <div class="p-5 rounded-2xl bg-slate-900/60 border border-white/10 space-y-1">
        <div class="text-[11px] font-mono uppercase tracking-wider text-slate-400">Benchmarks</div>
        <div class="text-2xl sm:text-3xl font-bold font-mono text-fuchsia-400">{total_benchmarks}</div>
        <div class="text-[10px] text-slate-500">Writes tested</div>
      </div>

      <!-- Unique Clients -->
      <div class="p-5 rounded-2xl bg-slate-900/60 border border-white/10 space-y-1">
        <div class="text-[11px] font-mono uppercase tracking-wider text-slate-400">Unique Clients</div>
        <div class="text-2xl sm:text-3xl font-bold font-mono text-indigo-400">{unique_clients}</div>
        <div class="text-[10px] text-slate-500">{active_ports} UDP/TCP ports</div>
      </div>

    </div>

    <!-- Active Clusters Section -->
    <div class="space-y-4">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2">
          <span class="w-2.5 h-2.5 rounded-full bg-emerald-400"></span>
          <h2 class="text-base font-bold uppercase tracking-wider text-slate-200">Live Active Clusters ({active_count})</h2>
        </div>
        <div class="text-xs font-mono text-slate-500">Total Reaped: {total_reaped}</div>
      </div>

      <div class="space-y-4">
        {active_clusters_html}
      </div>
    </div>

  </div>

  <script>
    document.getElementById('update-time').innerText = new Date().toLocaleTimeString();

    let timer = setInterval(() => {{
      const cb = document.getElementById('autoRefresh');
      if (cb && cb.checked) {{
        location.reload();
      }}
    }}, 3000);

    async function terminateCluster(sessionId) {{
      if (!confirm('Are you sure you want to terminate ' + sessionId + '?')) return;
      try {{
        const res = await fetch('/api/demo/' + sessionId + '/stop', {{ method: 'POST' }});
        if (res.ok) {{
          location.reload();
        }} else {{
          alert('Failed to terminate cluster');
        }}
      }} catch (err) {{
        alert('Error: ' + err.message);
      }}
    }}
  </script>
</body>
</html>"#
    )
}
