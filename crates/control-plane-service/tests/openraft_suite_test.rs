use control_plane_service::storage::ControlPlaneStorage;
use control_plane_service::types::TypeConfig;
use node::{Context, Env, EventHub, Network, NodeId, Store, Uuid};
use openraft::storage::Adaptor;
use openraft::testing::StoreBuilder;
use std::sync::Arc;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

type LogStore = Adaptor<TypeConfig, ControlPlaneStorage>;
type StateMachine = Adaptor<TypeConfig, ControlPlaneStorage>;

struct TestStoreBuilder;

impl StoreBuilder<TypeConfig, LogStore, StateMachine, TempDir> for TestStoreBuilder {
    async fn build(&self) -> Result<(TempDir, LogStore, StateMachine), openraft::StorageError<u64>> {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp).unwrap();
        let network = Network::new();
        let event_hub = EventHub::new();
        let env = Arc::new(Env::detect());
        let identity = NodeId::new(Uuid::random(), 1, None);
        let token = CancellationToken::new();

        let ctx = Context::new(event_hub, network, store, identity, env, token);
        let storage = ControlPlaneStorage::new(ctx, "control-plane").await.unwrap();
        let (log_store, sm) = Adaptor::new(storage);

        Ok((tmp, log_store, sm))
    }
}

#[test]
fn test_openraft_official_storage_suite() {
    openraft::testing::Suite::test_all(TestStoreBuilder).unwrap();
}
