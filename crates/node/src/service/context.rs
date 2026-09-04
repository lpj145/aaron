use crate::{Env, EventHub, Network, NodeId, Store};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Describes a configuration field expected by a supervised service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceConfigFieldDescriptor {
    pub name: String,
    pub type_name: String,
    pub required: bool,
    pub default: Option<String>,
    pub description: String,
}

/// Describes a service registered and managed by the node supervisor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceDescriptor {
    pub name: String,
    pub capabilities: Vec<String>,
    pub schema: Vec<ServiceConfigFieldDescriptor>,
}

/// Runtime context provided to services and background workers in the node.
///
/// Contains shared handles to the node's event bus, network manager, persistent store,
/// node identity, environment configuration, service cancellation token, and node shutdown controller.
#[derive(Clone)]
pub struct Context {
    /// Name of the primary application service running on this node (e.g. "bank", "treasurer").
    pub service_name: String,
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
    /// Registry of currently registered and supervised services and schemas on this node.
    pub services: Arc<tokio::sync::RwLock<Vec<ServiceDescriptor>>>,
    /// Node capability tags, metadata, and roles.
    pub tags: Arc<tokio::sync::RwLock<Vec<String>>>,
    /// Dynamic hardware benchmark and workload performance telemetry.
    pub telemetry: Arc<crate::NodeTelemetry>,
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
        Self::with_tags("node", event_hub, network, store, identity, env, token, Vec::new())
    }

    /// Creates a new `Context` instance with a specific primary service name.
    pub fn named(
        service_name: impl Into<String>,
        event_hub: EventHub,
        network: Network,
        store: Store,
        identity: NodeId,
        env: Arc<Env>,
        token: CancellationToken,
    ) -> Self {
        Self::with_tags(service_name, event_hub, network, store, identity, env, token, Vec::new())
    }

    /// Creates a new `Context` with pre-populated tags.
    pub fn with_tags(
        service_name: impl Into<String>,
        event_hub: EventHub,
        network: Network,
        store: Store,
        identity: NodeId,
        env: Arc<Env>,
        token: CancellationToken,
        tags: Vec<String>,
    ) -> Self {
        let telemetry = Arc::new(crate::NodeTelemetry::default());
        let mut initial_tags = tags;
        if !initial_tags.iter().any(|t| t.starts_with("wps:")) {
            initial_tags.push(format!("wps:{}", telemetry.nominal_wps()));
        }

        Self {
            service_name: service_name.into(),
            event_hub,
            network,
            store,
            identity,
            env,
            shutdown_token: token.clone(),
            token,
            services: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            tags: Arc::new(tokio::sync::RwLock::new(initial_tags)),
            telemetry,
        }
    }

    /// Returns a copy of the node's current tags.
    pub async fn tags(&self) -> Vec<String> {
        self.tags.read().await.clone()
    }

    /// Adds a tag to the node's metadata.
    pub async fn add_tag(&self, tag: impl Into<String>) {
        let mut t = self.tags.write().await;
        let tag_str = tag.into();
        if !t.contains(&tag_str) {
            t.push(tag_str);
        }
    }

    /// Returns the list of registered services and their configuration schemas.
    pub async fn services(&self) -> Vec<ServiceDescriptor> {
        self.services.read().await.clone()
    }

    /// Returns a list of currently registered / supervised service names running on this node.
    pub async fn running_services(&self) -> Vec<String> {
        self.services.read().await.iter().map(|s| s.name.clone()).collect()
    }

    /// Updates the registered supervised services and their schemas on this node.
    pub async fn set_services(&self, services: Vec<ServiceDescriptor>) {
        let mut guard = self.services.write().await;
        *guard = services;
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
