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
    ShardCommand {
        shard_id: u32,
        role: u8,
        primary_high: u64,
        primary_low: u64,
        replicas: Vec<(u64, u64)>,
        epoch: u64,
        op_type: u8,
        target_role: u8,
    },
    ShardCommandResp {
        success: bool,
        shard_id: u32,
        current_role: u8,
        term: u64,
        reject_reason: u8,
    },
    TelemetryHeartbeat {
        node_id_high: u64,
        node_id_low: u64,
        current_wps: u32,
        error_rate: u32,
        timestamp: u64,
    },
    TelemetryHeartbeatResp {
        acknowledged: bool,
    },
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
                    b.extend_from_slice(value);
                }
                crate::types::ClientRequest::Delete { key } => {
                    b.push(1);
                    b.extend_from_slice(key.as_bytes());
                }
                crate::types::ClientRequest::SetBatch { entries } => {
                    b.push(2);
                    b.extend_from_slice(&(entries.len() as u32).to_le_bytes());
                    for (k, v) in entries {
                        b.extend_from_slice(&(k.len() as u32).to_le_bytes());
                        b.extend_from_slice(k.as_bytes());
                        b.extend_from_slice(&(v.len() as u32).to_le_bytes());
                        b.extend_from_slice(v);
                    }
                }
            }
            b
        }
        EntryPayload::Membership(mem) => {
            let mut bytes = vec![3];
            bytes.extend(crate::storage::serialize_stored_membership(
                &openraft::StoredMembership::new(None, mem.clone()),
            ));
            bytes
        }
    }
}

pub(crate) fn decode_payload(bytes: &[u8]) -> Result<EntryPayload<TypeConfig>, MessageError> {
    use std::io::{Cursor, Read};
    fn take(cursor: &mut Cursor<&[u8]>, len: usize) -> Result<Vec<u8>, MessageError> {
        let remaining = cursor
            .get_ref()
            .len()
            .saturating_sub(cursor.position() as usize);
        if len > remaining {
            return Err(MessageError::UnknownPayload);
        }
        let mut bytes = vec![0; len];
        cursor
            .read_exact(&mut bytes)
            .map_err(|_| MessageError::UnknownPayload)?;
        Ok(bytes)
    }
    fn u32_value(cursor: &mut Cursor<&[u8]>) -> Result<u32, MessageError> {
        Ok(u32::from_le_bytes(take(cursor, 4)?.try_into().unwrap()))
    }
    fn u64_value(cursor: &mut Cursor<&[u8]>) -> Result<u64, MessageError> {
        Ok(u64::from_le_bytes(take(cursor, 8)?.try_into().unwrap()))
    }
    fn string(bytes: Vec<u8>) -> Result<String, MessageError> {
        String::from_utf8(bytes).map_err(|_| MessageError::UnknownPayload)
    }
    let (&kind, rest) = bytes.split_first().ok_or(MessageError::UnknownPayload)?;
    let mut cursor = Cursor::new(rest);
    let payload = match kind {
        0 if rest.is_empty() => EntryPayload::Blank,
        1 => {
            let op = take(&mut cursor, 1)?[0];
            let req = match op {
                0 => {
                    let len = u32_value(&mut cursor)? as usize;
                    let key = string(take(&mut cursor, len)?)?;
                    let remaining = rest.len() - cursor.position() as usize;
                    crate::types::ClientRequest::Set {
                        key,
                        value: take(&mut cursor, remaining)?,
                    }
                }
                1 => {
                    let remaining = rest.len() - cursor.position() as usize;
                    crate::types::ClientRequest::Delete {
                        key: string(take(&mut cursor, remaining)?)?,
                    }
                }
                2 => {
                    let count = u32_value(&mut cursor)? as usize;
                    if count > rest.len() / 8 {
                        return Err(MessageError::UnknownPayload);
                    }
                    let mut entries = Vec::new();
                    for _ in 0..count {
                        let len = u32_value(&mut cursor)? as usize;
                        let key = string(take(&mut cursor, len)?)?;
                        let len = u32_value(&mut cursor)? as usize;
                        entries.push((key, take(&mut cursor, len)?));
                    }
                    crate::types::ClientRequest::SetBatch { entries }
                }
                _ => return Err(MessageError::UnknownPayload),
            };
            EntryPayload::Normal(req)
        }
        // Read legacy persisted payloads; network peers must use protocol version 1.
        2 => {
            let count = u32_value(&mut cursor)? as usize;
            if count > rest.len() / 8 {
                return Err(MessageError::UnknownPayload);
            }
            let mut voters = BTreeSet::new();
            for _ in 0..count {
                voters.insert(u64_value(&mut cursor)?);
            }
            let count = u32_value(&mut cursor)? as usize;
            if count > rest.len() / 28 {
                return Err(MessageError::UnknownPayload);
            }
            let mut nodes = BTreeMap::new();
            for _ in 0..count {
                let id = u64_value(&mut cursor)?;
                let high = u64_value(&mut cursor)?;
                let low = u64_value(&mut cursor)?;
                let len = u32_value(&mut cursor)? as usize;
                let addr = string(take(&mut cursor, len)?)?;
                nodes.insert(
                    id,
                    ControlPlaneNode::new(addr, aaron_core::Uuid::new(high, low)),
                );
            }
            EntryPayload::Membership(openraft::Membership::new(vec![voters], nodes))
        }
        3 => {
            let membership = crate::storage::deserialize_stored_membership(rest)
                .ok_or(MessageError::UnknownPayload)?;
            return Ok(EntryPayload::Membership(membership.membership().clone()));
        }
        _ => return Err(MessageError::UnknownPayload),
    };
    if cursor.position() as usize != rest.len() {
        return Err(MessageError::UnknownPayload);
    }
    Ok(payload)
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
                let voted_for_proto = proto_node::Uuid {
                    high: voted_for_high,
                    low: 0,
                };

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
                    .map(|e| proto::LogEntry {
                        term: e.log_id.leader_id.term,
                        index: e.log_id.index,
                        entry_type: 0,
                        payload: Some(encode_payload(&e.payload)),
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
                    AppendEntriesResponse::HigherVote(v) => {
                        (v.leader_id().term, v.leader_id().voted_for().unwrap_or(0))
                    }
                    _ => (0, 0),
                };

                let voted_for_proto = proto_node::Uuid {
                    high: voted_for_high,
                    low: 0,
                };

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
                        last_included_term: req
                            .meta
                            .last_log_id
                            .map(|l| l.leader_id.term)
                            .unwrap_or(0),
                        last_included_index: req.meta.last_log_id.map(|l| l.index).unwrap_or(0),
                        offset: req.offset,
                        data: Some(req.data.clone()),
                        done: req.done,
                        meta: Some(Box::new(crate::storage::snapshot_meta_to_proto(&req.meta))),
                    },
                ))
            }
            Self::SnapshotResp(resp) => {
                let voted_for_high = resp.vote.leader_id().voted_for().unwrap_or(0);
                let voted_for_proto = proto_node::Uuid {
                    high: voted_for_high,
                    low: 0,
                };

                proto::ControlPlanePayload::InstallSnapshotResponse(Box::new(
                    proto::InstallSnapshotResponse {
                        term: resp.vote.leader_id().term,
                        success: true,
                        voted_for: Some(voted_for_proto),
                    },
                ))
            }
            Self::ShardCommand {
                shard_id,
                role,
                primary_high,
                primary_low,
                replicas,
                epoch,
                op_type,
                target_role,
            } => {
                let primary_proto = proto_node::Uuid {
                    high: *primary_high,
                    low: *primary_low,
                };
                let replicas_proto: Vec<_> = replicas
                    .iter()
                    .map(|(high, low)| proto_node::Uuid {
                        high: *high,
                        low: *low,
                    })
                    .collect();

                proto::ControlPlanePayload::ShardCommand(Box::new(proto::ShardCommand {
                    shard_id: *shard_id,
                    role: *role,
                    primary: Some(primary_proto),
                    replicas: Some(replicas_proto),
                    epoch: *epoch,
                    op_type: *op_type,
                    target_role: *target_role,
                }))
            }
            Self::ShardCommandResp {
                success,
                shard_id,
                current_role,
                term,
                reject_reason,
            } => proto::ControlPlanePayload::ShardCommandResponse(Box::new(
                proto::ShardCommandResponse {
                    success: *success,
                    shard_id: *shard_id,
                    current_role: *current_role,
                    term: *term,
                    reject_reason: *reject_reason,
                },
            )),
            Self::TelemetryHeartbeat {
                node_id_high,
                node_id_low,
                current_wps,
                error_rate,
                timestamp,
            } => {
                let node_proto = proto_node::Uuid {
                    high: *node_id_high,
                    low: *node_id_low,
                };
                proto::ControlPlanePayload::TelemetryHeartbeat(Box::new(
                    proto::TelemetryHeartbeat {
                        node_id: Some(node_proto),
                        current_wps: *current_wps,
                        error_rate: *error_rate,
                        timestamp: *timestamp,
                    },
                ))
            }
            Self::TelemetryHeartbeatResp { acknowledged } => {
                proto::ControlPlanePayload::TelemetryHeartbeatResponse(Box::new(
                    proto::TelemetryHeartbeatResponse {
                        acknowledged: *acknowledged,
                    },
                ))
            }
        };

        let msg = proto::ControlPlaneMessage {
            protocol_version: 1,
            payload: Some(payload),
        };
        let offset = msg.prepare(&mut builder);
        builder.finish(offset, None).to_vec()
    }

    /// Deserializes a FlatBuffers binary buffer into a strongly-typed `RaftMessage`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MessageError> {
        let msg_ref = proto::ControlPlaneMessageRef::read_as_root(bytes)?;
        if msg_ref.protocol_version()? != 1 {
            return Err(MessageError::UnknownPayload);
        }

        let payload_ref = msg_ref.payload()?.ok_or(MessageError::UnknownPayload)?;

        match payload_ref {
            proto::ControlPlanePayloadRef::VoteRequest(req) => {
                let term = req.term()?;
                let cand_ref = req.candidate_id()?.ok_or(MessageError::MissingField {
                    field: "candidate_id",
                })?;
                let cand_id = cand_ref.high();

                let last_log_term = req.last_log_term()?;
                let last_log_index = req.last_log_index()?;

                let last_log_id = if last_log_index > 0 {
                    Some(LogId::new(
                        CommittedLeaderId::new(last_log_term, cand_id),
                        last_log_index,
                    ))
                } else {
                    None
                };

                let vote = Vote::new(term, cand_id);
                Ok(Self::Vote(VoteRequest { vote, last_log_id }))
            }
            proto::ControlPlanePayloadRef::VoteResponse(resp) => {
                let term = resp.term()?;
                let vote_granted = resp.vote_granted()?;
                let last_log_term = resp.last_log_term()?;
                let last_log_index = resp.last_log_index()?;
                let voted_for = resp.voted_for()?.map(|v| v.high()).unwrap_or(0);

                let last_log_id = if last_log_index > 0 {
                    Some(LogId::new(
                        CommittedLeaderId::new(last_log_term, voted_for),
                        last_log_index,
                    ))
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
                    Some(LogId::new(
                        CommittedLeaderId::new(prev_log_term, leader_id),
                        prev_log_index,
                    ))
                } else {
                    None
                };

                let leader_commit = if leader_commit_idx > 0 {
                    Some(LogId::new(
                        CommittedLeaderId::new(term, leader_id),
                        leader_commit_idx,
                    ))
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

                        let payload = decode_payload(payload_bytes)?;

                        entries.push(Entry {
                            log_id: LogId::new(
                                CommittedLeaderId::new(entry_term, leader_id),
                                entry_idx,
                            ),
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

                let offset = req.offset()?;
                let data = req.data()?.unwrap_or_default().to_vec();
                let done = req.done()?;

                let meta = req
                    .meta()?
                    .ok_or(MessageError::MissingField { field: "meta" })?;
                let meta = crate::storage::snapshot_meta_from_proto(
                    proto::StoredSnapshotMeta::try_from(meta)?,
                )
                .ok_or(MessageError::UnknownPayload)?;

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
            proto::ControlPlanePayloadRef::ShardCommand(cmd) => {
                let shard_id = cmd.shard_id()?;
                let role = cmd.role()?;
                let p = cmd
                    .primary()?
                    .ok_or(MessageError::MissingField { field: "primary" })?;
                let primary_high = p.high();
                let primary_low = p.low();
                let mut replicas = Vec::new();
                if let Some(reps) = cmd.replicas()? {
                    for rep in reps {
                        replicas.push((rep.high(), rep.low()));
                    }
                }
                let epoch = cmd.epoch()?;
                let op_type = cmd.op_type().unwrap_or(0);
                let target_role = cmd.target_role().unwrap_or(0);
                Ok(Self::ShardCommand {
                    shard_id,
                    role,
                    primary_high,
                    primary_low,
                    replicas,
                    epoch,
                    op_type,
                    target_role,
                })
            }
            proto::ControlPlanePayloadRef::ShardCommandResponse(resp) => {
                let success = resp.success()?;
                let shard_id = resp.shard_id()?;
                let current_role = resp.current_role().unwrap_or(0);
                let term = resp.term().unwrap_or(0);
                let reject_reason = resp.reject_reason().unwrap_or(0);
                Ok(Self::ShardCommandResp {
                    success,
                    shard_id,
                    current_role,
                    term,
                    reject_reason,
                })
            }
            proto::ControlPlanePayloadRef::TelemetryHeartbeat(t) => {
                let node_ref = t
                    .node_id()?
                    .ok_or(MessageError::MissingField { field: "node_id" })?;
                Ok(Self::TelemetryHeartbeat {
                    node_id_high: node_ref.high(),
                    node_id_low: node_ref.low(),
                    current_wps: t.current_wps()?,
                    error_rate: t.error_rate()?,
                    timestamp: t.timestamp()?,
                })
            }
            proto::ControlPlanePayloadRef::TelemetryHeartbeatResponse(t) => {
                Ok(Self::TelemetryHeartbeatResp {
                    acknowledged: t.acknowledged()?,
                })
            }
            _ => Err(MessageError::UnknownPayload),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_heartbeat_roundtrip() {
        let original = RaftMessage::TelemetryHeartbeat {
            node_id_high: 0x1122334455667788,
            node_id_low: 0x99AABBCCDDEEFF00,
            current_wps: 450,
            error_rate: 3,
            timestamp: 1725321600,
        };

        let bytes = original.to_bytes();
        let decoded = RaftMessage::from_bytes(&bytes).expect("failed to decode TelemetryHeartbeat");

        match decoded {
            RaftMessage::TelemetryHeartbeat {
                node_id_high,
                node_id_low,
                current_wps,
                error_rate,
                timestamp,
            } => {
                assert_eq!(node_id_high, 0x1122334455667788);
                assert_eq!(node_id_low, 0x99AABBCCDDEEFF00);
                assert_eq!(current_wps, 450);
                assert_eq!(error_rate, 3);
                assert_eq!(timestamp, 1725321600);
            }
            _ => panic!("Decoded wrong message variant"),
        }
    }
}
