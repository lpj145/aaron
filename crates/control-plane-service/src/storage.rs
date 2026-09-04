use crate::message::decode_payload;
use crate::proto::aaron::control_plane as cp_proto;
use crate::proto::aaron::node as node_proto;
use crate::types::{
    ClientRequest, ClientResponse, ControlPlaneNode, Entry, LogId, Snapshot, SnapshotMeta,
    StoredMembership, TypeConfig, Vote,
};
use node::{Context, Keyspace, KeyspaceExt};
use openraft::storage::{LogState, RaftLogReader, RaftSnapshotBuilder, RaftStorage};
use openraft::{CommittedLeaderId, EntryPayload, OptionalSend, StorageError, StorageIOError};
use planus::{ReadAsRoot, WriteAsOffset};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Storage engine for OpenRaft built on top of `Fjall` LSM Store (`"control-plane"` keyspace).
#[derive(Clone)]
pub struct ControlPlaneStorage {
    ctx: Context,
    keyspace_name: String,
    vote: Arc<RwLock<Option<Vote>>>,
    log: Arc<RwLock<BTreeMap<u64, Entry>>>,
    data: Arc<RwLock<BTreeMap<String, Vec<u8>>>>,
    last_applied: Arc<RwLock<Option<LogId>>>,
    last_membership: Arc<RwLock<StoredMembership>>,
    last_purged_log_id: Arc<RwLock<Option<LogId>>>,
    current_snapshot: Arc<RwLock<Option<Snapshot>>>,
}

// ----------------------------------------------------------------------------
// FlatBuffers Schema-Driven On-Disk Serialization & Deserialization
// ----------------------------------------------------------------------------

pub fn serialize_stored_vote(vote: &Vote) -> Vec<u8> {
    let mut builder = planus::Builder::new();
    let stored = cp_proto::StoredVote {
        term: vote.leader_id().term,
        node_id: vote.leader_id().voted_for().unwrap_or(0),
        is_committed: vote.is_committed(),
    };
    let offset = stored.prepare(&mut builder);
    builder.finish(offset, None).to_vec()
}

pub fn deserialize_stored_vote(bytes: &[u8]) -> Option<Vote> {
    if let Ok(vote_ref) = cp_proto::StoredVoteRef::read_as_root(bytes)
        && let Ok(stored) = cp_proto::StoredVote::try_from(vote_ref) {
            let mut v = Vote::new(stored.term, stored.node_id);
            if stored.is_committed {
                v = Vote::new_committed(stored.term, stored.node_id);
            }
            return Some(v);
        }
    // Backward-compatible fallback for legacy raw bytes
    if bytes.len() >= 16 {
        let term = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
        let node_id = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
        let is_committed = if bytes.len() >= 17 { bytes[16] == 1 } else { false };
        let mut v = Vote::new(term, node_id);
        if is_committed {
            v = Vote::new_committed(term, node_id);
        }
        return Some(v);
    }
    None
}

pub fn serialize_stored_log_id(log_id: &LogId) -> Vec<u8> {
    let mut builder = planus::Builder::new();
    let stored = cp_proto::StoredLogId {
        term: log_id.leader_id.term,
        index: log_id.index,
    };
    let offset = stored.prepare(&mut builder);
    builder.finish(offset, None).to_vec()
}

pub fn deserialize_stored_log_id(bytes: &[u8]) -> Option<LogId> {
    if let Ok(id_ref) = cp_proto::StoredLogIdRef::read_as_root(bytes)
        && let Ok(stored) = cp_proto::StoredLogId::try_from(id_ref) {
            return Some(LogId::new(CommittedLeaderId::new(stored.term, 0u64), stored.index));
        }
    // Backward-compatible fallback for legacy raw bytes
    if bytes.len() >= 16 {
        let term = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
        let index = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
        return Some(LogId::new(CommittedLeaderId::new(term, 0u64), index));
    }
    None
}

pub fn serialize_stored_membership(sm: &StoredMembership) -> Vec<u8> {
    let mut builder = planus::Builder::new();
    let log_id_proto = sm.log_id().map(|l| Box::new(cp_proto::StoredLogId {
        term: l.leader_id.term,
        index: l.index,
    }));
    let voter_ids: Vec<u64> = sm.membership().voter_ids().collect();
    let nodes: Vec<cp_proto::NodeEndpoint> = sm
        .membership()
        .nodes()
        .map(|(_nid, n)| cp_proto::NodeEndpoint {
            uuid: Some(node_proto::Uuid {
                high: n.node_uuid_high,
                low: n.node_uuid_low,
            }),
            addr: Some(n.addr.clone()),
        })
        .collect();

    let stored = cp_proto::StoredMembership {
        log_id: log_id_proto,
        voter_ids: Some(voter_ids),
        nodes: Some(nodes),
    };
    let offset = stored.prepare(&mut builder);
    builder.finish(offset, None).to_vec()
}

pub fn deserialize_stored_membership(bytes: &[u8]) -> Option<StoredMembership> {
    if let Ok(sm_ref) = cp_proto::StoredMembershipRef::read_as_root(bytes)
        && let Ok(stored) = cp_proto::StoredMembership::try_from(sm_ref) {
            let log_id = stored.log_id.map(|lid| {
                LogId::new(CommittedLeaderId::new(lid.term, 0u64), lid.index)
            });

            let mut voters_set = std::collections::BTreeSet::new();
            if let Some(voters) = stored.voter_ids {
                for v in voters {
                    voters_set.insert(v);
                }
            }

            let mut nodes_map = std::collections::BTreeMap::new();
            if let Some(nodes) = stored.nodes {
                for n in nodes {
                    let uuid = n.uuid.map(|u| node::Uuid::new(u.high, u.low)).unwrap_or(node::Uuid::NIL);
                    let addr = n.addr.unwrap_or_default();
                    let node_id_u64 = uuid.low;
                    let cp_node = ControlPlaneNode::new(addr, uuid);
                    nodes_map.insert(node_id_u64, cp_node);
                }
            }

            let membership = openraft::Membership::new(vec![voters_set], nodes_map);
            return Some(StoredMembership::new(log_id, membership));
        }
    // Backward-compatible fallback for legacy raw bytes
    if bytes.len() >= 20 {
        let term = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
        let index = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
        let len = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;
        if bytes.len() >= 20 + len
            && let EntryPayload::Membership(mem) = decode_payload(&bytes[20..20 + len]) {
                return Some(StoredMembership::new(
                    Some(LogId::new(CommittedLeaderId::new(term, 0u64), index)),
                    mem,
                ));
            }
    }
    None
}

pub fn serialize_stored_log_entry(entry: &Entry) -> Vec<u8> {
    let mut builder = planus::Builder::new();
    let term = entry.log_id.leader_id.term;
    let index = entry.log_id.index;

    let (entry_type, normal_op, normal_key, normal_value, membership_proto) = match &entry.payload {
        EntryPayload::Blank => (0u8, 0u8, None, None, None),
        EntryPayload::Normal(req) => match req {
            ClientRequest::Set { key, value } => (1u8, 0u8, Some(key.clone()), Some(value.clone()), None),
            ClientRequest::Delete { key } => (1u8, 1u8, Some(key.clone()), None, None),
            ClientRequest::SetBatch { entries } => {
                let mut buf = Vec::new();
                buf.extend_from_slice(&(entries.len() as u32).to_le_bytes());
                for (k, v) in entries {
                    buf.extend_from_slice(&(k.len() as u32).to_le_bytes());
                    buf.extend_from_slice(k.as_bytes());
                    buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
                    buf.extend_from_slice(v);
                }
                (1u8, 2u8, None, Some(buf), None)
            }
        },
        EntryPayload::Membership(mem) => {
            let log_id_proto = Some(Box::new(cp_proto::StoredLogId { term, index }));
            let voter_ids: Vec<u64> = mem.voter_ids().collect();
            let nodes: Vec<cp_proto::NodeEndpoint> = mem
                .nodes()
                .map(|(_nid, n)| cp_proto::NodeEndpoint {
                    uuid: Some(node_proto::Uuid {
                        high: n.node_uuid_high,
                        low: n.node_uuid_low,
                    }),
                    addr: Some(n.addr.clone()),
                })
                .collect();
            let sm = cp_proto::StoredMembership {
                log_id: log_id_proto,
                voter_ids: Some(voter_ids),
                nodes: Some(nodes),
            };
            (2u8, 0u8, None, None, Some(Box::new(sm)))
        }
    };

    let stored = cp_proto::StoredLogEntry {
        term,
        index,
        entry_type,
        normal_op,
        normal_key,
        normal_value,
        membership: membership_proto,
    };
    let offset = stored.prepare(&mut builder);
    builder.finish(offset, None).to_vec()
}

pub fn deserialize_stored_log_entry(bytes: &[u8]) -> Option<Entry> {
    if let Ok(entry_ref) = cp_proto::StoredLogEntryRef::read_as_root(bytes)
        && let Ok(stored) = cp_proto::StoredLogEntry::try_from(entry_ref) {
            let payload = match stored.entry_type {
                0 => EntryPayload::Blank,
                1 => match stored.normal_op {
                    0 => {
                        let key = stored.normal_key.unwrap_or_default();
                        let value = stored.normal_value.unwrap_or_default();
                        EntryPayload::Normal(ClientRequest::Set { key, value })
                    }
                    1 => {
                        let key = stored.normal_key.unwrap_or_default();
                        EntryPayload::Normal(ClientRequest::Delete { key })
                    }
                    2 => {
                        let val_bytes = stored.normal_value.unwrap_or_default();
                        let mut entries = Vec::new();
                        if val_bytes.len() >= 4 {
                            let count = u32::from_le_bytes(val_bytes[0..4].try_into().unwrap()) as usize;
                            let mut cursor = 4;
                            for _ in 0..count {
                                if cursor + 4 > val_bytes.len() { break; }
                                let k_len = u32::from_le_bytes(val_bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
                                cursor += 4;
                                if cursor + k_len > val_bytes.len() { break; }
                                let key = String::from_utf8_lossy(&val_bytes[cursor..cursor + k_len]).to_string();
                                cursor += k_len;

                                if cursor + 4 > val_bytes.len() { break; }
                                let v_len = u32::from_le_bytes(val_bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
                                cursor += 4;
                                if cursor + v_len > val_bytes.len() { break; }
                                let value = val_bytes[cursor..cursor + v_len].to_vec();
                                cursor += v_len;

                                entries.push((key, value));
                            }
                        }
                        EntryPayload::Normal(ClientRequest::SetBatch { entries })
                    }
                    _ => EntryPayload::Blank,
                },
                2 => {
                    if let Some(sm_proto) = stored.membership {
                        let mut voters_set = std::collections::BTreeSet::new();
                        if let Some(voters) = sm_proto.voter_ids {
                            for v in voters {
                                voters_set.insert(v);
                            }
                        }

                        let mut nodes_map = std::collections::BTreeMap::new();
                        if let Some(nodes) = sm_proto.nodes {
                            for n in nodes {
                                let uuid = n.uuid.map(|u| node::Uuid::new(u.high, u.low)).unwrap_or(node::Uuid::NIL);
                                let addr = n.addr.unwrap_or_default();
                                let node_id_u64 = uuid.low;
                                let cp_node = ControlPlaneNode::new(addr, uuid);
                                nodes_map.insert(node_id_u64, cp_node);
                            }
                        }
                        let membership = openraft::Membership::new(vec![voters_set], nodes_map);
                        EntryPayload::Membership(membership)
                    } else {
                        EntryPayload::Blank
                    }
                }
                _ => EntryPayload::Blank,
            };

            return Some(Entry {
                log_id: LogId::new(CommittedLeaderId::new(stored.term, 0u64), stored.index),
                payload,
            });
        }

    // Backward-compatible fallback for legacy raw bytes
    if bytes.len() >= 20 {
        let term = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
        let index = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
        let payload_len = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;
        if bytes.len() >= 20 + payload_len {
            let payload = decode_payload(&bytes[20..20 + payload_len]);
            return Some(Entry {
                log_id: LogId::new(CommittedLeaderId::new(term, 0u64), index),
                payload,
            });
        }
    }
    None
}

const SNAPSHOT_MAGIC: &[u8; 4] = b"AAR1";

pub fn encode_snapshot_data(data: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(SNAPSHOT_MAGIC);
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    for (k, v) in data {
        buf.extend_from_slice(&(k.len() as u32).to_le_bytes());
        buf.extend_from_slice(k.as_bytes());
        buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
        buf.extend_from_slice(v);
    }
    buf
}

/// Borrowed zero-copy entry from a serialized snapshot payload.
pub struct SnapshotStreamEntry<'a> {
    pub key: &'a str,
    pub val: &'a [u8],
}

/// Zero-copy streaming iterator over snapshot key-value pairs.
pub struct SnapshotZeroCopyIterator<'a> {
    bytes: &'a [u8],
    idx: usize,
    remaining: usize,
}

impl<'a> SnapshotZeroCopyIterator<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, String> {
        if bytes.is_empty() {
            return Ok(Self {
                bytes,
                idx: 0,
                remaining: 0,
            });
        }
        if bytes.starts_with(SNAPSHOT_MAGIC) {
            let mut idx = 4;
            if bytes.len() < idx + 4 {
                return Err("corrupted snapshot: truncated header".to_string());
            }
            let count = u32::from_le_bytes(bytes[idx..idx + 4].try_into().unwrap()) as usize;
            idx += 4;
            return Ok(Self {
                bytes,
                idx,
                remaining: count,
            });
        }
        Err("non-binary or legacy snapshot".to_string())
    }

    pub fn remaining_count(&self) -> usize {
        self.remaining
    }
}

impl<'a> Iterator for SnapshotZeroCopyIterator<'a> {
    type Item = Result<SnapshotStreamEntry<'a>, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        if self.bytes.len() < self.idx + 4 {
            return Some(Err("corrupted snapshot: truncated key length".to_string()));
        }
        let k_len = u32::from_le_bytes(self.bytes[self.idx..self.idx + 4].try_into().unwrap()) as usize;
        self.idx += 4;
        if self.bytes.len() < self.idx + k_len {
            return Some(Err("corrupted snapshot: truncated key bytes".to_string()));
        }
        let key = match std::str::from_utf8(&self.bytes[self.idx..self.idx + k_len]) {
            Ok(k) => k,
            Err(e) => return Some(Err(format!("invalid UTF-8 in snapshot key: {e}"))),
        };
        self.idx += k_len;

        if self.bytes.len() < self.idx + 4 {
            return Some(Err("corrupted snapshot: truncated value length".to_string()));
        }
        let v_len = u32::from_le_bytes(self.bytes[self.idx..self.idx + 4].try_into().unwrap()) as usize;
        self.idx += 4;
        if self.bytes.len() < self.idx + v_len {
            return Some(Err("corrupted snapshot: truncated value bytes".to_string()));
        }
        let val = &self.bytes[self.idx..self.idx + v_len];
        self.idx += v_len;
        self.remaining -= 1;

        Some(Ok(SnapshotStreamEntry { key, val }))
    }
}

pub fn decode_snapshot_data(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    if bytes.is_empty() {
        return Ok(BTreeMap::new());
    }
    if let Ok(iter) = SnapshotZeroCopyIterator::new(bytes) {
        let mut map = BTreeMap::new();
        for item in iter {
            let entry = item?;
            map.insert(entry.key.to_string(), entry.val.to_vec());
        }
        return Ok(map);
    }

    // Backward-compatible fallback for legacy JSON snapshots
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

impl ControlPlaneStorage {
    /// Creates a new `ControlPlaneStorage` on the specified keyspace and restores existing state.
    pub async fn new(ctx: Context, keyspace_name: &str) -> Result<Self, node::Error> {
        let store = Self {
            ctx,
            keyspace_name: keyspace_name.to_string(),
            vote: Arc::new(RwLock::new(None)),
            log: Arc::new(RwLock::new(BTreeMap::new())),
            data: Arc::new(RwLock::new(BTreeMap::new())),
            last_applied: Arc::new(RwLock::new(None)),
            last_membership: Arc::new(RwLock::new(StoredMembership::default())),
            last_purged_log_id: Arc::new(RwLock::new(None)),
            current_snapshot: Arc::new(RwLock::new(None)),
        };

        store.load_from_store().await?;
        Ok(store)
    }

    async fn get_keyspace(&self) -> Result<Keyspace, StorageError<u64>> {
        self.ctx
            .store
            .keyspace(&self.keyspace_name)
            .map_err(|e: node::BoxError| StorageIOError::read_state_machine(openraft::AnyError::error(e.to_string())).into())
    }

    /// Loads persisted state machine entries, logs, and vote from the LSM keyspace.
    async fn load_from_store(&self) -> Result<(), node::Error> {
        let ks: Keyspace = self
            .ctx
            .store
            .keyspace(&self.keyspace_name)
            .map_err(|e: node::BoxError| node::Error::new(node::ErrorKind::Internal, e.to_string()))?;

        // 1. Load vote
        if let Ok(Some(vote_bytes)) = ks.get(b"meta/vote")
            && let Some(v) = deserialize_stored_vote(&vote_bytes) {
                *self.vote.write().await = Some(v);
            }

        // 2. Load last purged log id
        if let Ok(Some(purged_bytes)) = ks.get(b"meta/last_purged")
            && let Some(log_id) = deserialize_stored_log_id(&purged_bytes) {
                *self.last_purged_log_id.write().await = Some(log_id);
            }

        // 3. Load log entries with full pagination
        let mut log_guard = self.log.write().await;
        let mut log_cursor = None;
        loop {
            let log_page = ks
                .scan_prefix(b"log/", log_cursor.as_deref(), 10_000)
                .map_err(|e: node::BoxError| node::Error::new(node::ErrorKind::Internal, e.to_string()))?;
            for item in log_page.items {
                if let Some(entry) = deserialize_stored_log_entry(&item.value) {
                    log_guard.insert(entry.log_id.index, entry);
                }
            }
            if !log_page.has_more {
                break;
            }
            log_cursor = log_page.next_cursor;
        }

        // 4. Load state machine data with full pagination
        let mut data_guard = self.data.write().await;
        let mut data_cursor = None;
        loop {
            let page = ks
                .scan_prefix(b"data/", data_cursor.as_deref(), 5_000)
                .map_err(|e: node::BoxError| node::Error::new(node::ErrorKind::Internal, e.to_string()))?;
            for item in page.items {
                if let Some(k_str) = item.key_str() {
                    let key = k_str.trim_start_matches("data/").to_string();
                    data_guard.insert(key, item.value.to_vec());
                }
            }
            if !page.has_more {
                break;
            }
            data_cursor = page.next_cursor;
        }

        // 5. Load last applied log id
        if let Ok(Some(applied_bytes)) = ks.get(b"meta/last_applied")
            && let Some(log_id) = deserialize_stored_log_id(&applied_bytes) {
                *self.last_applied.write().await = Some(log_id);
            }

        // 6. Load last membership
        if let Ok(Some(sm_bytes)) = ks.get(b"meta/last_membership")
            && let Some(sm) = deserialize_stored_membership(&sm_bytes) {
                *self.last_membership.write().await = sm;
            }

        // Fallback: If last_membership is still empty, scan log backwards to recover membership
        if self.last_membership.read().await.membership().nodes().next().is_none() {
            for (_, entry) in log_guard.iter().rev() {
                if let EntryPayload::Membership(ref mem) = entry.payload {
                    *self.last_membership.write().await = StoredMembership::new(Some(entry.log_id), mem.clone());
                    break;
                }
            }
        }

        // Fallback: If last_applied is still None, recover from highest log entry
        if self.last_applied.read().await.is_none()
            && let Some((_, last_entry)) = log_guard.iter().next_back() {
                *self.last_applied.write().await = Some(last_entry.log_id);
            }

        Ok(())
    }

    /// Exposes read access to the in-memory replicated state machine.
    pub async fn get_data(&self, key: &str) -> Option<Vec<u8>> {
        self.data.read().await.get(key).cloned()
    }

    /// Returns a snapshot map of all keys and values in the replicated state machine.
    pub async fn all_data(&self) -> BTreeMap<String, Vec<u8>> {
        self.data.read().await.clone()
    }

    /// Returns a map of key-value entries in the replicated state machine matching a given prefix.
    pub async fn prefix_data(&self, prefix: &str) -> BTreeMap<String, Vec<u8>> {
        let data = self.data.read().await;
        data.range(prefix.to_string()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Returns all state machine keys and string values (or lossy utf-8 conversion).
    pub async fn all_data_strings(&self) -> BTreeMap<String, String> {
        self.data
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), String::from_utf8_lossy(v).to_string()))
            .collect()
    }
}

// ----------------------------------------------------------------------------
// RaftLogReader Implementation
// ----------------------------------------------------------------------------
impl RaftLogReader<TypeConfig> for ControlPlaneStorage {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry>, StorageError<u64>> {
        let log = self.log.read().await;
        let entries = log.range(range).map(|(_, v)| v.clone()).collect();
        Ok(entries)
    }
}

// ----------------------------------------------------------------------------
// RaftStorage Implementation
// ----------------------------------------------------------------------------
impl RaftStorage<TypeConfig> for ControlPlaneStorage {
    type LogReader = Self;
    type SnapshotBuilder = Self;

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<u64>> {
        let log = self.log.read().await;
        let last_purged_log_id = *self.last_purged_log_id.read().await;

        let last_log_id = match log.values().last() {
            Some(e) => Some(e.log_id),
            None => last_purged_log_id,
        };

        Ok(LogState {
            last_purged_log_id,
            last_log_id,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(&mut self, vote: &Vote) -> Result<(), StorageError<u64>> {
        *self.vote.write().await = Some(*vote);

        // Persist vote to LSM keyspace
        if let Ok(ks) = self.get_keyspace().await {
            let bytes = serialize_stored_vote(vote);
            let _ = ks.insert(b"meta/vote", bytes.as_slice());
            let _ = self.ctx.store.persist();
        }

        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote>, StorageError<u64>> {
        Ok(*self.vote.read().await)
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<u64>>
    where
        I: IntoIterator<Item = Entry> + OptionalSend,
    {
        let mut log = self.log.write().await;
        let ks: Keyspace = self.get_keyspace().await?;
        let mut batch = self.ctx.store.batch();

        for entry in entries {
            let db_key = format!("log/{:020}", entry.log_id.index);
            let db_val = serialize_stored_log_entry(&entry);
            batch.insert(&ks, db_key.as_bytes(), db_val.as_slice());
            log.insert(entry.log_id.index, entry);
        }

        let _ = batch.commit();
        let _ = self.ctx.store.persist();
        Ok(())
    }

    async fn delete_conflict_logs_since(&mut self, log_id: LogId) -> Result<(), StorageError<u64>> {
        let mut log = self.log.write().await;
        let ks: Keyspace = self.get_keyspace().await?;
        let mut batch = self.ctx.store.batch();

        let keys_to_remove: Vec<u64> = log.range(log_id.index..).map(|(k, _)| *k).collect();
        for k in keys_to_remove {
            let db_key = format!("log/{:020}", k);
            batch.remove(&ks, db_key.as_bytes());
            log.remove(&k);
        }

        let _ = batch.commit();
        let _ = self.ctx.store.persist();
        Ok(())
    }

    async fn purge_logs_upto(&mut self, log_id: LogId) -> Result<(), StorageError<u64>> {
        let mut log = self.log.write().await;
        let ks: Keyspace = self.get_keyspace().await?;

        *self.last_purged_log_id.write().await = Some(log_id);

        let purged_bytes = serialize_stored_log_id(&log_id);
        let mut batch = self.ctx.store.batch();
        batch.insert(&ks, b"meta/last_purged", purged_bytes.as_slice());

        let keys_to_remove: Vec<u64> = log.range(..=log_id.index).map(|(k, _)| *k).collect();
        for k in keys_to_remove {
            let db_key = format!("log/{:020}", k);
            batch.remove(&ks, db_key.as_bytes());
            log.remove(&k);
        }

        let _ = batch.commit();
        let _ = self.ctx.store.persist();
        Ok(())
    }

    async fn last_applied_state(
        &mut self,
    ) -> Result<(Option<LogId>, StoredMembership), StorageError<u64>> {
        let last_applied = *self.last_applied.read().await;
        let last_membership = self.last_membership.read().await.clone();
        Ok((last_applied, last_membership))
    }

    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry],
    ) -> Result<Vec<ClientResponse>, StorageError<u64>> {
        let mut res = Vec::new();
        let mut data = self.data.write().await;
        let ks: Keyspace = self.get_keyspace().await?;
        let mut batch = self.ctx.store.batch();

        for entry in entries {
            *self.last_applied.write().await = Some(entry.log_id);

            // Persist last_applied metadata with FlatBuffers
            let applied_bytes = serialize_stored_log_id(&entry.log_id);
            batch.insert(&ks, b"meta/last_applied", applied_bytes.as_slice());

            match entry.payload {
                EntryPayload::Blank => {
                    res.push(ClientResponse {
                        success: true,
                        value: None,
                    });
                }
                EntryPayload::Normal(ref req) => match req {
                    ClientRequest::Set { key, value } => {
                        data.insert(key.clone(), value.clone());
                        let db_key = format!("data/{key}");
                        batch.insert(&ks, db_key.as_bytes(), value.as_slice());
                        res.push(ClientResponse {
                            success: true,
                            value: Some(value.clone()),
                        });
                    }
                    ClientRequest::Delete { key } => {
                        let prev = data.remove(key);
                        let db_key = format!("data/{key}");
                        batch.remove(&ks, db_key.as_bytes());
                        res.push(ClientResponse {
                            success: true,
                            value: prev,
                        });
                    }
                    ClientRequest::SetBatch { entries } => {
                        for (k, v) in entries {
                            data.insert(k.clone(), v.clone());
                            let db_key = format!("data/{k}");
                            batch.insert(&ks, db_key.as_bytes(), v.as_slice());
                        }
                        res.push(ClientResponse {
                            success: true,
                            value: None,
                        });
                    }
                },
                EntryPayload::Membership(ref mem) => {
                    let sm = StoredMembership::new(Some(entry.log_id), mem.clone());
                    *self.last_membership.write().await = sm.clone();

                    // Persist last_membership metadata to disk with FlatBuffers
                    let sm_bytes = serialize_stored_membership(&sm);
                    batch.insert(&ks, b"meta/last_membership", sm_bytes.as_slice());

                    res.push(ClientResponse {
                        success: true,
                        value: None,
                    });
                }
            }
        }

        let _ = batch.commit();
        let _ = self.ctx.store.persist();
        Ok(res)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<Cursor<Vec<u8>>>, StorageError<u64>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta,
        snapshot_data: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<u64>> {
        let data_bytes = snapshot_data.get_ref();

        // 1. Update in-memory metadata
        *self.last_applied.write().await = meta.last_log_id;
        *self.last_membership.write().await = meta.last_membership.clone();

        const SNAPSHOT_BATCH_LIMIT: usize = 1_000;
        let mut in_memory_data = BTreeMap::new();

        // 2. Persist state machine to LSM keyspace using bounded, chunked WriteBatches
        if let Ok(ks) = self.get_keyspace().await {
            // A. Remove existing data/ keys in bounded chunks
            let mut cursor = None;
            loop {
                let existing_page = ks.scan_prefix(b"data/", cursor.as_deref(), SNAPSHOT_BATCH_LIMIT).map_err(|e| {
                    StorageIOError::write_snapshot(Some(meta.signature()), openraft::AnyError::error(e.to_string()))
                })?;

                if existing_page.items.is_empty() {
                    break;
                }

                let mut remove_batch = self.ctx.store.batch();
                for item in &existing_page.items {
                    remove_batch.remove(&ks, &*item.key);
                }
                let _ = remove_batch.commit();

                if !existing_page.has_more {
                    break;
                }
                cursor = existing_page.next_cursor;
            }

            // B. Stream snapshot entries directly into LSM using chunked commits
            let mut insert_batch = self.ctx.store.batch();
            let mut uncommitted = 0;

            if let Ok(iter) = SnapshotZeroCopyIterator::new(data_bytes) {
                for item in iter {
                    let entry = item.map_err(|e| {
                        StorageIOError::write_snapshot(Some(meta.signature()), openraft::AnyError::error(e))
                    })?;

                    let db_key = format!("data/{}", entry.key);
                    insert_batch.insert(&ks, db_key.as_bytes(), entry.val);
                    in_memory_data.insert(entry.key.to_string(), entry.val.to_vec());
                    uncommitted += 1;

                    if uncommitted >= SNAPSHOT_BATCH_LIMIT {
                        let _ = insert_batch.commit();
                        insert_batch = self.ctx.store.batch();
                        uncommitted = 0;
                    }
                }
            } else {
                // Backward-compatible fallback for legacy snapshots
                let legacy_data: BTreeMap<String, Vec<u8>> = serde_json::from_slice(data_bytes).map_err(|e| {
                    StorageIOError::write_snapshot(Some(meta.signature()), openraft::AnyError::error(e))
                })?;
                for (key, val) in &legacy_data {
                    let db_key = format!("data/{key}");
                    insert_batch.insert(&ks, db_key.as_bytes(), val.as_slice());
                    in_memory_data.insert(key.clone(), val.clone());
                    uncommitted += 1;

                    if uncommitted >= SNAPSHOT_BATCH_LIMIT {
                        let _ = insert_batch.commit();
                        insert_batch = self.ctx.store.batch();
                        uncommitted = 0;
                    }
                }
            }

            // Commit final uncommitted items
            if uncommitted > 0 {
                let _ = insert_batch.commit();
            }

            // C. Persist last purged metadata
            if let Some(log_id) = meta.last_log_id {
                let mut purged_batch = self.ctx.store.batch();
                let purged_bytes = serialize_stored_log_id(&log_id);
                purged_batch.insert(&ks, b"meta/last_purged", purged_bytes.as_slice());
                let _ = purged_batch.commit();
            }

            let _ = self.ctx.store.persist();
        }

        // 3. Update in-memory state cache
        *self.data.write().await = in_memory_data;

        let snapshot = openraft::Snapshot {
            meta: meta.clone(),
            snapshot: snapshot_data,
        };
        *self.current_snapshot.write().await = Some(snapshot);
        Ok(())
    }

    async fn get_current_snapshot(&mut self) -> Result<Option<Snapshot>, StorageError<u64>> {
        Ok(self.current_snapshot.read().await.clone())
    }
}

// RaftSnapshotBuilder implementation
impl RaftSnapshotBuilder<TypeConfig> for ControlPlaneStorage {
    async fn build_snapshot(&mut self) -> Result<Snapshot, StorageError<u64>> {
        let last_applied = *self.last_applied.read().await;
        let last_membership = self.last_membership.read().await.clone();
        let data = self.data.read().await.clone();

        let data_bytes = encode_snapshot_data(&data);

        let meta = SnapshotMeta {
            last_log_id: last_applied,
            last_membership,
            snapshot_id: last_applied.map(|l: LogId| l.to_string()).unwrap_or_default(),
        };

        let snapshot = openraft::Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(data_bytes)),
        };

        *self.current_snapshot.write().await = Some(snapshot.clone());

        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ControlPlaneNode;
    use node::{Context, Env, EventHub, Network, NodeId, Store, Uuid};
    use openraft::Membership;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_storage_restart_recovery_of_membership_and_applied() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let tmp = tempdir().map_err(|e| e.to_string())?;
        let store = Store::open(&tmp).map_err(|e| e.to_string())?;
        let ctx = Context::new(
            EventHub::new(),
            Network::new(),
            store.clone(),
            NodeId::new(Uuid::random(), 1, None),
            Arc::new(Env::detect()),
            CancellationToken::new(),
        );

        // 1. Initial boot: write membership and normal log entries
        let mut storage = ControlPlaneStorage::new(ctx.clone(), "control-plane").await?;
        let node_id = 100u64;
        let mut nodes = BTreeMap::new();
        nodes.insert(node_id, ControlPlaneNode::new("10.0.0.1:18946", Uuid::random()));
        let membership = Membership::new(vec![std::collections::BTreeSet::from([node_id])], nodes);

        let mem_entry = Entry {
            log_id: LogId::new(CommittedLeaderId::new(1, 0), 1),
            payload: EntryPayload::Membership(membership.clone()),
        };
        let normal_entry = Entry {
            log_id: LogId::new(CommittedLeaderId::new(1, 0), 2),
            payload: EntryPayload::Normal(ClientRequest::Set {
                key: "cluster/status".to_string(),
                value: b"active".to_vec(),
            }),
        };

        storage.append_to_log(vec![mem_entry.clone(), normal_entry.clone()]).await?;
        storage.apply_to_state_machine(&[mem_entry, normal_entry]).await?;

        let (applied, mem) = storage.last_applied_state().await?;
        assert_eq!(applied.unwrap().index, 2);
        assert_eq!(mem.membership().voter_ids().collect::<Vec<_>>(), vec![100]);

        // 2. Simulate Node Crash / Restart (Create new storage instance from same underlying store)
        let mut recovered_storage = ControlPlaneStorage::new(ctx, "control-plane").await?;
        let (rec_applied, rec_mem) = recovered_storage.last_applied_state().await?;

        assert_eq!(rec_applied.unwrap().index, 2, "last_applied must be preserved across restarts");
        assert_eq!(
            rec_mem.membership().voter_ids().collect::<Vec<_>>(),
            vec![100],
            "membership must be preserved across restarts"
        );
        assert_eq!(recovered_storage.get_data("cluster/status").await, Some(b"active".to_vec()));

        Ok(())
    }

    #[tokio::test]
    async fn test_snapshot_streaming_and_chunked_install() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let tmp = tempdir().map_err(|e| e.to_string())?;
        let store = Store::open(&tmp).map_err(|e| e.to_string())?;
        let ctx = Context::new(
            EventHub::new(),
            Network::new(),
            store.clone(),
            NodeId::new(Uuid::random(), 1, None),
            Arc::new(Env::detect()),
            CancellationToken::new(),
        );

        let mut storage = ControlPlaneStorage::new(ctx.clone(), "control-plane").await?;

        // 1. Prepare 2,500 entries (exceeds SNAPSHOT_BATCH_LIMIT of 1,000)
        let mut test_data = BTreeMap::new();
        for i in 0..2_500 {
            test_data.insert(format!("shard_metric_{i:04}"), format!("wps_score_{i}").into_bytes());
        }

        let encoded_bytes = encode_snapshot_data(&test_data);

        // Verify zero-copy iterator
        let iter = SnapshotZeroCopyIterator::new(&encoded_bytes).map_err(|e| e.to_string())?;
        assert_eq!(iter.remaining_count(), 2_500);

        let mut verified_count = 0;
        for item in iter {
            let entry = item.map_err(|e| e.to_string())?;
            assert!(entry.key.starts_with("shard_metric_"));
            verified_count += 1;
        }
        assert_eq!(verified_count, 2_500);

        // 2. Install snapshot into storage
        let log_id = LogId::new(CommittedLeaderId::new(2, 0), 250);
        let meta = SnapshotMeta {
            last_log_id: Some(log_id),
            last_membership: StoredMembership::new(Some(log_id), Membership::new(vec![std::collections::BTreeSet::from([1])], BTreeMap::new())),
            snapshot_id: "snap-2500".to_string(),
        };

        storage.install_snapshot(&meta, Box::new(Cursor::new(encoded_bytes))).await?;

        // 3. Verify in-memory cache and persisted keys in Fjall
        assert_eq!(storage.get_data("shard_metric_0000").await, Some(b"wps_score_0".to_vec()));
        assert_eq!(storage.get_data("shard_metric_2499").await, Some(b"wps_score_2499".to_vec()));
        let (applied, _) = storage.last_applied_state().await?;
        assert_eq!(applied.unwrap().index, 250);

        Ok(())
    }
}
