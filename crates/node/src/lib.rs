pub use service::{
    AnonymousService, BackoffStrategy, BoxError, ConfigError, ConfigField, Context, RestartPolicy,
    Service, ServiceConfig, ServiceHandler, ServiceOpts, service_fn,
};
use std::{path::Path, path::PathBuf, sync::Arc, time::Duration};
use tokio::task::JoinSet;
pub use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub use crate::events::NodeEvents;
// `supervise`/`SupervisedService`/`TaskResult` are internal plumbing, not part
// of the normal public API — but exposed under `test-util` so integration
// tests in `tests/` can exercise them directly instead of only through `Node`.
#[cfg(feature = "test-util")]
pub use crate::supervise::{SupervisedService, TaskResult, supervise};
#[cfg(not(feature = "test-util"))]
use crate::supervise::{SupervisedService, TaskResult, supervise};

pub mod error;
pub use error::{Error, ErrorKind};
pub type Result<T, E = Error> = std::result::Result<T, E>;

pub use env::{Env, TrackedVar};
pub use event_hub::{EventHub, EventHubError, Subscriber};
pub use identity::{NodeId, NodeIdBuilder, NodeIdRef, Uuid, UuidRef};
pub use network::{
    DEFAULT_MAX_FRAME_SIZE, FrameError, Network, NetworkError, P2pServerCertVerifier, QuicManager,
    QuicPool, TcpConnection, TcpManager, TcpPool, TcpReader, TcpWriter, UdpManager,
    build_p2p_client_config, build_p2p_server_config, generate_node_cert,
    generate_self_signed_cert, read_frame, read_frame_with_limit, write_frame,
    write_frame_with_limit,
};
pub use store::{
    KeyValue, Keyspace, KeyspaceExt, Page, Readable, ScanOptions, Snapshot, Store, StoreBuilder,
    StoreError, WriteBatch,
};

mod env;
mod event_hub;
mod events;
mod identity;
mod network;
mod service;
mod store;
mod supervise;

pub struct Node {
    services: Vec<SupervisedService>,
    dir_path: PathBuf,
    env: Option<Arc<Env>>,
    cancel_token: Option<CancellationToken>,
}

impl Default for Node {
    fn default() -> Self {
        Self::new()
    }
}

impl Node {
    pub fn new() -> Self {
        Self {
            services: vec![],
            dir_path: PathBuf::from("./data"),
            env: None,
            cancel_token: None,
        }
    }

    /// Sets the directory path where the node's persistent store and data will reside.
    pub fn with_dir_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.dir_path = path.into();
        self
    }

    /// Configures a specific [`Env`] instance for the node.
    pub fn with_env(mut self, env: impl Into<Arc<Env>>) -> Self {
        self.env = Some(env.into());
        self
    }

    /// Configures a custom [`CancellationToken`] to trigger or listen for node shutdown.
    pub fn with_cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = Some(token);
        self
    }

    pub fn with<S: Service>(self, s: S) -> Self {
        self.with_opts(s, ServiceOpts::default())
    }

    pub fn with_opts<S: Service>(mut self, s: S, opts: ServiceOpts) -> Self {
        let name = s.name();
        assert!(
            !self
                .services
                .iter()
                .any(|reg| reg.service.dyn_name() == name),
            "Service with name '{name}' is already registered on this Node"
        );
        self.services.push(SupervisedService {
            service: Arc::new(s),
            opts: Arc::new(opts),
        });

        self
    }

    /// Validates environment configuration schemas across all registered services.
    ///
    /// Returns `Ok(())` if all required environment variables are present and valid,
    /// or `Err(Vec<ConfigError>)` containing all missing or invalid configuration fields.
    pub fn validate_env(&self, env: &Env) -> Result<(), Vec<ConfigError>> {
        let mut errors = Vec::new();

        for supervised in &self.services {
            let service_name = supervised.service.dyn_name();
            let schema = supervised.service.dyn_schema();

            for field in schema {
                let val = env.get_raw(field.name);
                match val {
                    None => {
                        if field.required {
                            errors.push(ConfigError::MissingRequired {
                                service: service_name.to_string(),
                                var_name: field.name.to_string(),
                                description: field.description.to_string(),
                            });
                        }
                    }
                    Some(raw) => {
                        // Validate type parsing
                        if !is_valid_type_value(field.type_name, &raw) {
                            errors.push(ConfigError::InvalidValue {
                                service: service_name.to_string(),
                                var_name: field.name.to_string(),
                                expected_type: field.type_name.to_string(),
                                raw_value: raw,
                            });
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Generates content for a comprehensive `.env.example` file based on all
    /// Generates a comprehensive `.env.example` configuration template across all
    /// registered services and their declared configuration schemas.
    pub fn generate_env_example(&self) -> String {
        let mut out = String::new();
        out.push_str(
            "# ==============================================================================\n",
        );
        out.push_str("# Auto-generated .env.example for Aaron Node\n");
        out.push_str(
            "# ==============================================================================\n\n",
        );

        let mut seen = std::collections::HashSet::new();
        for supervised in &self.services {
            let service_name = supervised.service.dyn_name();
            let schema = supervised.service.dyn_schema();

            if schema.is_empty() {
                continue;
            }

            let mut section_buf = String::new();
            for field in schema {
                let clean_name = field.name.replace(['\r', '\n'], "");
                if clean_name.is_empty() || !seen.insert(clean_name.clone()) {
                    continue;
                }

                let req_label = if field.required {
                    "Required".to_string()
                } else if let Some(def) = field.default {
                    format!("Optional, default: {}", def.replace(['\r', '\n'], ""))
                } else {
                    "Optional".to_string()
                };

                if !field.description.is_empty() {
                    section_buf.push_str(&format!(
                        "# {}\n",
                        field.description.replace(['\r', '\n'], " ")
                    ));
                }
                section_buf.push_str(&format!(
                    "# Type: {} ({})\n",
                    field.type_name.replace(['\r', '\n'], ""),
                    req_label
                ));

                if let Some(def) = field.default {
                    let clean_def = def.replace(['\r', '\n'], "");
                    section_buf.push_str(&format!("{clean_name}={clean_def}\n\n"));
                } else {
                    section_buf.push_str(&format!("{clean_name}=\n\n"));
                }
            }

            if !section_buf.is_empty() {
                out.push_str(&format!("# === [{service_name}] ===\n"));
                out.push_str(&section_buf);
            }
        }

        out
    }

    /// Writes the generated `.env.example` template to the specified filesystem path.
    pub fn write_env_example(&self, path: impl AsRef<Path>) -> Result<(), BoxError> {
        let content = self.generate_env_example();
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Loads or creates the node identity in the store under the dedicated "node" namespace.
    fn load_or_create_identity(store: &Store) -> Result<NodeId, BoxError> {
        let node_ks = store.keyspace("node")?;
        match node_ks.get("id")? {
            Some(bytes) => {
                let mut existing = NodeId::from_flatbuffer_bytes(&bytes)?;
                let new_incarnation = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64);
                existing.incarnation = new_incarnation;
                node_ks.insert("id", existing.to_flatbuffer_bytes())?;
                store.persist()?;
                Ok(existing)
            }
            None => {
                let raw_uuid = Uuid::random();
                let new_node = NodeId::with_current_incarnation(raw_uuid, None);
                node_ks.insert("id", new_node.to_flatbuffer_bytes())?;
                store.persist()?;
                Ok(new_node)
            }
        }
    }

    fn update_persisted_cluster_id(store: &Store, cluster_id: Uuid) -> Result<(), BoxError> {
        let node_ks = store.keyspace("node")?;
        if let Some(bytes) = node_ks.get("id")? {
            let mut current_id = NodeId::from_flatbuffer_bytes(&bytes)?;
            if current_id.cluster_id != Some(cluster_id) {
                current_id.cluster_id = Some(cluster_id);
                node_ks.insert("id", current_id.to_flatbuffer_bytes())?;
                store.persist()?;
                info!(
                    target: "node",
                    cluster_id = %cluster_id,
                    "Node cluster identity bound and persisted to store"
                );
            }
        }
        Ok(())
    }

    pub async fn run(self) -> Result<(), BoxError> {
        // 1. Initialize environment & configuration
        let env = self.env.clone().unwrap_or_else(|| Arc::new(Env::detect()));

        // 2. Validate configuration schemas across all registered services (Fail-Fast)
        if let Err(errors) = self.validate_env(&env) {
            error!("Node startup aborted due to configuration errors:");
            for err in &errors {
                error!("  - {err}");
            }
            return Err(Box::new(ConfigError::Custom {
                message: format!(
                    "Configuration validation failed with {} error(s)",
                    errors.len()
                ),
            }) as BoxError);
        }

        // 3. Initialize persistent storage engine at configured directory path
        let store = Store::open(&self.dir_path)?;

        // 4. Load or generate node identity in dedicated "node" namespace
        let node_id = Self::load_or_create_identity(&store)?;

        // 5. Initialize Network, EventHub, and root CancellationToken
        let network = Network::new();
        let event_hub = EventHub::new();
        let token = self.cancel_token.unwrap_or_default();

        // 6. Assemble runtime Context
        let ctx = Context::new(
            event_hub.clone(),
            network,
            store.clone(),
            node_id,
            env.clone(),
            token.clone(),
        );

        info!(
            "Node initialized with ID: {}, incarnation: {}, store path: {}",
            ctx.identity.id(),
            ctx.identity.incarnation,
            self.dir_path.display()
        );

        // Subscriptions for Node core management
        let mut node_events = event_hub.subscribe::<NodeEvents>().await;

        // 7. Supervised execution
        let mut tasks: JoinSet<TaskResult> = JoinSet::new();
        let services = self.services;

        for registry in &services {
            tasks.spawn(supervise(registry.clone(), ctx.clone(), token.clone()));
        }

        let shutdown = shutdown_signal();
        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    break;
                },
                _ = &mut shutdown => {
                    token.cancel();
                    break;
                },
                event = node_events.recv() => match event {
                    Ok(NodeEvents::BindClusterId { cluster_id }) => {
                        Self::update_persisted_cluster_id(&store, cluster_id).unwrap_or_else(|err| {
                            error!("Failed to update cluster identity {err}");
                        });
                    },
                    Ok(NodeEvents::StartService { name }) => {
                        if let Some(target_svc) = services.iter().find(|s| s.service.dyn_name() == name) {
                            info!(
                                target: "node",
                                service = %name,
                                "Spawning dynamic service instance via StartServiceCommand"
                            );
                            tasks.spawn(supervise(
                                target_svc.clone(),
                                ctx.clone(),
                                token.clone(),
                            ));
                        } else {
                            warn!(
                                target: "node",
                                service = %name,
                                "StartServiceCommand received for unregistered service name"
                            );
                        }
                    },
                    Err(err) => {
                        error!("Error during get event {err}");
                    }
                },
                task = tasks.join_next() => {
                    match task {
                        Some(Ok(result)) => {
                            info!("{result}");
                        },
                        Some(Err(err)) => {
                            warn!("{err}")
                        },
                        None => {
                            break;
                        }
                    }
                }
            }
        }

        token.cancel();

        let result = tokio::time::timeout(Duration::from_secs(30), tasks.join_all()).await;

        match result {
            Ok(results) => {
                for result in results {
                    info!("{result}")
                }
            }
            Err(elapsed) => {
                warn!(
                    "Node shutdown after {elapsed} timeout, check if some service is hang after the timeout!"
                )
            }
        }

        Ok(())
    }
}

fn is_valid_type_value(type_name: &str, raw: &str) -> bool {
    let t = type_name.trim();
    if t == "u8" {
        raw.trim().parse::<u8>().is_ok()
    } else if t == "u16" {
        raw.trim().parse::<u16>().is_ok()
    } else if t == "u32" {
        raw.trim().parse::<u32>().is_ok()
    } else if t == "u64" || t == "usize" {
        raw.trim().parse::<u64>().is_ok()
    } else if t == "i8" || t == "i16" || t == "i32" || t == "i64" || t == "isize" {
        raw.trim().parse::<i64>().is_ok()
    } else if t == "bool" {
        raw.trim().parse::<bool>().is_ok()
    } else if t == "f32" || t == "f64" {
        raw.trim().parse::<f64>().is_ok()
    } else {
        true
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            // No sensible fallback other than "never fires" — Ctrl+C (SIGINT) above still works.
            Err(e) => {
                warn!(error = %e, "failed to install SIGTERM handler; this process will only react to Ctrl+C, not SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received SIGINT (Ctrl+C), initiating graceful shutdown");
        }
        _ = terminate => {
            info!("Received SIGTERM, initiating graceful shutdown");
        }
    }
}
