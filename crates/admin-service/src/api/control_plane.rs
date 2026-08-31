use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use control_plane_service::types::ControlPlaneNode;
use node::Uuid;
use openraft::error::{ClientWriteError, InitializeError, RaftError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::state::AppState;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ControlPlaneNodeInfo {
    #[serde(default)]
    pub node_id: u64,
    pub addr: String,
    pub uuid: String,
}

#[derive(Serialize)]
pub struct ControlPlaneStatusResponse {
    pub available: bool,
    pub node_uuid: Option<String>,
    pub node_id: Option<u64>,
    pub node_id_str: Option<String>,
    pub is_leader: bool,
    pub leader_uuid: Option<String>,
    pub current_leader: Option<u64>,
    pub current_leader_str: Option<String>,
    pub current_term: u64,
    pub last_log_index: u64,
    pub last_applied_index: u64,
    pub voters: Vec<u64>,
    pub voter_uuids: Vec<String>,
    pub learners: Vec<u64>,
    pub learner_uuids: Vec<String>,
    pub nodes: BTreeMap<String, ControlPlaneNodeInfo>,
    pub state_data: BTreeMap<String, String>,
}

pub async fn get_control_plane_status(
    State(state): State<AppState>,
) -> Json<ControlPlaneStatusResponse> {
    let handle = match &state.control_plane {
        Some(h) => h,
        None => {
            return Json(ControlPlaneStatusResponse {
                available: false,
                node_uuid: Some(state.ctx.identity.id().to_string()),
                node_id: None,
                node_id_str: None,
                is_leader: false,
                leader_uuid: None,
                current_leader: None,
                current_leader_str: None,
                current_term: 0,
                last_log_index: 0,
                last_applied_index: 0,
                voters: Vec::new(),
                voter_uuids: Vec::new(),
                learners: Vec::new(),
                learner_uuids: Vec::new(),
                nodes: BTreeMap::new(),
                state_data: BTreeMap::new(),
            });
        }
    };

    let metrics = handle.metrics();
    let node_id = handle.node_id();
    let is_leader = handle.is_leader();
    let current_leader = handle.current_leader();
    let state_data = handle.all_data().await;

    let (current_term, last_log_index, last_applied_index, voters, voter_uuids, learners, learner_uuids, nodes) =
        if let Some(m) = metrics {
            let term = m.current_term;
            let last_log = m.last_log_index.unwrap_or(0);
            let last_applied = m.last_applied.map(|l| l.index).unwrap_or(0);

            let mut voter_ids = Vec::new();
            let mut voter_uuid_list = Vec::new();
            let mut learner_ids = Vec::new();
            let mut learner_uuid_list = Vec::new();
            let mut node_map = BTreeMap::new();

            for voter_id in m.membership_config.membership().voter_ids() {
                voter_ids.push(voter_id);
            }

            for (nid, node) in m.membership_config.membership().nodes() {
                let uuid_str = node.node_uuid().to_string();
                if voter_ids.contains(nid) {
                    voter_uuid_list.push(uuid_str.clone());
                } else {
                    learner_ids.push(*nid);
                    learner_uuid_list.push(uuid_str.clone());
                }
                node_map.insert(
                    uuid_str.clone(),
                    ControlPlaneNodeInfo {
                        node_id: *nid,
                        addr: node.addr.clone(),
                        uuid: uuid_str,
                    },
                );
            }

            (
                term,
                last_log,
                last_applied,
                voter_ids,
                voter_uuid_list,
                learner_ids,
                learner_uuid_list,
                node_map,
            )
        } else {
            (0, 0, 0, Vec::new(), Vec::new(), Vec::new(), Vec::new(), BTreeMap::new())
        };

    let leader_uuid = current_leader.and_then(|lid| {
        nodes.values().find(|n| n.node_id == lid).map(|n| n.uuid.clone())
    });

    Json(ControlPlaneStatusResponse {
        available: true,
        node_uuid: Some(state.ctx.identity.id().to_string()),
        node_id,
        node_id_str: node_id.map(|n| n.to_string()),
        is_leader,
        leader_uuid,
        current_leader,
        current_leader_str: current_leader.map(|n| n.to_string()),
        current_term,
        last_log_index,
        last_applied_index,
        voters,
        voter_uuids,
        learners,
        learner_uuids,
        nodes,
        state_data,
    })
}

#[derive(Deserialize, Default)]
pub struct InitClusterRequest {
    #[serde(default)]
    pub voters: Vec<ControlPlaneNodeInfo>,
    #[serde(default)]
    pub learners: Vec<ControlPlaneNodeInfo>,
}

pub async fn init_control_plane_cluster(
    State(state): State<AppState>,
    Json(payload): Json<InitClusterRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let handle = state.control_plane.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Control Plane service is not enabled on this node" })),
        )
    })?;

    let local_node_id = handle.node_id().unwrap_or_else(|| state.ctx.identity.id().low);
    let local_uuid = state.ctx.identity.id();

    // Determine the real network address of the local node
    let local_cp_addr = if let Some(ref membership) = state.membership
        && let Some(lm) = membership.local_member().await
    {
        let cp_port = crate::api::cluster::derive_cp_port(lm.addr.port());
        format!("{}:{}", lm.addr.ip(), cp_port)
    } else {
        "127.0.0.1:18946".to_string()
    };

    let mut voters_map = BTreeMap::new();

    if payload.voters.is_empty() {
        // Auto-bootstrap cluster with all active discovered SWIM members
        if let Some(ref membership) = state.membership {
            let active_members = membership.active_members().await;
            for m in active_members {
                let nid = m.node_id.id().low;
                let cp_port = crate::api::cluster::derive_cp_port(m.addr.port());
                let cp_addr = format!("{}:{}", m.addr.ip(), cp_port);
                voters_map.insert(nid, ControlPlaneNode::new(cp_addr, m.node_id.id()));
            }
        }

        // Ensure the local node is present with its real routable network address
        voters_map.insert(local_node_id, ControlPlaneNode::new(local_cp_addr.clone(), local_uuid));
    } else {
        for v in &payload.voters {
            let uuid = v.uuid.parse::<Uuid>().map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Invalid UUID for node {}: {}", v.node_id, e) })),
                )
            })?;
            // Derive nid directly from uuid.low to avoid JS float rounding
            let nid = uuid.low;
            let mut addr = v.addr.clone();
            if uuid == local_uuid && (addr.starts_with("127.0.0.1") || addr.starts_with("0.0.0.0")) {
                addr = local_cp_addr.clone();
            }
            voters_map.insert(nid, ControlPlaneNode::new(addr, uuid));
        }

        // Ensure the local node is present in initial voters with its real routable network address
        voters_map.entry(local_node_id).or_insert_with(|| {
            ControlPlaneNode::new(local_cp_addr.clone(), local_uuid)
        });
    }

    if let Err(err) = handle.initialize(voters_map.clone()).await {
        let (status, msg) = match err {
            RaftError::APIError(InitializeError::NotAllowed(_)) => (
                StatusCode::CONFLICT,
                "Raft cluster has already been initialized".to_string(),
            ),
            RaftError::APIError(InitializeError::NotInMembers(e)) => (
                StatusCode::BAD_REQUEST,
                format!("Local node {} is not included in the voter set", e.node_id),
            ),
            other => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to initialize Raft cluster: {other}"),
            ),
        };
        return Err((status, Json(serde_json::json!({ "error": msg }))));
    }

    // Optionally register initial learners
    for l in payload.learners {
        if let Ok(uuid) = l.uuid.parse::<Uuid>() {
            let nid = uuid.low;
            if nid != local_node_id && !voters_map.contains_key(&nid) {
                let _ = handle
                    .add_learner(nid, ControlPlaneNode::new(l.addr, uuid), false)
                    .await;
            }
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Raft Control Plane cluster successfully initialized with {} voter(s)", voters_map.len())
    })))
}

/// Transparently proxies a Control Plane mutating request to the current Raft leader's Admin Service HTTP port.
async fn proxy_request_to_leader<T: Serialize>(
    state: &AppState,
    leader_id: u64,
    path: &str,
    method: reqwest::Method,
    body: Option<&T>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let leader_ip = if let Some(ref membership) = state.membership {
        membership
            .all_members()
            .await
            .into_iter()
            .find(|m| m.node_id.id().low == leader_id)
            .map(|m| m.addr.ip())
    } else {
        None
    };

    let leader_ip = match leader_ip {
        Some(ip) => ip,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": format!("Raft leader (node_id: {leader_id}) IP address could not be resolved from membership table")
                })),
            ));
        }
    };

    let url = format!("http://{leader_ip}:8080{path}");
    tracing::info!(
        target: "admin_service",
        leader_id = leader_id,
        leader_ip = %leader_ip,
        url = %url,
        "Transparently proxying Control Plane write request to Raft leader"
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to create proxy client: {e}") })),
            )
        })?;

    let mut req = client.request(method, &url);
    if let Some(b) = body {
        req = req.json(b);
    }

    let resp = req.send().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Failed to forward request to Raft leader at {url}: {e}") })),
        )
    })?;

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let resp_json: serde_json::Value = resp.json().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to parse leader response JSON: {e}") })),
        )
    })?;

    if status.is_success() {
        Ok(Json(resp_json))
    } else {
        Err((status, Json(resp_json)))
    }
}

#[derive(Serialize, Deserialize)]
pub struct ChangeMembershipRequest {
    #[serde(default)]
    pub voter_ids: Vec<u64>,
    #[serde(default)]
    pub voter_uuids: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<ControlPlaneNodeInfo>,
    #[serde(default = "default_true")]
    pub retain: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ChangeMembershipRequest {
    fn default() -> Self {
        Self {
            voter_ids: Vec::new(),
            voter_uuids: Vec::new(),
            nodes: Vec::new(),
            retain: true,
        }
    }
}

pub async fn change_control_plane_membership(
    State(state): State<AppState>,
    Json(payload): Json<ChangeMembershipRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let handle = state.control_plane.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Control Plane service is not enabled on this node" })),
        )
    })?;

    // 1. If not currently leader, transparently proxy to the leader
    if !handle.is_leader()
        && let Some(leader_id) = handle.current_leader() {
            return proxy_request_to_leader(&state, leader_id, "/api/control-plane/membership", reqwest::Method::POST, Some(&payload)).await;
        }

    let mut voter_set = BTreeSet::new();

    // 1. Populate from voter_uuids (immune to JS float precision loss)
    for uuid_str in &payload.voter_uuids {
        if let Ok(uuid) = uuid_str.parse::<Uuid>() {
            voter_set.insert(uuid.low);
        }
    }

    // 2. Populate from nodes list if voter_uuids is empty
    if voter_set.is_empty() && !payload.nodes.is_empty() {
        for node in &payload.nodes {
            if let Ok(uuid) = node.uuid.parse::<Uuid>() {
                voter_set.insert(uuid.low);
            }
        }
    }

    // 3. Fallback to voter_ids
    if voter_set.is_empty() {
        for id in &payload.voter_ids {
            voter_set.insert(*id);
        }
    }

    if voter_set.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Voter set cannot be empty" })),
        ));
    }

    // Register candidate endpoints as learners first (only for explicit candidate nodes)
    for node in &payload.nodes {
        if let Ok(uuid) = node.uuid.parse::<Uuid>() {
            let nid = uuid.low;
            let _ = handle
                .add_learner(nid, ControlPlaneNode::new(node.addr.clone(), uuid), false)
                .await;
        }
    }

    let retain = payload.retain;
    match handle.change_membership(voter_set, retain).await {
        Ok(_) => Ok(Json(serde_json::json!({
            "success": true,
            "message": "Control Plane membership updated successfully"
        }))),
        Err(RaftError::APIError(ClientWriteError::ForwardToLeader(f))) => {
            if let Some(leader_id) = f.leader_id {
                proxy_request_to_leader(&state, leader_id, "/api/control-plane/membership", reqwest::Method::POST, Some(&payload)).await
            } else {
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "No active Raft leader available to process membership change" })),
                ))
            }
        }
        Err(other) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to change Raft membership: {other}") })),
        )),
    }
}

#[derive(Serialize, Deserialize)]
pub struct AddLearnerRequest {
    pub uuid: String,
    pub addr: String,
}

pub async fn add_control_plane_learner(
    State(state): State<AppState>,
    Json(payload): Json<AddLearnerRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let handle = state.control_plane.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Control Plane service is not enabled on this node" })),
        )
    })?;

    // 1. If not currently leader, transparently proxy to the leader
    if !handle.is_leader()
        && let Some(leader_id) = handle.current_leader() {
            return proxy_request_to_leader(&state, leader_id, "/api/control-plane/learner", reqwest::Method::POST, Some(&payload)).await;
        }

    let uuid = payload.uuid.parse::<Uuid>().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid UUID '{}': {}", payload.uuid, e) })),
        )
    })?;

    let nid = uuid.low;
    let node = ControlPlaneNode::new(payload.addr.clone(), uuid);

    match handle.add_learner(nid, node, false).await {
        Ok(_) => Ok(Json(serde_json::json!({
            "success": true,
            "message": format!("Node {uuid} successfully registered as a learner")
        }))),
        Err(RaftError::APIError(ClientWriteError::ForwardToLeader(f))) => {
            if let Some(leader_id) = f.leader_id {
                proxy_request_to_leader(&state, leader_id, "/api/control-plane/learner", reqwest::Method::POST, Some(&payload)).await
            } else {
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "No active Raft leader available to register learner" })),
                ))
            }
        }
        Err(other) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to add learner: {other}") })),
        )),
    }
}

#[derive(Serialize, Deserialize)]
pub struct RemoveRaftNodeRequest {
    pub uuid: String,
}

pub async fn remove_control_plane_node(
    State(state): State<AppState>,
    Json(payload): Json<RemoveRaftNodeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let handle = state.control_plane.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Control Plane service is not enabled on this node" })),
        )
    })?;

    // 1. If not currently leader, transparently proxy to the leader
    if !handle.is_leader()
        && let Some(leader_id) = handle.current_leader() {
            return proxy_request_to_leader(&state, leader_id, "/api/control-plane/remove-node", reqwest::Method::POST, Some(&payload)).await;
        }

    let uuid = payload.uuid.parse::<Uuid>().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid Node UUID '{}': {}", payload.uuid, e) })),
        )
    })?;

    let nid = uuid.low;

    match handle.remove_node_from_raft(nid).await {
        Ok(_) => Ok(Json(serde_json::json!({
            "success": true,
            "message": format!("Node {uuid} ({nid}) removed from Raft consensus")
        }))),
        Err(RaftError::APIError(ClientWriteError::ForwardToLeader(f))) => {
            if let Some(leader_id) = f.leader_id {
                proxy_request_to_leader(&state, leader_id, "/api/control-plane/remove-node", reqwest::Method::POST, Some(&payload)).await
            } else {
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "No active Raft leader available to process removal" })),
                ))
            }
        }
        Err(other) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to remove node from Raft: {other}") })),
        )),
    }
}

#[derive(Serialize, Deserialize)]
pub struct WriteStateRequest {
    pub key: String,
    pub value: String,
}

pub async fn write_control_plane_state(
    State(state): State<AppState>,
    Json(payload): Json<WriteStateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let handle = state.control_plane.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Control Plane service is not enabled on this node" })),
        )
    })?;

    // 1. If not currently leader, transparently proxy to the leader
    if !handle.is_leader()
        && let Some(leader_id) = handle.current_leader() {
            return proxy_request_to_leader(&state, leader_id, "/api/control-plane/write", reqwest::Method::POST, Some(&payload)).await;
        }

    match handle.set(&payload.key, &payload.value).await {
        Ok(_) => Ok(Json(serde_json::json!({
            "success": true,
            "message": format!("Key '{}' replicated across Raft cluster", payload.key)
        }))),
        Err(RaftError::APIError(ClientWriteError::ForwardToLeader(f))) => {
            if let Some(leader_id) = f.leader_id {
                proxy_request_to_leader(&state, leader_id, "/api/control-plane/write", reqwest::Method::POST, Some(&payload)).await
            } else {
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "No active Raft leader available to process write" })),
                ))
            }
        }
        Err(other) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to write replicated state: {other}") })),
        )),
    }
}

#[derive(Serialize, Deserialize)]
pub struct DeleteStateRequest {
    pub key: String,
}

pub async fn delete_control_plane_state(
    State(state): State<AppState>,
    Json(payload): Json<DeleteStateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let handle = state.control_plane.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Control Plane service is not enabled on this node" })),
        )
    })?;

    // 1. If not currently leader, transparently proxy to the leader
    if !handle.is_leader()
        && let Some(leader_id) = handle.current_leader() {
            return proxy_request_to_leader(&state, leader_id, "/api/control-plane/delete", reqwest::Method::POST, Some(&payload)).await;
        }

    match handle.delete(&payload.key).await {
        Ok(_) => Ok(Json(serde_json::json!({
            "success": true,
            "message": format!("Key '{}' deleted from replicated state", payload.key)
        }))),
        Err(RaftError::APIError(ClientWriteError::ForwardToLeader(f))) => {
            if let Some(leader_id) = f.leader_id {
                proxy_request_to_leader(&state, leader_id, "/api/control-plane/delete", reqwest::Method::POST, Some(&payload)).await
            } else {
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "No active Raft leader available to process delete" })),
                ))
            }
        }
        Err(other) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to delete replicated key: {other}") })),
        )),
    }
}
