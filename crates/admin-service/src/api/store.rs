use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use node::KeyspaceExt;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Serialize)]
pub struct StoreInfoResponse {
    pub path: String,
    pub keyspaces: Vec<String>,
    pub maintenance: bool,
}

pub async fn get_store_info(State(state): State<AppState>) -> Json<StoreInfoResponse> {
    let store = state.ctx.store.clone();
    let (path, keyspaces, maintenance) = tokio::task::spawn_blocking(move || {
        (
            store.path().display().to_string(),
            store.list_keyspaces(),
            store.is_maintenance(),
        )
    })
    .await
    .unwrap_or_else(|_| (String::new(), Vec::new(), false));

    Json(StoreInfoResponse {
        path,
        keyspaces,
        maintenance,
    })
}

#[derive(Deserialize)]
pub struct ScanQuery {
    pub prefix: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Serialize)]
pub struct KeyEntryResponse {
    pub key: String,
    pub key_hex: String,
    pub value_str: Option<String>,
    pub value_hex: String,
    pub size_bytes: usize,
}

#[derive(Serialize)]
pub struct KeyspaceScanResponse {
    pub keyspace: String,
    pub entries: Vec<KeyEntryResponse>,
    pub has_more: bool,
    pub total_scanned: usize,
    pub next_cursor: Option<String>,
}

pub async fn scan_keyspace(
    State(state): State<AppState>,
    Path(keyspace_name): Path<String>,
    Query(query): Query<ScanQuery>,
) -> Result<Json<KeyspaceScanResponse>, (StatusCode, Json<serde_json::Value>)> {
    let store = state.ctx.store.clone();
    let limit = query.limit.unwrap_or(50).min(500);
    let prefix_str = query.prefix.unwrap_or_default();
    let ks_name = keyspace_name.clone();

    let res = tokio::task::spawn_blocking(move || {
        let ks = store
            .keyspace(&ks_name)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        let cursor_bytes = query.cursor.map(|c| {
            if let Some(hex) = c.strip_prefix("0x") {
                hex_decode(hex).unwrap_or_else(|_| c.into_bytes())
            } else {
                c.into_bytes()
            }
        });

        let page = ks
            .scan_prefix(prefix_str.as_bytes(), cursor_bytes.as_deref(), limit)
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, format!("Scan error: {err}")))?;

        let entries: Vec<KeyEntryResponse> = page
            .items
            .into_iter()
            .map(|item| {
                let key_bytes: &[u8] = &item.key;
                let val_bytes: &[u8] = &item.value;

                let k_str = match item.key_str() {
                    Some(s) if s.chars().all(|c| !c.is_control() || c == '\n' || c == '\t') => {
                        s.to_string()
                    }
                    _ => {
                        if key_bytes.len() >= 2
                            && let Ok(suffix) = std::str::from_utf8(&key_bytes[2..])
                            && suffix.chars().all(|c| !c.is_control() || c == '\n' || c == '\t')
                        {
                            let shard_id = u16::from_be_bytes([key_bytes[0], key_bytes[1]]);
                            format!("[shard:{shard_id}] {suffix}")
                        } else {
                            format!("0x{}", hex_encode(key_bytes))
                        }
                    }
                };
                let k_hex = hex_encode(key_bytes);
                let v_str = item.value_str().map(|s| s.to_string());
                let v_hex = hex_encode(val_bytes);
                let size_bytes = val_bytes.len();

                KeyEntryResponse {
                    key: k_str,
                    key_hex: k_hex,
                    value_str: v_str,
                    value_hex: v_hex,
                    size_bytes,
                }
            })
            .collect();

        let total_scanned = entries.len();
        let next_cursor = page.next_cursor.map(|c| {
            String::from_utf8(c.to_vec()).unwrap_or_else(|_| format!("0x{}", hex_encode(&c)))
        });

        Ok(KeyspaceScanResponse {
            keyspace: ks_name,
            entries,
            has_more: page.has_more,
            total_scanned,
            next_cursor,
        })
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("Task error: {e}") }))))?
    .map_err(|(code, msg)| (code, Json(serde_json::json!({ "error": msg }))))?;

    Ok(Json(res))
}

#[derive(Deserialize)]
pub struct KeyQuery {
    pub key: String,
}

#[derive(Serialize)]
pub struct GetKeyResponse {
    pub key: String,
    pub value: Option<String>,
    pub exists: bool,
}

pub async fn get_key(
    State(state): State<AppState>,
    Path(keyspace_name): Path<String>,
    Query(query): Query<KeyQuery>,
) -> Result<Json<GetKeyResponse>, (StatusCode, Json<serde_json::Value>)> {
    let store = state.ctx.store.clone();
    let key_to_get = query.key.clone();

    let res = tokio::task::spawn_blocking(move || {
        let ks = store
            .keyspace(&keyspace_name)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        let val = ks
            .get(key_to_get.as_bytes())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        match val {
            Some(bytes) => {
                let b_slice: &[u8] = bytes.as_ref();
                let str_val = String::from_utf8(b_slice.to_vec()).unwrap_or_else(|_| hex_encode(b_slice));
                Ok(GetKeyResponse {
                    key: key_to_get,
                    value: Some(str_val),
                    exists: true,
                })
            }
            None => Ok(GetKeyResponse {
                key: key_to_get,
                value: None,
                exists: false,
            }),
        }
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("Task error: {e}") }))))?
    .map_err(|(code, msg)| (code, Json(serde_json::json!({ "error": msg }))))?;

    Ok(Json(res))
}

#[derive(Deserialize)]
pub struct SetKeyRequest {
    pub key: String,
    pub value: String,
}

pub async fn set_key(
    State(state): State<AppState>,
    Path(keyspace_name): Path<String>,
    Json(payload): Json<SetKeyRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let store = state.ctx.store.clone();
    let key = payload.key.clone();
    let val = payload.value.clone();
    let ks_name = keyspace_name.clone();

    tokio::task::spawn_blocking(move || {
        let ks = store
            .keyspace(&ks_name)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        ks.insert(key.as_bytes(), val.as_bytes())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let _ = store.persist();
        Ok(())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("Task error: {e}") }))))?
    .map_err(|(code, msg)| (code, Json(serde_json::json!({ "error": msg }))))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Key '{}' set in keyspace '{}'", payload.key, keyspace_name)
    })))
}

pub async fn delete_key(
    State(state): State<AppState>,
    Path(keyspace_name): Path<String>,
    Query(query): Query<KeyQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let store = state.ctx.store.clone();
    let key = query.key.clone();
    let ks_name = keyspace_name.clone();

    tokio::task::spawn_blocking(move || {
        let ks = store
            .keyspace(&ks_name)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        ks.remove(key.as_bytes())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let _ = store.persist();
        Ok(())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("Task error: {e}") }))))?
    .map_err(|(code, msg)| (code, Json(serde_json::json!({ "error": msg }))))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Key '{}' removed from keyspace '{}'", query.key, keyspace_name)
    })))
}

#[derive(Deserialize)]
pub struct CreateKeyspaceRequest {
    pub name: String,
}

pub async fn create_keyspace(
    State(state): State<AppState>,
    Json(payload): Json<CreateKeyspaceRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Keyspace name cannot be empty" })),
        ));
    }

    let store = state.ctx.store.clone();
    tokio::task::spawn_blocking(move || {
        store
            .keyspace(&name)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("Task error: {e}") }))))?
    .map_err(|(code, msg)| (code, Json(serde_json::json!({ "error": msg }))))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Keyspace '{}' initialized", payload.name.trim())
    })))
}

#[derive(Deserialize)]
pub struct BenchmarkRequest {
    pub keyspace: Option<String>,
    pub operations: Option<usize>,
    pub val_size_bytes: Option<usize>,
}

#[derive(Serialize)]
pub struct BenchmarkResponse {
    pub keyspace: String,
    pub operations: usize,
    pub val_size_bytes: usize,
    pub write_ops_sec: f64,
    pub write_latency_avg_us: f64,
    pub write_throughput_mb_s: f64,
    pub read_ops_sec: f64,
    pub read_latency_avg_us: f64,
    pub read_throughput_mb_s: f64,
    pub total_duration_ms: f64,
}

pub async fn run_store_benchmark(
    State(state): State<AppState>,
    Json(payload): Json<BenchmarkRequest>,
) -> Result<Json<BenchmarkResponse>, (StatusCode, Json<serde_json::Value>)> {
    let store = state.ctx.store.clone();
    let ks_name = payload.keyspace.unwrap_or_else(|| "benchmark".to_string());
    let operations = payload.operations.unwrap_or(1000).clamp(50, 50_000);
    let val_size = payload.val_size_bytes.unwrap_or(128).clamp(16, 65_536);

    let res = tokio::task::spawn_blocking(move || {
        let ks = store.keyspace(&ks_name).map_err(|e| e.to_string())?;

        let val_payload = vec![b'a'; val_size];
        let keys: Vec<String> = (0..operations)
            .map(|i| format!("__bench_{:08}", i))
            .collect();

        // 1. Benchmark Sequential Writes
        let write_start = std::time::Instant::now();
        for k in &keys {
            ks.insert(k.as_bytes(), &val_payload).map_err(|e| e.to_string())?;
        }
        let write_duration = write_start.elapsed();

        // 2. Benchmark Sequential / Point Reads
        let read_start = std::time::Instant::now();
        for k in &keys {
            let _ = ks.get(k.as_bytes()).map_err(|e| e.to_string())?;
        }
        let read_duration = read_start.elapsed();

        // 3. Cleanup benchmark keys from keyspace
        for k in &keys {
            let _ = ks.remove(k.as_bytes());
        }
        let _ = store.persist();

        let write_secs = write_duration.as_secs_f64().max(0.000001);
        let read_secs = read_duration.as_secs_f64().max(0.000001);

        let write_ops_sec = (operations as f64) / write_secs;
        let write_latency_avg_us = (write_duration.as_micros() as f64) / (operations as f64);
        let write_total_bytes = (operations * val_size) as f64;
        let write_throughput_mb_s = (write_total_bytes / (1024.0 * 1024.0)) / write_secs;

        let read_ops_sec = (operations as f64) / read_secs;
        let read_latency_avg_us = (read_duration.as_micros() as f64) / (operations as f64);
        let read_total_bytes = (operations * val_size) as f64;
        let read_throughput_mb_s = (read_total_bytes / (1024.0 * 1024.0)) / read_secs;

        let total_duration_ms = (write_duration + read_duration).as_secs_f64() * 1000.0;

        Ok(BenchmarkResponse {
            keyspace: ks_name,
            operations,
            val_size_bytes: val_size,
            write_ops_sec,
            write_latency_avg_us,
            write_throughput_mb_s,
            read_ops_sec,
            read_latency_avg_us,
            read_throughput_mb_s,
            total_duration_ms,
        })
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("Task join error: {e}") }))))?
    .map_err(|e: String| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;

    Ok(Json(res))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    if !s.len().is_multiple_of(2) {
        return Err(());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
        .collect()
}
