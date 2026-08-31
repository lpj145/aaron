use crate::proto::aaron::control_plane as proto;
use crate::proto::aaron::node as proto_node;
use crate::types::{ControlPlaneNode, TypeConfig};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::{CommittedLeaderId, Entry, EntryPayload, LogId, Vote};
use planus::{ReadAsRoot, WriteAsOffset};
use snafu::Snafu;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Snafu)]
pub enum MessageError {
    #[snafu(display("Planus FlatBuffers serialization error: {source}"))]
    Planus { source: planus::Error },
    #[snafu(display("Missing mandatory FlatBuffers field: {field}"))]
    MissingField { field: &'static str },
    #[snafu(display("Unknown or malformed message payload"))]
    UnknownPayload,
}

impl From<planus::Error> for MessageError {
    fn from(source: planus::Error) -> Self {
        Self::Planus { source }
    }
}

pub enum RaftMessage {
    Vote(VoteRequest<u64>),
    VoteResp(VoteResponse<u64>),
    Append(AppendEntriesRequest<TypeConfig>),
    AppendResp(AppendEntriesResponse<u64>),
    Snapshot(InstallSnapshotRequest<TypeConfig>),
    SnapshotResp(InstallSnapshotResponse<u64>),
}

pub(crate) fn encode_payload(payload: &EntryPayload<TypeConfig>) -> Vec<u8> {
    match payload {
        EntryPayload::Blank => vec![0],
        EntryPayload::Normal(data) => {
            let mut b = vec![1];
            match data {
                crate::types::ClientRequest::Set { key, value } => {
                    b.push(0);
                    b.extend_from_slice(&(key.len() as u32).to_le_bytes());
                    b.extend_from_slice(key.as_bytes());
                    b.extend_from_slice(value.as_bytes());
                }
                crate::types::ClientRequest::Delete { key } => {
                    b.push(1);
                    b.extend_from_slice(key.as_bytes());
                }
            }
            b
        }
        EntryPayload::Membership(mem) => {
            let mut b = vec![2];
            let voter_ids: Vec<u64> = mem.voter_ids().collect();
            b.extend_from_slice(&(voter_ids.len() as u32).to_le_bytes());
            for id in voter_ids {
                b.extend_from_slice(&id.to_le_bytes());
            }

            let nodes: Vec<(&u64, &ControlPlaneNode)> = mem.nodes().collect();
            b.extend_from_slice(&(nodes.len() as u32).to_le_bytes());
            for (id, node) in nodes {
                b.extend_from_slice(&id.to_le_bytes());
                b.extend_from_slice(&node.node_uuid_high.to_le_bytes());
                b.extend_from_slice(&node.node_uuid_low.to_le_bytes());
                let addr_bytes = node.addr.as_bytes();
                b.extend_from_slice(&(addr_bytes.len() as u32).to_le_bytes());
                b.extend_from_slice(addr_bytes);
            }
            b
        }
    }
}

pub(crate) fn decode_payload(bytes: &[u8]) -> EntryPayload<TypeConfig> {
    if bytes.is_empty() || bytes[0] == 0 {
        return EntryPayload::Blank;
    }

    if bytes[0] == 1 {
        let sub = &bytes[1..];
        if sub.is_empty() {
            return EntryPayload::Blank;
        }
        if sub[0] == 0 {
            // Set
            if sub.len() >= 5 {
                let mut len_bytes = [0u8; 4];
                len_bytes.copy_from_slice(&sub[1..5]);
                let k_len = u32::from_le_bytes(len_bytes) as usize;
                if sub.len() >= 5 + k_len {
                    let key = String::from_utf8_lossy(&sub[5..5 + k_len]).to_string();
                    let value = String::from_utf8_lossy(&sub[5 + k_len..]).to_string();
                    return EntryPayload::Normal(crate::types::ClientRequest::Set { key, value });
                }
            }
        } else {
            // Delete
            let key = String::from_utf8_lossy(&sub[1..]).to_string();
            return EntryPayload::Normal(crate::types::ClientRequest::Delete { key });
        }
    } else if bytes[0] == 2 {
        let mut cursor = 1;
        if bytes.len() >= cursor + 4 {
            let voter_count = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let mut voter_set = BTreeSet::new();
            for _ in 0..voter_count {
                if bytes.len() >= cursor + 8 {
                    let vid = u64::from_le_bytes(bytes[cursor..cursor + 8].try_into().unwrap());
                    voter_set.insert(vid);
                    cursor += 8;
                }
            }

            if bytes.len() >= cursor + 4 {
                let node_count = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
                cursor += 4;
                let mut node_map = BTreeMap::new();
                for _ in 0..node_count {
                    if bytes.len() >= cursor + 28 {
                        let nid = u64::from_le_bytes(bytes[cursor..cursor + 8].try_into().unwrap());
                        let high = u64::from_le_bytes(bytes[cursor + 8..cursor + 16].try_into().unwrap());
                        let low = u64::from_le_bytes(bytes[cursor + 16..cursor + 24].try_into().unwrap());
                        let addr_len = u32::from_le_bytes(bytes[cursor + 24..cursor + 28].try_into().unwrap()) as usize;
                        cursor += 28;
                        if bytes.len() >= cursor + addr_len {
                            let addr = String::from_utf8_lossy(&bytes[cursor..cursor + addr_len]).to_string();
                            cursor += addr_len;
                            let node = ControlPlaneNode {
                                addr,
                                node_uuid_high: high,
                                node_uuid_low: low,
                            };
                            node_map.insert(nid, node);
                        }
                    }
                }

                let membership = openraft::Membership::new(vec![voter_set], node_map);
                return EntryPayload::Membership(membership);
            }
        }
    }

    EntryPayload::Blank
}

impl RaftMessage {
    /// Serializes a Raft message into a FlatBuffers binary buffer.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut builder = planus::Builder::new();

        let payload = match self {
            Self::Vote(req) => {
                let cand_bytes = req.vote.leader_id().voted_for().unwrap_or(0).to_le_bytes();
                let high = u64::from_le_bytes(cand_bytes);
                let cand_proto = proto_node::Uuid { high, low: 0 };

                proto::ControlPlanePayload::VoteRequest(Box::new(proto::VoteRequest {
                    term: req.vote.leader_id().term,
                    candidate_id: Some(cand_proto),
                    last_log_term: req.last_log_id.map(|l| l.leader_id.term).unwrap_or(0),
                    last_log_index: req.last_log_id.map(|l| l.index).unwrap_or(0),
                }))
            }
            Self::VoteResp(resp) => {
                let voted_for_high = resp.vote.leader_id().voted_for().unwrap_or(0);
                let voted_for_proto = proto_node::Uuid { high: voted_for_high, low: 0 };

                proto::ControlPlanePayload::VoteResponse(Box::new(proto::VoteResponse {
                    term: resp.vote.leader_id().term,
                    vote_granted: resp.vote_granted,
                    last_log_term: resp.last_log_id.map(|l| l.leader_id.term).unwrap_or(0),
                    last_log_index: resp.last_log_id.map(|l| l.index).unwrap_or(0),
                    voted_for: Some(voted_for_proto),
                }))
            }
            Self::Append(req) => {
                let leader_bytes = req.vote.leader_id().voted_for().unwrap_or(0).to_le_bytes();
                let high = u64::from_le_bytes(leader_bytes);
                let leader_proto = proto_node::Uuid { high, low: 0 };

                let entries: Vec<_> = req
                    .entries
                    .iter()
                    .map(|e| {
                        proto::LogEntry {
                            term: e.log_id.leader_id.term,
                            index: e.log_id.index,
                            entry_type: 0,
                            payload: Some(encode_payload(&e.payload)),
                        }
                    })
                    .collect();

                proto::ControlPlanePayload::AppendEntriesRequest(Box::new(
                    proto::AppendEntriesRequest {
                        term: req.vote.leader_id().term,
                        leader_id: Some(leader_proto),
                        prev_log_term: req.prev_log_id.map(|l| l.leader_id.term).unwrap_or(0),
                        prev_log_index: req.prev_log_id.map(|l| l.index).unwrap_or(0),
                        entries: Some(entries),
                        leader_commit: req.leader_commit.map(|l| l.index).unwrap_or(0),
                    },
                ))
            }
            Self::AppendResp(resp) => {
                let (conflict_term, conflict_index) = match resp {
                    AppendEntriesResponse::Conflict => (1, 1),
                    _ => (0, 0),
                };

                let (term, voted_for_high) = match resp {
                    AppendEntriesResponse::HigherVote(v) => (v.leader_id().term, v.leader_id().voted_for().unwrap_or(0)),
                    _ => (0, 0),
                };

                let voted_for_proto = proto_node::Uuid { high: voted_for_high, low: 0 };

                proto::ControlPlanePayload::AppendEntriesResponse(Box::new(
                    proto::AppendEntriesResponse {
                        term,
                        success: resp.is_success(),
                        last_log_term: 0,
                        last_log_index: 0,
                        conflict_term,
                        conflict_index,
                        voted_for: Some(voted_for_proto),
                    },
                ))
            }
            Self::Snapshot(req) => {
                let leader_bytes = req.vote.leader_id().voted_for().unwrap_or(0).to_le_bytes();
                let high = u64::from_le_bytes(leader_bytes);
                let leader_proto = proto_node::Uuid { high, low: 0 };

                proto::ControlPlanePayload::InstallSnapshotRequest(Box::new(
                    proto::InstallSnapshotRequest {
                        term: req.vote.leader_id().term,
                        leader_id: Some(leader_proto),
                        last_included_term: req.meta.last_log_id.map(|l| l.leader_id.term).unwrap_or(0),
                        last_included_index: req.meta.last_log_id.map(|l| l.index).unwrap_or(0),
                        offset: req.offset,
                        data: Some(req.data.clone()),
                        done: req.done,
                    },
                ))
            }
            Self::SnapshotResp(resp) => {
                let voted_for_high = resp.vote.leader_id().voted_for().unwrap_or(0);
                let voted_for_proto = proto_node::Uuid { high: voted_for_high, low: 0 };

                proto::ControlPlanePayload::InstallSnapshotResponse(Box::new(
                    proto::InstallSnapshotResponse {
                        term: resp.vote.leader_id().term,
                        success: true,
                        voted_for: Some(voted_for_proto),
                    },
                ))
            }
        };

        let msg = proto::ControlPlaneMessage {
            payload: Some(payload),
        };
        let offset = msg.prepare(&mut builder);
        builder.finish(offset, None).to_vec()
    }

    /// Deserializes a FlatBuffers binary buffer into a strongly-typed `RaftMessage`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MessageError> {
        let msg_ref = proto::ControlPlaneMessageRef::read_as_root(bytes)?;

        let payload_ref = msg_ref
            .payload()?
            .ok_or(MessageError::UnknownPayload)?;

        match payload_ref {
            proto::ControlPlanePayloadRef::VoteRequest(req) => {
                let term = req.term()?;
                let cand_ref = req
                    .candidate_id()?
                    .ok_or(MessageError::MissingField {
                        field: "candidate_id",
                    })?;
                let cand_id = cand_ref.high();

                let last_log_term = req.last_log_term()?;
                let last_log_index = req.last_log_index()?;

                let last_log_id = if last_log_index > 0 {
                    Some(LogId::new(CommittedLeaderId::new(last_log_term, cand_id), last_log_index))
                } else {
                    None
                };

                let vote = Vote::new(term, cand_id);
                Ok(Self::Vote(VoteRequest {
                    vote,
                    last_log_id,
                }))
            }
            proto::ControlPlanePayloadRef::VoteResponse(resp) => {
                let term = resp.term()?;
                let vote_granted = resp.vote_granted()?;
                let last_log_term = resp.last_log_term()?;
                let last_log_index = resp.last_log_index()?;
                let voted_for = resp.voted_for()?.map(|v| v.high()).unwrap_or(0);

                let last_log_id = if last_log_index > 0 {
                    Some(LogId::new(CommittedLeaderId::new(last_log_term, voted_for), last_log_index))
                } else {
                    None
                };

                Ok(Self::VoteResp(VoteResponse {
                    vote: Vote::new(term, voted_for),
                    vote_granted,
                    last_log_id,
                }))
            }
            proto::ControlPlanePayloadRef::AppendEntriesRequest(req) => {
                let term = req.term()?;
                let leader_ref = req
                    .leader_id()?
                    .ok_or(MessageError::MissingField { field: "leader_id" })?;
                let leader_id = leader_ref.high();

                let prev_log_term = req.prev_log_term()?;
                let prev_log_index = req.prev_log_index()?;
                let leader_commit_idx = req.leader_commit()?;

                let prev_log_id = if prev_log_index > 0 {
                    Some(LogId::new(CommittedLeaderId::new(prev_log_term, leader_id), prev_log_index))
                } else {
                    None
                };

                let leader_commit = if leader_commit_idx > 0 {
                    Some(LogId::new(CommittedLeaderId::new(term, leader_id), leader_commit_idx))
                } else {
                    None
                };

                let mut entries = Vec::new();
                if let Some(proto_entries) = req.entries()? {
                    for entry_res in proto_entries {
                        let entry_ref = entry_res?;
                        let entry_term = entry_ref.term()?;
                        let entry_idx = entry_ref.index()?;
                        let payload_bytes = entry_ref.payload()?.unwrap_or_default();

                        let payload = decode_payload(payload_bytes);

                        entries.push(Entry {
                            log_id: LogId::new(CommittedLeaderId::new(entry_term, leader_id), entry_idx),
                            payload,
                        });
                    }
                }

                Ok(Self::Append(AppendEntriesRequest {
                    vote: Vote::new_committed(term, leader_id),
                    prev_log_id,
                    entries,
                    leader_commit,
                }))
            }
            proto::ControlPlanePayloadRef::AppendEntriesResponse(resp) => {
                let term = resp.term()?;
                let success = resp.success()?;
                let conflict_index = resp.conflict_index()?;
                let voted_for = resp.voted_for()?.map(|v| v.high()).unwrap_or(0);

                let res = if success {
                    AppendEntriesResponse::Success
                } else if conflict_index > 0 {
                    AppendEntriesResponse::Conflict
                } else {
                    AppendEntriesResponse::HigherVote(Vote::new(term, voted_for))
                };

                Ok(Self::AppendResp(res))
            }
            proto::ControlPlanePayloadRef::InstallSnapshotRequest(req) => {
                let term = req.term()?;
                let leader_ref = req
                    .leader_id()?
                    .ok_or(MessageError::MissingField { field: "leader_id" })?;
                let leader_id = leader_ref.high();

                let last_log_term = req.last_included_term()?;
                let last_log_index = req.last_included_index()?;
                let offset = req.offset()?;
                let data = req.data()?.unwrap_or_default().to_vec();
                let done = req.done()?;

                let meta = openraft::SnapshotMeta {
                    last_log_id: if last_log_index > 0 {
                        Some(LogId::new(CommittedLeaderId::new(last_log_term, leader_id), last_log_index))
                    } else {
                        None
                    },
                    last_membership: openraft::StoredMembership::default(),
                    snapshot_id: format!("{last_log_term}-{last_log_index}"),
                };

                Ok(Self::Snapshot(InstallSnapshotRequest {
                    vote: Vote::new_committed(term, leader_id),
                    meta,
                    offset,
                    data,
                    done,
                }))
            }
            proto::ControlPlanePayloadRef::InstallSnapshotResponse(resp) => {
                let term = resp.term()?;
                let voted_for = resp.voted_for()?.map(|v| v.high()).unwrap_or(0);
                Ok(Self::SnapshotResp(InstallSnapshotResponse {
                    vote: Vote::new(term, voted_for),
                }))
            }
            _ => Err(MessageError::UnknownPayload),
        }
    }
}
