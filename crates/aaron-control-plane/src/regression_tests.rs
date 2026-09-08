use crate::RaftMessage;
use crate::storage::*;
use crate::types::*;
use aaron_core::{Context, Env, EventHub, Network, NodeId, Store, Uuid};
use openraft::storage::{RaftSnapshotBuilder, RaftStorage};
use openraft::{CommittedLeaderId, EntryPayload, Membership};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    path::Path,
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

fn context(path: &Path) -> Context {
    Context::new(
        EventHub::new(),
        Network::new(),
        Store::open(path).unwrap(),
        NodeId::new(Uuid::random(), 1, None),
        Arc::new(Env::detect()),
        CancellationToken::new(),
    )
}

fn id(index: u64) -> LogId {
    LogId::new(CommittedLeaderId::new(2, 1), index)
}

fn joint() -> StoredMembership {
    // Numeric Raft IDs deliberately differ from UUID low bits.
    let nodes = (1..=4)
        .map(|id| {
            (
                id,
                ControlPlaneNode::new("127.0.0.1:9000", Uuid::new(10, id + 100)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    StoredMembership::new(
        Some(id(10)),
        Membership::new(
            vec![BTreeSet::from([1, 2, 3]), BTreeSet::from([2, 3, 4])],
            nodes,
        ),
    )
}

#[test]
fn joint_membership_preserves_quorums_and_node_ids_on_disk_and_wire() {
    let membership = joint();
    assert_eq!(
        deserialize_stored_membership(&serialize_stored_membership(&membership)),
        Some(membership.clone())
    );
    let entry = Entry {
        log_id: id(10),
        payload: EntryPayload::Membership(membership.membership().clone()),
    };
    assert_eq!(
        deserialize_stored_log_entry(&serialize_stored_log_entry(&entry)),
        Some(entry.clone())
    );
    let msg = RaftMessage::Append(openraft::raft::AppendEntriesRequest {
        vote: Vote::new_committed(2, 1),
        prev_log_id: None,
        leader_commit: Some(id(10)),
        entries: vec![entry.clone()],
    });
    let RaftMessage::Append(decoded) = RaftMessage::from_bytes(&msg.to_bytes()).unwrap() else {
        panic!("wrong message");
    };
    assert_eq!(decoded.entries, vec![entry]);
}

#[test]
fn snapshot_wire_preserves_all_metadata_including_index_zero() {
    let meta = SnapshotMeta {
        last_log_id: Some(id(0)),
        last_membership: joint(),
        snapshot_id: "unique-snapshot-id".into(),
    };
    let msg = RaftMessage::Snapshot(openraft::raft::InstallSnapshotRequest {
        vote: Vote::new_committed(2, 1),
        meta: meta.clone(),
        offset: 37,
        data: vec![1, 2, 3],
        done: false,
    });
    let RaftMessage::Snapshot(decoded) = RaftMessage::from_bytes(&msg.to_bytes()).unwrap() else {
        panic!("wrong message");
    };
    assert_eq!(decoded.meta, meta);
    assert_eq!(decoded.offset, 37);
    assert_eq!(decoded.data, [1, 2, 3]);
    assert!(!decoded.done);
}

#[tokio::test]
async fn restart_does_not_apply_uncommitted_logs() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut storage = ControlPlaneStorage::new(context(dir.path()), "raft")
            .await
            .unwrap();
        storage
            .append_to_log(vec![Entry {
                log_id: id(1),
                payload: EntryPayload::Normal(ClientRequest::Set {
                    key: "pending".into(),
                    value: vec![1],
                }),
            }])
            .await
            .unwrap();
        storage
            .append_to_log(vec![Entry {
                log_id: id(2),
                payload: EntryPayload::Membership(joint().membership().clone()),
            }])
            .await
            .unwrap();
    }
    let mut storage = ControlPlaneStorage::new(context(dir.path()), "raft")
        .await
        .unwrap();
    assert_eq!(
        storage.last_applied_state().await.unwrap(),
        (None, StoredMembership::default())
    );
    assert_eq!(storage.get_data("pending").await, None);
}

#[tokio::test]
async fn installed_snapshot_survives_full_store_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let meta = SnapshotMeta {
        last_log_id: Some(id(50)),
        last_membership: joint(),
        snapshot_id: "installed".into(),
    };
    let data = BTreeMap::from([("data/key".into(), vec![7])]);
    {
        let mut storage = ControlPlaneStorage::new(context(dir.path()), "raft")
            .await
            .unwrap();
        storage
            .install_snapshot(&meta, Box::new(Cursor::new(encode_snapshot_data(&data))))
            .await
            .unwrap();
    }
    let mut storage = ControlPlaneStorage::new(context(dir.path()), "raft")
        .await
        .unwrap();
    assert_eq!(
        storage.last_applied_state().await.unwrap(),
        (meta.last_log_id, meta.last_membership.clone())
    );
    assert_eq!(storage.all_data().await, data);
    assert_eq!(
        storage.get_current_snapshot().await.unwrap().unwrap().meta,
        meta
    );
}

#[tokio::test]
async fn built_snapshot_survives_log_purge_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let meta;
    {
        let mut storage = ControlPlaneStorage::new(context(dir.path()), "raft")
            .await
            .unwrap();
        let entry = Entry {
            log_id: id(10),
            payload: EntryPayload::Membership(joint().membership().clone()),
        };
        storage.append_to_log(vec![entry.clone()]).await.unwrap();
        storage.apply_to_state_machine(&[entry]).await.unwrap();
        meta = storage.build_snapshot().await.unwrap().meta;
        storage.purge_logs_upto(id(10)).await.unwrap();
    }
    let mut storage = ControlPlaneStorage::new(context(dir.path()), "raft")
        .await
        .unwrap();
    assert_eq!(
        storage.get_current_snapshot().await.unwrap().unwrap().meta,
        meta
    );
    assert_eq!(storage.last_applied_state().await.unwrap().0, Some(id(10)));
}

#[tokio::test]
async fn rejected_writes_never_advance_memory_or_report_success() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let mut storage = ControlPlaneStorage::new(ctx.clone(), "raft").await.unwrap();
    let entry = Entry {
        log_id: id(1),
        payload: EntryPayload::Normal(ClientRequest::Set {
            key: "old".into(),
            value: vec![1],
        }),
    };
    storage.append_to_log(vec![entry.clone()]).await.unwrap();
    storage
        .apply_to_state_machine(std::slice::from_ref(&entry))
        .await
        .unwrap();
    let old_snapshot = storage.build_snapshot().await.unwrap();
    ctx.store.set_maintenance(true);
    assert!(storage.save_vote(&Vote::new(10, 1)).await.is_err());
    assert_eq!(storage.read_vote().await.unwrap(), None);
    let next = Entry {
        log_id: id(2),
        payload: EntryPayload::Normal(ClientRequest::Delete { key: "old".into() }),
    };
    assert!(storage.append_to_log(vec![next.clone()]).await.is_err());
    assert!(storage.apply_to_state_machine(&[next]).await.is_err());
    assert!(storage.delete_conflict_logs_since(id(1)).await.is_err());
    assert!(storage.purge_logs_upto(id(1)).await.is_err());
    assert!(
        storage
            .install_snapshot(
                &old_snapshot.meta,
                Box::new(Cursor::new(encode_snapshot_data(&BTreeMap::new())))
            )
            .await
            .is_err()
    );
    assert_eq!(storage.get_data("old").await, Some(vec![1]));
    assert_eq!(storage.last_applied_state().await.unwrap().0, Some(id(1)));
    assert_eq!(
        storage.get_log_state().await.unwrap().last_log_id,
        Some(id(1))
    );
    assert_eq!(
        storage.get_log_state().await.unwrap().last_purged_log_id,
        None
    );
    ctx.store.set_maintenance(false);
}

#[tokio::test]
async fn invalid_snapshot_cannot_replace_existing_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = ControlPlaneStorage::new(context(dir.path()), "raft")
        .await
        .unwrap();
    let meta = SnapshotMeta {
        last_log_id: Some(id(5)),
        last_membership: joint(),
        snapshot_id: "valid".into(),
    };
    let data = BTreeMap::from([("key".into(), vec![1, 2, 3])]);
    let bytes = encode_snapshot_data(&data);
    storage
        .install_snapshot(&meta, Box::new(Cursor::new(bytes.clone())))
        .await
        .unwrap();
    for invalid in [
        bytes[..bytes.len() - 1].to_vec(),
        [bytes.as_slice(), &[42]].concat(),
    ] {
        assert!(
            storage
                .install_snapshot(&meta, Box::new(Cursor::new(invalid)))
                .await
                .is_err()
        );
        assert_eq!(storage.all_data().await, data);
    }
}

#[test]
fn malformed_payloads_are_rejected_without_partial_batches() {
    for bytes in [
        &[][..],
        &[1, 2, 255, 255, 255, 255],
        &[1, 2, 1, 0, 0, 0],
        &[255],
    ] {
        assert!(crate::message::decode_payload(bytes).is_err());
    }
}

#[tokio::test]
async fn corrupt_vote_does_not_become_a_fabricated_term_on_restart() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    ctx.store
        .keyspace("raft")
        .unwrap()
        .insert(b"meta/vote", vec![255; 32])
        .unwrap();
    assert!(ControlPlaneStorage::new(ctx, "raft").await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_position_and_data_are_consistent_during_application() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = ControlPlaneStorage::new(context(dir.path()), "raft")
        .await
        .unwrap();
    let mut writer = storage.clone();
    let task = tokio::spawn(async move {
        for index in 1..=50 {
            writer
                .apply_to_state_machine(&[Entry {
                    log_id: id(index),
                    payload: EntryPayload::Normal(ClientRequest::Set {
                        key: "position".into(),
                        value: index.to_le_bytes().to_vec(),
                    }),
                }])
                .await
                .unwrap();
            tokio::task::yield_now().await;
        }
    });
    for _ in 0..30 {
        let snapshot = storage.build_snapshot().await.unwrap();
        let data = decode_snapshot_data(snapshot.snapshot.get_ref()).unwrap();
        match snapshot.meta.last_log_id {
            Some(id) => assert_eq!(data.get("position"), Some(&id.index.to_le_bytes().to_vec())),
            None => assert!(data.is_empty()),
        }
        tokio::task::yield_now().await;
    }
    task.await.unwrap();
}
