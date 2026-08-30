use crate::{Env, EventHub, Network, NodeId, Store};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Runtime context provided to services and background workers in the node.
///
/// Contains shared handles to the node's event bus, network manager, persistent store,
/// node identity, environment configuration, service cancellation token, and node shutdown controller.
#[derive(Clone)]
pub struct Context {
    /// Shared in-memory pub/sub event bus (lockless crossfire queues).
    pub event_hub: EventHub,
    /// Multi-transport network manager (TCP, UDP, QUIC with P2P TLS).
    pub network: Network,
    /// Persistent LSM-tree storage engine (Fjall 3.1).
    pub store: Store,
    /// Unique identity and incarnation of this node (128-bit Uuid).
    pub identity: NodeId,
    /// System environment, IP detection, and configuration tracking.
    pub env: Arc<Env>,
    /// Cancellation token tied to this service's local lifecycle (isolated child token).
    pub token: CancellationToken,
    /// Root cancellation token used to initiate a node-wide graceful shutdown.
    shutdown_token: CancellationToken,
}

impl Context {
    /// Creates a new `Context` instance containing all node subsystems.
    pub fn new(
        event_hub: EventHub,
        network: Network,
        store: Store,
        identity: NodeId,
        env: Arc<Env>,
        token: CancellationToken,
    ) -> Self {
        Self {
            event_hub,
            network,
            store,
            identity,
            env,
            shutdown_token: token.clone(),
            token,
        }
    }

    /// Creates a child context sharing all subsystems and root shutdown token,
    /// but with a dedicated child cancellation token for service isolation.
    pub fn with_child_token(&self) -> Self {
        let mut child = self.clone();
        child.token = self.shutdown_token.child_token();
        child
    }

    /// Requests a graceful shutdown of the entire node and all supervised services.
    pub fn shutdown(&self) {
        self.shutdown_token.cancel();
    }

    /// Returns `true` if a node-wide shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown_token.is_cancelled()
    }
}
