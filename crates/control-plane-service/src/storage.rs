use crate::message::{decode_payload, encode_payload};
use crate::types::{
    ClientRequest, ClientResponse, Entry, LogId, Snapshot, SnapshotMeta,
    StoredMembership, TypeConfig, Vote,
};
use node::{Context, Keyspace, KeyspaceExt};
use openraft::storage::{LogState, RaftLogReader, RaftSnapshotBuilder, RaftStorage};
use openraft::{CommittedLeaderId, EntryPayload, OptionalSend, StorageError, StorageIOError};
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
    data: Arc<RwLock<BTreeMap<String, String>>>,
    last_applied: Arc<RwLock<Option<LogId>>>,
    last_membership: Arc<RwLock<StoredMembership>>,
    last_purged_log_id: Arc<RwLock<Option<LogId>>>,
    current_snapshot: Arc<RwLock<Option<Snapshot>>>,
}

fn encode_entry(entry: &Entry) -> Vec<u8> {
    let mut b = Vec::with_capacity(24);
    b.extend_from_slice(&entry.log_id.leader_id.term.to_le_bytes());
    b.extend_from_slice(&entry.log_id.index.to_le_bytes());
    let payload_bytes = encode_payload(&entry.payload);
    b.extend_from_slice(&(payload_bytes.len() as u32).to_le_bytes());
    b.extend_from_slice(&payload_bytes);
    b
}

fn decode_entry(bytes: &[u8]) -> Option<Entry> {
    if bytes.len() < 20 {
        return None;
    }
    let term = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let index = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let payload_len = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;
    if bytes.len() < 20 + payload_len {
        return None;
    }
    let payload = decode_payload(&bytes[20..20 + payload_len]);
    Some(Entry {
        log_id: LogId::new(CommittedLeaderId::new(term, 0u64), index),
        payload,
    })
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
        let vote_opt = ks
            .get(b"meta/vote")
            .map_err(|e| node::Error::new(node::ErrorKind::Internal, e.to_string()))?;

        if let Some(vote_bytes) = vote_opt
            && vote_bytes.len() >= 16 {
                let term = u64::from_le_bytes(vote_bytes[0..8].try_into().unwrap());
                let node_id = u64::from_le_bytes(vote_bytes[8..16].try_into().unwrap());
                let is_committed = if vote_bytes.len() >= 17 { vote_bytes[16] == 1 } else { false };
                let mut v = Vote::new(term, node_id);
                if is_committed {
                    v = Vote::new_committed(term, node_id);
                }
                *self.vote.write().await = Some(v);
            }

        // 2. Load last purged log id
        let purged_opt = ks
            .get(b"meta/last_purged")
            .map_err(|e| node::Error::new(node::ErrorKind::Internal, e.to_string()))?;
        if let Some(purged_bytes) = purged_opt
            && purged_bytes.len() >= 16 {
                let term = u64::from_le_bytes(purged_bytes[0..8].try_into().unwrap());
                let index = u64::from_le_bytes(purged_bytes[8..16].try_into().unwrap());
                *self.last_purged_log_id.write().await = Some(LogId::new(CommittedLeaderId::new(term, 0u64), index));
            }

        // 3. Load log entries
        let log_page = ks
            .scan_prefix(b"log/", None::<&[u8]>, 100_000)
            .map_err(|e: node::BoxError| node::Error::new(node::ErrorKind::Internal, e.to_string()))?;
        let mut log_guard = self.log.write().await;
        for item in log_page.items {
            if let Some(entry) = decode_entry(&item.value) {
                log_guard.insert(entry.log_id.index, entry);
            }
        }

        // 4. Load state machine data
        let page = ks
            .scan_prefix(b"data/", None::<&[u8]>, 10_000)
            .map_err(|e: node::BoxError| node::Error::new(node::ErrorKind::Internal, e.to_string()))?;
        let mut data_guard = self.data.write().await;
        for item in page.items {
            if let (Some(k_str), Some(v_str)) = (item.key_str(), item.value_str()) {
                let key = k_str.trim_start_matches("data/").to_string();
                data_guard.insert(key, v_str.to_string());
            }
        }

        // 5. Load last applied log id
        if let Ok(Some(applied_bytes)) = ks.get(b"meta/last_applied") {
            if applied_bytes.len() >= 16 {
                let term = u64::from_le_bytes(applied_bytes[0..8].try_into().unwrap());
                let index = u64::from_le_bytes(applied_bytes[8..16].try_into().unwrap());
                *self.last_applied.write().await = Some(LogId::new(CommittedLeaderId::new(term, 0u64), index));
            }
        }

        // 6. Load last membership
        if let Ok(Some(sm_bytes)) = ks.get(b"meta/last_membership") {
            if sm_bytes.len() >= 20 {
                let term = u64::from_le_bytes(sm_bytes[0..8].try_into().unwrap());
                let index = u64::from_le_bytes(sm_bytes[8..16].try_into().unwrap());
                let len = u32::from_le_bytes(sm_bytes[16..20].try_into().unwrap()) as usize;
                if sm_bytes.len() >= 20 + len {
                    if let EntryPayload::Membership(mem) = decode_payload(&sm_bytes[20..20 + len]) {
                        *self.last_membership.write().await = StoredMembership::new(
                            Some(LogId::new(CommittedLeaderId::new(term, 0u64), index)),
                            mem,
                        );
                    }
                }
            }
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
        if self.last_applied.read().await.is_none() {
            if let Some((_, last_entry)) = log_guard.iter().next_back() {
                *self.last_applied.write().await = Some(last_entry.log_id);
            }
        }

        Ok(())
    }

    /// Exposes read access to the in-memory replicated state machine.
    pub async fn get_data(&self, key: &str) -> Option<String> {
        self.data.read().await.get(key).cloned()
    }

    /// Returns a snapshot map of all keys and values in the replicated state machine.
    pub async fn all_data(&self) -> BTreeMap<String, String> {
        self.data.read().await.clone()
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
            let mut bytes = Vec::with_capacity(17);
            bytes.extend_from_slice(&vote.leader_id().term.to_le_bytes());
            bytes.extend_from_slice(&vote.leader_id().voted_for().unwrap_or(0).to_le_bytes());
            bytes.push(if vote.is_committed() { 1 } else { 0 });
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

        for entry in entries {
            let db_key = format!("log/{:020}", entry.log_id.index);
            let db_val = encode_entry(&entry);
            let _ = ks.insert(db_key.as_bytes(), db_val.as_slice());
            log.insert(entry.log_id.index, entry);
        }

        let _ = self.ctx.store.persist();
        Ok(())
    }

    async fn delete_conflict_logs_since(&mut self, log_id: LogId) -> Result<(), StorageError<u64>> {
        let mut log = self.log.write().await;
        let ks: Keyspace = self.get_keyspace().await?;

        let keys_to_remove: Vec<u64> = log.range(log_id.index..).map(|(k, _)| *k).collect();
        for k in keys_to_remove {
            let db_key = format!("log/{:020}", k);
            let _ = ks.remove(db_key.as_bytes());
            log.remove(&k);
        }

        let _ = self.ctx.store.persist();
        Ok(())
    }

    async fn purge_logs_upto(&mut self, log_id: LogId) -> Result<(), StorageError<u64>> {
        let mut log = self.log.write().await;
        let ks: Keyspace = self.get_keyspace().await?;

        *self.last_purged_log_id.write().await = Some(log_id);

        let mut purged_bytes = Vec::with_capacity(16);
        purged_bytes.extend_from_slice(&log_id.leader_id.term.to_le_bytes());
        purged_bytes.extend_from_slice(&log_id.index.to_le_bytes());
        let _ = ks.insert(b"meta/last_purged", purged_bytes.as_slice());

        let keys_to_remove: Vec<u64> = log.range(..=log_id.index).map(|(k, _)| *k).collect();
        for k in keys_to_remove {
            let db_key = format!("log/{:020}", k);
            let _ = ks.remove(db_key.as_bytes());
            log.remove(&k);
        }

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

        for entry in entries {
            *self.last_applied.write().await = Some(entry.log_id);

            // Persist last_applied metadata
            let mut applied_bytes = Vec::with_capacity(16);
            applied_bytes.extend_from_slice(&entry.log_id.leader_id.term.to_le_bytes());
            applied_bytes.extend_from_slice(&entry.log_id.index.to_le_bytes());
            let _ = ks.insert(b"meta/last_applied", applied_bytes.as_slice());

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
                        let _ = ks.insert(db_key.as_bytes(), value.as_bytes());
                        res.push(ClientResponse {
                            success: true,
                            value: Some(value.clone()),
                        });
                    }
                    ClientRequest::Delete { key } => {
                        let prev = data.remove(key);
                        let db_key = format!("data/{key}");
                        let _ = ks.remove(db_key.as_bytes());
                        res.push(ClientResponse {
                            success: true,
                            value: prev,
                        });
                    }
                },
                EntryPayload::Membership(ref mem) => {
                    let sm = StoredMembership::new(Some(entry.log_id), mem.clone());
                    *self.last_membership.write().await = sm;

                    // Persist last_membership metadata to disk
                    let mem_payload = encode_payload(&EntryPayload::Membership(mem.clone()));
                    let mut stored_mem_bytes = Vec::with_capacity(20 + mem_payload.len());
                    stored_mem_bytes.extend_from_slice(&entry.log_id.leader_id.term.to_le_bytes());
                    stored_mem_bytes.extend_from_slice(&entry.log_id.index.to_le_bytes());
                    stored_mem_bytes.extend_from_slice(&(mem_payload.len() as u32).to_le_bytes());
                    stored_mem_bytes.extend_from_slice(&mem_payload);
                    let _ = ks.insert(b"meta/last_membership", stored_mem_bytes.as_slice());

                    res.push(ClientResponse {
                        success: true,
                        value: None,
                    });
                }
            }
        }

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
        let deserialized_data: BTreeMap<String, String> = if data_bytes.is_empty() {
            BTreeMap::new()
        } else {
            serde_json::from_slice(data_bytes).map_err(|e| {
                StorageIOError::write_snapshot(Some(meta.signature()), openraft::AnyError::error(e.to_string()))
            })?
        };

        // 1. Update in-memory state
        *self.last_applied.write().await = meta.last_log_id;
        *self.last_membership.write().await = meta.last_membership.clone();
        *self.data.write().await = deserialized_data.clone();

        // 2. Persist state machine to LSM keyspace
        if let Ok(ks) = self.get_keyspace().await {
            // Remove old data/ keys that are not in the snapshot
            if let Ok(existing_page) = ks.scan_prefix(b"data/", None::<&[u8]>, 100_000) {
                for item in existing_page.items {
                    let _ = ks.remove(&*item.key);
                }
            }

            // Insert snapshot key/value entries
            for (key, val) in deserialized_data {
                let db_key = format!("data/{key}");
                let _ = ks.insert(db_key.as_bytes(), val.as_bytes());
            }

            // Persist last purged metadata
            if let Some(log_id) = meta.last_log_id {
                let mut purged_bytes = Vec::with_capacity(16);
                purged_bytes.extend_from_slice(&log_id.leader_id.term.to_le_bytes());
                purged_bytes.extend_from_slice(&log_id.index.to_le_bytes());
                let _ = ks.insert(b"meta/last_purged", purged_bytes.as_slice());
            }

            let _ = self.ctx.store.persist();
        }

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

        let data_bytes = serde_json::to_vec(&data).map_err(|e| {
            StorageIOError::read_state_machine(openraft::AnyError::error(e.to_string()))
        })?;

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
                value: "active".to_string(),
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
        assert_eq!(recovered_storage.get_data("cluster/status").await, Some("active".to_string()));

        Ok(())
    }
}
