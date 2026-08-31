use axum::Router;
use membership_service::{MembershipEvent, MembershipHandle};
use node::{BoxError, Context, NodeEvents, Service, ServiceConfig};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;
use tracing::info;
use tracing_service::ChangeLogLevel;

use crate::api::create_api_router;
use crate::config::AdminConfig;
use crate::error::AdminError;
use crate::state::{AppState, ConfigFieldMetadata, EventLogEntry, ServiceMetadata};
use crate::static_files::serve_static;

/// Supervised Admin Dashboard & Management Service serving an embedded Vue.js SPA and REST/SSE API.
#[derive(Clone, Default)]
pub struct AdminService {
    config_override: Option<AdminConfig>,
    membership_handle: Option<MembershipHandle>,
    services_metadata: Vec<ServiceMetadata>,
}

impl AdminService {
    /// Creates a new `AdminService` resolving settings from the node's environment.
    pub fn new() -> Self {
        Self {
            config_override: None,
            membership_handle: None,
            services_metadata: Vec::new(),
        }
    }

    /// Creates a new `AdminService` with explicit configuration settings.
    pub fn with_config(config: AdminConfig) -> Self {
        Self {
            config_override: Some(config),
            membership_handle: None,
            services_metadata: Vec::new(),
        }
    }

    /// Associates a [`MembershipHandle`] to enable cluster topology inspection and join/leave operations.
    pub fn with_membership_handle(mut self, handle: MembershipHandle) -> Self {
        self.membership_handle = Some(handle);
        self
    }

    /// Registers service metadata for schema inspection in the admin dashboard.
    pub fn with_service_schema<S: Service>(mut self, service: &S) -> Self {
        let schema = S::Config::schema()
            .into_iter()
            .map(|f| ConfigFieldMetadata {
                name: f.name.to_string(),
                type_name: f.type_name.to_string(),
                required: f.required,
                default: f.default.map(|s| s.to_string()),
                description: f.description.to_string(),
                current_value: None,
            })
            .collect();

        self.services_metadata.push(ServiceMetadata {
            name: service.name().to_string(),
            schema,
        });
        self
    }
}

impl Service for AdminService {
    type Config = AdminConfig;

    fn name(&self) -> &str {
        "admin-service"
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        // 1. Resolve configuration
        let config = match &self.config_override {
            Some(cfg) => cfg.clone(),
            None => AdminConfig::from_env(&ctx.env)?,
        };

        if !config.enabled {
            info!(target: "admin_service", "AdminService is disabled via ADMIN_ENABLED=false");
            return Ok(());
        }

        // 2. Setup real-time event broadcasting channel
        let (event_tx, _) = broadcast::channel::<EventLogEntry>(500);

        // 3. Assemble application state
        let mut services_list = self.services_metadata.clone();
        if !services_list.iter().any(|s| s.name == "admin-service") {
            services_list.push(ServiceMetadata {
                name: "admin-service".to_string(),
                schema: AdminConfig::schema()
                    .into_iter()
                    .map(|f| ConfigFieldMetadata {
                        name: f.name.to_string(),
                        type_name: f.type_name.to_string(),
                        required: f.required,
                        default: f.default.map(|s| s.to_string()),
                        description: f.description.to_string(),
                        current_value: None,
                    })
                    .collect(),
            });
        }

        let state = AppState {
            ctx: ctx.clone(),
            membership: self.membership_handle.clone(),
            start_time: Instant::now(),
            event_tx: event_tx.clone(),
            static_dir: config.static_dir.clone(),
            services: Arc::new(services_list),
        };

        // 4. Spawn EventHub observers for SSE streaming
        let event_hub = ctx.event_hub.clone();
        let sse_tx_membership = event_tx.clone();
        let token_mem = ctx.token.clone();
        tokio::spawn(async move {
            let mut sub = event_hub.subscribe::<MembershipEvent>().await;
            loop {
                tokio::select! {
                    _ = token_mem.cancelled() => break,
                    event = sub.recv() => {
                        match event {
                            Ok(ev) => {
                                let (event_type, details) = match &ev {
                                    MembershipEvent::Joined(m) => ("MEMBERSHIP_JOINED", serde_json::json!({ "node_id": m.node_id.id().to_string(), "addr": m.addr.to_string(), "status": format!("{}", m.status), "incarnation": m.incarnation })),
                                    MembershipEvent::Alive(m) => ("MEMBERSHIP_ALIVE", serde_json::json!({ "node_id": m.node_id.id().to_string(), "addr": m.addr.to_string(), "status": format!("{}", m.status), "incarnation": m.incarnation })),
                                    MembershipEvent::Suspect(m) => ("MEMBERSHIP_SUSPECT", serde_json::json!({ "node_id": m.node_id.id().to_string(), "addr": m.addr.to_string(), "status": format!("{}", m.status), "incarnation": m.incarnation })),
                                    MembershipEvent::Dead(m) => ("MEMBERSHIP_DEAD", serde_json::json!({ "node_id": m.node_id.id().to_string(), "addr": m.addr.to_string(), "status": format!("{}", m.status), "incarnation": m.incarnation })),
                                    MembershipEvent::Left(m) => ("MEMBERSHIP_LEFT", serde_json::json!({ "node_id": m.node_id.id().to_string(), "addr": m.addr.to_string(), "status": format!("{}", m.status), "incarnation": m.incarnation })),
                                    MembershipEvent::Refuted(m) => ("MEMBERSHIP_REFUTED", serde_json::json!({ "node_id": m.node_id.id().to_string(), "addr": m.addr.to_string(), "status": format!("{}", m.status), "incarnation": m.incarnation })),
                                };
                                let entry = EventLogEntry {
                                    id: node::Uuid::random().to_string(),
                                    timestamp: chrono_like_timestamp(),
                                    source: "membership".to_string(),
                                    event_type: event_type.to_string(),
                                    details,
                                };
                                let _ = sse_tx_membership.send(entry);
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        });

        let event_hub = ctx.event_hub.clone();
        let sse_tx_tracing = event_tx.clone();
        let token_tracing = ctx.token.clone();
        tokio::spawn(async move {
            let mut sub = event_hub.subscribe::<ChangeLogLevel>().await;
            loop {
                tokio::select! {
                    _ = token_tracing.cancelled() => break,
                    event = sub.recv() => {
                        match event {
                            Ok(ev) => {
                                let entry = EventLogEntry {
                                    id: node::Uuid::random().to_string(),
                                    timestamp: chrono_like_timestamp(),
                                    source: "tracing".to_string(),
                                    event_type: "CHANGE_LOG_LEVEL".to_string(),
                                    details: serde_json::json!({ "filter": ev.filter }),
                                };
                                let _ = sse_tx_tracing.send(entry);
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        });

        let event_hub = ctx.event_hub.clone();
        let sse_tx_node = event_tx.clone();
        let token_node = ctx.token.clone();
        tokio::spawn(async move {
            let mut sub = event_hub.subscribe::<NodeEvents>().await;
            loop {
                tokio::select! {
                    _ = token_node.cancelled() => break,
                    event = sub.recv() => {
                        match event {
                            Ok(NodeEvents::BindClusterId { cluster_id }) => {
                                let entry = EventLogEntry {
                                    id: node::Uuid::random().to_string(),
                                    timestamp: chrono_like_timestamp(),
                                    source: "node".to_string(),
                                    event_type: "BIND_CLUSTER_ID".to_string(),
                                    details: serde_json::json!({ "cluster_id": cluster_id.to_string() }),
                                };
                                let _ = sse_tx_node.send(entry);
                            }
                            Ok(NodeEvents::StartService { name }) => {
                                let entry = EventLogEntry {
                                    id: node::Uuid::random().to_string(),
                                    timestamp: chrono_like_timestamp(),
                                    source: "node".to_string(),
                                    event_type: "START_SERVICE".to_string(),
                                    details: serde_json::json!({ "service": name }),
                                };
                                let _ = sse_tx_node.send(entry);
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        });

        // 5. Build Axum Router
        let static_dir = config.static_dir.clone();
        let app = Router::new()
            .nest("/api", create_api_router())
            .fallback(move |uri| serve_static(uri, static_dir.clone()))
            .with_state(state);

        // 6. Bind TCP listener
        let listener = tokio::net::TcpListener::bind(config.bind_addr)
            .await
            .map_err(|e| {
                Box::new(AdminError::Bind {
                    addr: config.bind_addr.to_string(),
                    source: e,
                }) as BoxError
            })?;

        let local_addr = listener.local_addr()?;
        info!(
            target: "admin_service",
            bind_addr = %local_addr,
            "AdminService active: serving Vue.js dashboard and REST API at http://{}",
            local_addr
        );

        // 7. Run HTTP server with graceful shutdown
        let cancel_token = ctx.token.clone();
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                cancel_token.cancelled().await;
                info!(target: "admin_service", "AdminService received cancellation signal, stopping HTTP server");
            })
            .await
            .map_err(|e| {
                Box::new(AdminError::Serve { source: e }) as BoxError
            })?;

        Ok(())
    }
}

fn chrono_like_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", now.as_secs(), now.subsec_millis())
}
