use crate::message::decode_payload;
use crate::proto::aaron::control_plane as cp_proto;
use crate::proto::aaron::node as node_proto;
use crate::types::{
    ClientRequest, ClientResponse, ControlPlaneNode, Entry, LogId, Snapshot, SnapshotMeta,
    StoredMembership, TypeConfig, Vote,
};
use aaron_core::{Context, Keyspace, KeyspaceExt};
use openraft::storage::{LogState, RaftLogReader, RaftSnapshotBuilder, RaftStorage};
use openraft::{CommittedLeaderId, EntryPayload, OptionalSend, StorageError, StorageIOError};
use planus::{ReadAsRoot, WriteAsOffset};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

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
    state_change: Arc<Mutex<()>>,
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
        && let Ok(stored) = cp_proto::StoredVote::try_from(vote_ref)
    {
        let mut v = Vote::new(stored.term, stored.node_id);
        if stored.is_committed {
            v = Vote::new_committed(stored.term, stored.node_id);
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
        && let Ok(stored) = cp_proto::StoredLogId::try_from(id_ref)
    {
        return Some(LogId::new(
            CommittedLeaderId::new(stored.term, 0u64),
            stored.index,
        ));
    }
    None
}

pub(crate) fn membership_to_proto(sm: &StoredMembership) -> cp_proto::StoredMembership {
    cp_proto::StoredMembership {
        log_id: sm.log_id().map(|id| {
            Box::new(cp_proto::StoredLogId {
                term: id.leader_id.term,
                index: id.index,
            })
        }),
        voter_ids: Some(sm.membership().voter_ids().collect()),
        nodes: Some(
            sm.membership()
                .nodes()
                .map(|(_, node)| cp_proto::NodeEndpoint {
                    uuid: Some(node_proto::Uuid {
                        high: node.node_uuid_high,
                        low: node.node_uuid_low,
                    }),
                    addr: Some(node.addr.clone()),
                })
                .collect(),
        ),
        node_ids: Some(sm.membership().nodes().map(|(id, _)| *id).collect()),
        configs: Some(
            sm.membership()
                .get_joint_config()
                .iter()
                .map(|voters| cp_proto::VoterConfig {
                    voter_ids: Some(voters.iter().copied().collect()),
                })
                .collect(),
        ),
    }
}

pub(crate) fn membership_from_proto(
    stored: cp_proto::StoredMembership,
) -> Option<StoredMembership> {
    let log_id = stored
        .log_id
        .map(|id| LogId::new(CommittedLeaderId::new(id.term, 0), id.index));
    let nodes = stored.nodes.unwrap_or_default();
    let ids = stored.node_ids;
    if ids.as_ref().is_some_and(|ids| ids.len() != nodes.len()) {
        return None;
    }
    let mut nodes_map = BTreeMap::new();
    for (index, node) in nodes.into_iter().enumerate() {
        let uuid = node.uuid?;
        let id = ids.as_ref().map_or(uuid.low, |ids| ids[index]);
        if nodes_map
            .insert(
                id,
                ControlPlaneNode::new(node.addr?, aaron_core::Uuid::new(uuid.high, uuid.low)),
            )
            .is_some()
        {
            return None;
        }
    }
    // Old on-disk records contain only the flattened voter list.
    let configs = match stored.configs {
        Some(configs) => configs
            .into_iter()
            .map(|c| c.voter_ids.unwrap_or_default().into_iter().collect())
            .collect(),
        None => vec![stored.voter_ids.unwrap_or_default().into_iter().collect()],
    };
    Some(StoredMembership::new(
        log_id,
        openraft::Membership::new(configs, nodes_map),
    ))
}

pub fn serialize_stored_membership(sm: &StoredMembership) -> Vec<u8> {
    let mut builder = planus::Builder::new();
    let offset = membership_to_proto(sm).prepare(&mut builder);
    builder.finish(offset, None).to_vec()
}

pub fn deserialize_stored_membership(bytes: &[u8]) -> Option<StoredMembership> {
    let value = cp_proto::StoredMembershipRef::read_as_root(bytes).ok()?;
    membership_from_proto(cp_proto::StoredMembership::try_from(value).ok()?)
}

pub(crate) fn snapshot_meta_to_proto(meta: &SnapshotMeta) -> cp_proto::StoredSnapshotMeta {
    cp_proto::StoredSnapshotMeta {
        last_log_id: meta.last_log_id.map(|id| {
            Box::new(cp_proto::StoredLogId {
                term: id.leader_id.term,
                index: id.index,
            })
        }),
        last_membership: Some(Box::new(membership_to_proto(&meta.last_membership))),
        snapshot_id: Some(meta.snapshot_id.clone()),
    }
}

pub(crate) fn snapshot_meta_from_proto(meta: cp_proto::StoredSnapshotMeta) -> Option<SnapshotMeta> {
    Some(SnapshotMeta {
        last_log_id: meta
            .last_log_id
            .map(|id| LogId::new(CommittedLeaderId::new(id.term, 0), id.index)),
        last_membership: membership_from_proto(*meta.last_membership?)?,
        snapshot_id: meta.snapshot_id?,
    })
}

fn encode_snapshot_meta(meta: &SnapshotMeta) -> Vec<u8> {
    let mut builder = planus::Builder::new();
    let offset = snapshot_meta_to_proto(meta).prepare(&mut builder);
    builder.finish(offset, None).to_vec()
}

fn decode_snapshot_meta(bytes: &[u8]) -> Option<SnapshotMeta> {
    let value = cp_proto::StoredSnapshotMetaRef::read_as_root(bytes).ok()?;
    snapshot_meta_from_proto(cp_proto::StoredSnapshotMeta::try_from(value).ok()?)
}

pub fn serialize_stored_log_entry(entry: &Entry) -> Vec<u8> {
    let mut builder = planus::Builder::new();
    let term = entry.log_id.leader_id.term;
    let index = entry.log_id.index;

    let (entry_type, normal_op, normal_key, normal_value, membership_proto) = match &entry.payload {
        EntryPayload::Blank => (0u8, 0u8, None, None, None),
        EntryPayload::Normal(req) => match req {
            ClientRequest::Set { key, value } => {
                (1u8, 0u8, Some(key.clone()), Some(value.clone()), None)
            }
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
            let stored =
                membership_to_proto(&StoredMembership::new(Some(entry.log_id), mem.clone()));
            (2u8, 0u8, None, None, Some(Box::new(stored)))
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
        && let Ok(stored) = cp_proto::StoredLogEntry::try_from(entry_ref)
    {
        let payload = match stored.entry_type {
            0 => EntryPayload::Blank,
            1 => match stored.normal_op {
                0 => {
                    let key = stored.normal_key?;
                    let value = stored.normal_value?;
                    EntryPayload::Normal(ClientRequest::Set { key, value })
                }
                1 => {
                    let key = stored.normal_key?;
                    EntryPayload::Normal(ClientRequest::Delete { key })
                }
                2 => {
                    let mut bytes = vec![1, 2];
                    bytes.extend(stored.normal_value?);
                    decode_payload(&bytes).ok()?
                }
                _ => return None,
            },
            2 => EntryPayload::Membership(
                membership_from_proto(*stored.membership?)?
                    .membership()
                    .clone(),
            ),
            _ => return None,
        };

        return Some(Entry {
            log_id: LogId::new(CommittedLeaderId::new(stored.term, 0u64), stored.index),
            payload,
        });
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
        let k_len =
            u32::from_le_bytes(self.bytes[self.idx..self.idx + 4].try_into().unwrap()) as usize;
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
        let v_len =
            u32::from_le_bytes(self.bytes[self.idx..self.idx + 4].try_into().unwrap()) as usize;
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
    if let Ok(mut iter) = SnapshotZeroCopyIterator::new(bytes) {
        let mut map = BTreeMap::new();
        for item in iter.by_ref() {
            let entry = item?;
            if map
                .insert(entry.key.to_string(), entry.val.to_vec())
                .is_some()
            {
                return Err("duplicate snapshot key".to_string());
            }
        }
        if iter.idx != bytes.len() {
            return Err("trailing snapshot data".to_string());
        }
        return Ok(map);
    }

    // Backward-compatible fallback for legacy JSON snapshots
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

impl ControlPlaneStorage {
    /// Creates a new `ControlPlaneStorage` on the specified keyspace and restores existing state.
    pub async fn new(ctx: Context, keyspace_name: &str) -> Result<Self, aaron_core::Error> {
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
            state_change: Arc::new(Mutex::new(())),
        };

        store.load_from_store().await?;
        Ok(store)
    }

    async fn get_keyspace(&self) -> Result<Keyspace, StorageError<u64>> {
        self.ctx
            .store
            .keyspace(&self.keyspace_name)
            .map_err(|e: aaron_core::BoxError| {
                StorageIOError::read_state_machine(openraft::AnyError::error(e.to_string())).into()
            })
    }

    /// Loads only durable state; an appended log is never evidence of application.
    async fn load_from_store(&self) -> Result<(), aaron_core::Error> {
        fn corrupt(error: impl std::fmt::Display) -> aaron_core::Error {
            aaron_core::Error::new(aaron_core::ErrorKind::Internal, error.to_string())
        }
        fn read<T>(
            ks: &Keyspace,
            key: &[u8],
            decode: impl FnOnce(&[u8]) -> Option<T>,
        ) -> Result<Option<T>, aaron_core::Error> {
            match ks.get(key).map_err(corrupt)? {
                Some(bytes) => decode(&bytes).map(Some).ok_or_else(|| {
                    corrupt(format!(
                        "corrupt Raft record: {}",
                        String::from_utf8_lossy(key)
                    ))
                }),
                None => Ok(None),
            }
        }
        let ks = self
            .ctx
            .store
            .keyspace(&self.keyspace_name)
            .map_err(corrupt)?;
        *self.vote.write().await = read(&ks, b"meta/vote", deserialize_stored_vote)?;
        *self.last_purged_log_id.write().await =
            read(&ks, b"meta/last_purged", deserialize_stored_log_id)?;
        let applied = read(&ks, b"meta/last_applied", deserialize_stored_log_id)?;
        *self.last_applied.write().await = applied;
        *self.last_membership.write().await =
            read(&ks, b"meta/last_membership", deserialize_stored_membership)?.unwrap_or_default();

        let mut log = self.log.write().await;
        let mut cursor = None;
        loop {
            let page = ks
                .scan_prefix(b"log/", cursor.as_deref(), 1000)
                .map_err(corrupt)?;
            for item in page.items {
                let entry = deserialize_stored_log_entry(&item.value)
                    .ok_or_else(|| corrupt("corrupt Raft log entry"))?;
                log.insert(entry.log_id.index, entry);
            }
            if !page.has_more {
                break;
            }
            cursor = page.next_cursor;
        }

        let mut data = self.data.write().await;
        let mut cursor = None;
        loop {
            let page = ks
                .scan_prefix(b"data/", cursor.as_deref(), 1000)
                .map_err(corrupt)?;
            for item in page.items {
                let key = std::str::from_utf8(&item.key).map_err(corrupt)?;
                data.insert(
                    key.strip_prefix("data/")
                        .ok_or_else(|| corrupt("invalid state key"))?
                        .to_string(),
                    item.value.to_vec(),
                );
            }
            if !page.has_more {
                break;
            }
            cursor = page.next_cursor;
        }
        if applied.is_none() && (!data.is_empty() || self.last_purged_log_id.read().await.is_some())
        {
            return Err(corrupt(
                "Raft state is missing last_applied; restore a valid backup instead of guessing from the log",
            ));
        }

        let meta = read(&ks, b"meta/snapshot", decode_snapshot_meta)?;
        let snapshot_data = ks.get(b"snapshot/data").map_err(corrupt)?;
        match (meta, snapshot_data) {
            (Some(meta), Some(bytes)) => {
                decode_snapshot_data(&bytes).map_err(corrupt)?;
                *self.current_snapshot.write().await = Some(openraft::Snapshot {
                    meta,
                    snapshot: Box::new(Cursor::new(bytes.to_vec())),
                });
            }
            (None, None) => {}
            _ => return Err(corrupt("incomplete persisted Raft snapshot")),
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
fn write_error(error: impl std::fmt::Display) -> StorageError<u64> {
    StorageIOError::write(openraft::AnyError::error(error.to_string())).into()
}

impl RaftStorage<TypeConfig> for ControlPlaneStorage {
    type LogReader = Self;
    type SnapshotBuilder = Self;

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<u64>> {
        let _guard = self.state_change.lock().await;
        let log = self.log.read().await;
        let last_purged_log_id = *self.last_purged_log_id.read().await;
        Ok(LogState {
            last_log_id: log.values().last().map(|e| e.log_id).or(last_purged_log_id),
            last_purged_log_id,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(&mut self, vote: &Vote) -> Result<(), StorageError<u64>> {
        let _guard = self.state_change.lock().await;
        let ks = self.get_keyspace().await?;
        let mut batch = self.ctx.store.batch();
        batch.insert(&ks, b"meta/vote", serialize_stored_vote(vote));
        self.ctx.store.commit_durable(batch).map_err(write_error)?;
        *self.vote.write().await = Some(*vote);
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote>, StorageError<u64>> {
        Ok(*self.vote.read().await)
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<u64>>
    where
        I: IntoIterator<Item = Entry> + OptionalSend,
    {
        let _guard = self.state_change.lock().await;
        let ks = self.get_keyspace().await?;
        let entries: Vec<_> = entries.into_iter().collect();
        let mut batch = self.ctx.store.batch();
        for entry in &entries {
            batch.insert(
                &ks,
                format!("log/{:020}", entry.log_id.index).as_bytes(),
                serialize_stored_log_entry(entry),
            );
        }
        self.ctx.store.commit_durable(batch).map_err(write_error)?;
        let mut log = self.log.write().await;
        for entry in entries {
            log.insert(entry.log_id.index, entry);
        }
        Ok(())
    }

    async fn delete_conflict_logs_since(&mut self, log_id: LogId) -> Result<(), StorageError<u64>> {
        let _guard = self.state_change.lock().await;
        let ks = self.get_keyspace().await?;
        let mut log = self.log.write().await;
        let keys: Vec<_> = log.range(log_id.index..).map(|(id, _)| *id).collect();
        let mut batch = self.ctx.store.batch();
        for id in &keys {
            batch.remove(&ks, format!("log/{id:020}").as_bytes());
        }
        self.ctx.store.commit_durable(batch).map_err(write_error)?;
        for id in keys {
            log.remove(&id);
        }
        Ok(())
    }

    async fn purge_logs_upto(&mut self, log_id: LogId) -> Result<(), StorageError<u64>> {
        let _guard = self.state_change.lock().await;
        let ks = self.get_keyspace().await?;
        let mut log = self.log.write().await;
        let keys: Vec<_> = log.range(..=log_id.index).map(|(id, _)| *id).collect();
        let mut batch = self.ctx.store.batch();
        batch.insert(&ks, b"meta/last_purged", serialize_stored_log_id(&log_id));
        for id in &keys {
            batch.remove(&ks, format!("log/{id:020}").as_bytes());
        }
        self.ctx.store.commit_durable(batch).map_err(write_error)?;
        for id in keys {
            log.remove(&id);
        }
        *self.last_purged_log_id.write().await = Some(log_id);
        Ok(())
    }

    async fn last_applied_state(
        &mut self,
    ) -> Result<(Option<LogId>, StoredMembership), StorageError<u64>> {
        let _guard = self.state_change.lock().await;
        Ok((
            *self.last_applied.read().await,
            self.last_membership.read().await.clone(),
        ))
    }

    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry],
    ) -> Result<Vec<ClientResponse>, StorageError<u64>> {
        let _guard = self.state_change.lock().await;
        let ks = self.get_keyspace().await?;
        let mut batch = self.ctx.store.batch();
        for entry in entries {
            batch.insert(
                &ks,
                b"meta/last_applied",
                serialize_stored_log_id(&entry.log_id),
            );
            match &entry.payload {
                EntryPayload::Blank => {}
                EntryPayload::Normal(ClientRequest::Set { key, value }) => {
                    batch.insert(&ks, format!("data/{key}").as_bytes(), value.as_slice())
                }
                EntryPayload::Normal(ClientRequest::Delete { key }) => {
                    batch.remove(&ks, format!("data/{key}").as_bytes())
                }
                EntryPayload::Normal(ClientRequest::SetBatch { entries }) => {
                    for (key, value) in entries {
                        batch.insert(&ks, format!("data/{key}").as_bytes(), value.as_slice());
                    }
                }
                EntryPayload::Membership(mem) => batch.insert(
                    &ks,
                    b"meta/last_membership",
                    serialize_stored_membership(&StoredMembership::new(
                        Some(entry.log_id),
                        mem.clone(),
                    )),
                ),
            }
        }
        self.ctx.store.commit_durable(batch).map_err(write_error)?;

        let mut data = self.data.write().await;
        let mut responses = Vec::with_capacity(entries.len());
        for entry in entries {
            let value = match &entry.payload {
                EntryPayload::Blank => None,
                EntryPayload::Normal(ClientRequest::Set { key, value }) => {
                    data.insert(key.clone(), value.clone());
                    Some(value.clone())
                }
                EntryPayload::Normal(ClientRequest::Delete { key }) => data.remove(key),
                EntryPayload::Normal(ClientRequest::SetBatch { entries }) => {
                    data.extend(entries.iter().cloned());
                    None
                }
                EntryPayload::Membership(mem) => {
                    *self.last_membership.write().await =
                        StoredMembership::new(Some(entry.log_id), mem.clone());
                    None
                }
            };
            *self.last_applied.write().await = Some(entry.log_id);
            responses.push(ClientResponse {
                success: true,
                value,
            });
        }
        Ok(responses)
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
        // Validate before touching the current state. One durable batch makes replacement
        // atomic even if the process stops while installing a snapshot.
        let data = decode_snapshot_data(snapshot_data.get_ref()).map_err(write_error)?;
        let _guard = self.state_change.lock().await;
        let ks = self.get_keyspace().await?;
        let mut batch = self.ctx.store.batch();
        let mut cursor = None;
        loop {
            let page = ks
                .scan_prefix(b"data/", cursor.as_deref(), 1000)
                .map_err(write_error)?;
            for item in page.items {
                batch.remove(&ks, item.key);
            }
            if !page.has_more {
                break;
            }
            cursor = page.next_cursor;
        }
        for (key, value) in &data {
            batch.insert(&ks, format!("data/{key}").as_bytes(), value.as_slice());
        }
        match meta.last_log_id {
            Some(id) => batch.insert(&ks, b"meta/last_applied", serialize_stored_log_id(&id)),
            None => batch.remove(&ks, b"meta/last_applied"),
        }
        batch.insert(
            &ks,
            b"meta/last_membership",
            serialize_stored_membership(&meta.last_membership),
        );
        batch.insert(&ks, b"meta/snapshot", encode_snapshot_meta(meta));
        batch.insert(&ks, b"snapshot/data", snapshot_data.get_ref().as_slice());
        self.ctx.store.commit_durable(batch).map_err(write_error)?;

        *self.data.write().await = data;
        *self.last_applied.write().await = meta.last_log_id;
        *self.last_membership.write().await = meta.last_membership.clone();
        *self.current_snapshot.write().await = Some(openraft::Snapshot {
            meta: meta.clone(),
            snapshot: snapshot_data,
        });
        Ok(())
    }

    async fn get_current_snapshot(&mut self) -> Result<Option<Snapshot>, StorageError<u64>> {
        Ok(self.current_snapshot.read().await.clone())
    }
}

impl RaftSnapshotBuilder<TypeConfig> for ControlPlaneStorage {
    async fn build_snapshot(&mut self) -> Result<Snapshot, StorageError<u64>> {
        let _guard = self.state_change.lock().await;
        let last_applied = *self.last_applied.read().await;
        let meta = SnapshotMeta {
            last_log_id: last_applied,
            last_membership: self.last_membership.read().await.clone(),
            snapshot_id: format!(
                "{}-{}",
                last_applied.map(|id| id.to_string()).unwrap_or_default(),
                aaron_core::Uuid::random()
            ),
        };
        let data_bytes = encode_snapshot_data(&*self.data.read().await);
        let ks = self.get_keyspace().await?;
        let mut batch = self.ctx.store.batch();
        batch.insert(&ks, b"meta/snapshot", encode_snapshot_meta(&meta));
        batch.insert(&ks, b"snapshot/data", data_bytes.as_slice());
        self.ctx.store.commit_durable(batch).map_err(write_error)?;
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
    use aaron_core::{Context, Env, EventHub, Network, NodeId, Store, Uuid};
    use openraft::Membership;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_storage_restart_recovery_of_membership_and_applied()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
        nodes.insert(
            node_id,
            ControlPlaneNode::new("10.0.0.1:18946", Uuid::random()),
        );
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

        storage
            .append_to_log(vec![mem_entry.clone(), normal_entry.clone()])
            .await?;
        storage
            .apply_to_state_machine(&[mem_entry, normal_entry])
            .await?;

        let (applied, mem) = storage.last_applied_state().await?;
        assert_eq!(applied.unwrap().index, 2);
        assert_eq!(mem.membership().voter_ids().collect::<Vec<_>>(), vec![100]);

        // 2. Simulate Node Crash / Restart (Create new storage instance from same underlying store)
        let mut recovered_storage = ControlPlaneStorage::new(ctx, "control-plane").await?;
        let (rec_applied, rec_mem) = recovered_storage.last_applied_state().await?;

        assert_eq!(
            rec_applied.unwrap().index,
            2,
            "last_applied must be preserved across restarts"
        );
        assert_eq!(
            rec_mem.membership().voter_ids().collect::<Vec<_>>(),
            vec![100],
            "membership must be preserved across restarts"
        );
        assert_eq!(
            recovered_storage.get_data("cluster/status").await,
            Some(b"active".to_vec())
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_snapshot_streaming_and_chunked_install()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
            test_data.insert(
                format!("shard_metric_{i:04}"),
                format!("wps_score_{i}").into_bytes(),
            );
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
            last_membership: StoredMembership::new(
                Some(log_id),
                Membership::new(vec![std::collections::BTreeSet::from([1])], BTreeMap::new()),
            ),
            snapshot_id: "snap-2500".to_string(),
        };

        storage
            .install_snapshot(&meta, Box::new(Cursor::new(encoded_bytes)))
            .await?;

        // 3. Verify in-memory cache and persisted keys in Fjall
        assert_eq!(
            storage.get_data("shard_metric_0000").await,
            Some(b"wps_score_0".to_vec())
        );
        assert_eq!(
            storage.get_data("shard_metric_2499").await,
            Some(b"wps_score_2499".to_vec())
        );
        let (applied, _) = storage.last_applied_state().await?;
        assert_eq!(applied.unwrap().index, 250);

        Ok(())
    }
}
