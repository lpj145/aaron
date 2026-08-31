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
    let keyspaces = state.ctx.store.list_keyspaces();
    Json(StoreInfoResponse {
        path: state.ctx.store.path().display().to_string(),
        keyspaces,
        maintenance: state.ctx.store.is_maintenance(),
    })
}

#[derive(Deserialize)]
pub struct ScanQuery {
    pub prefix: Option<String>,
    pub limit: Option<usize>,
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
}

pub async fn scan_keyspace(
    State(state): State<AppState>,
    Path(keyspace_name): Path<String>,
    Query(query): Query<ScanQuery>,
) -> Result<Json<KeyspaceScanResponse>, (StatusCode, Json<serde_json::Value>)> {
    let ks = state
        .ctx
        .store
        .keyspace(&keyspace_name)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))))?;

    let limit = query.limit.unwrap_or(50).min(500);
    let prefix_str = query.prefix.unwrap_or_default();

    let page = ks
        .scan_prefix(prefix_str.as_bytes(), None::<&[u8]>, limit)
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Scan error: {err}") })),
            )
        })?;

    let entries: Vec<KeyEntryResponse> = page
        .items
        .into_iter()
        .map(|item| {
            let key_bytes: &[u8] = &item.key;
            let val_bytes: &[u8] = &item.value;

            let k_str = item.key_str().map(|s| s.to_string()).unwrap_or_else(|| format!("0x{}", hex_encode(key_bytes)));
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

    Ok(Json(KeyspaceScanResponse {
        keyspace: keyspace_name,
        entries,
        has_more: page.has_more,
        total_scanned,
    }))
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
    let ks = state
        .ctx
        .store
        .keyspace(&keyspace_name)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))))?;

    let val = ks
        .get(query.key.as_bytes())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    match val {
        Some(bytes) => {
            let b_slice: &[u8] = bytes.as_ref();
            let str_val = String::from_utf8(b_slice.to_vec()).unwrap_or_else(|_| hex_encode(b_slice));
            Ok(Json(GetKeyResponse {
                key: query.key,
                value: Some(str_val),
                exists: true,
            }))
        }
        None => Ok(Json(GetKeyResponse {
            key: query.key,
            value: None,
            exists: false,
        })),
    }
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
    let ks = state
        .ctx
        .store
        .keyspace(&keyspace_name)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))))?;

    ks.insert(payload.key.as_bytes(), payload.value.as_bytes())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    let _ = state.ctx.store.persist();

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
    let ks = state
        .ctx
        .store
        .keyspace(&keyspace_name)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e.to_string() }))))?;

    ks.remove(query.key.as_bytes())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    let _ = state.ctx.store.persist();

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
    let name = payload.name.trim();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Keyspace name cannot be empty" })),
        ));
    }

    state
        .ctx
        .store
        .keyspace(name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Keyspace '{}' initialized", name)
    })))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
