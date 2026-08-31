use axum::Router;
use control_plane_service::ControlPlaneHandle;
use membership_service::MembershipHandle;
use node::{BoxError, Context, Service, ServiceConfig};
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

use crate::api::create_api_router;
use crate::config::AdminConfig;
use crate::error::AdminError;
use crate::state::{AppState, ConfigFieldMetadata, ServiceMetadata};
use crate::static_files::serve_static;

/// Supervised Admin Dashboard & Management Service serving an embedded Vue.js SPA and REST/SSE API.
#[derive(Clone, Default)]
pub struct AdminService {
    config_override: Option<AdminConfig>,
    membership_handle: Option<MembershipHandle>,
    control_plane_handle: Option<ControlPlaneHandle>,
    services_metadata: Vec<ServiceMetadata>,
}

impl AdminService {
    /// Creates a new `AdminService` resolving settings from the node's environment.
    pub fn new() -> Self {
        Self {
            config_override: None,
            membership_handle: None,
            control_plane_handle: None,
            services_metadata: Vec::new(),
        }
    }

    /// Creates a new `AdminService` with explicit configuration settings.
    pub fn with_config(config: AdminConfig) -> Self {
        Self {
            config_override: Some(config),
            membership_handle: None,
            control_plane_handle: None,
            services_metadata: Vec::new(),
        }
    }

    /// Associates a [`MembershipHandle`] to enable cluster topology inspection and join/leave operations.
    pub fn with_membership_handle(mut self, handle: MembershipHandle) -> Self {
        self.membership_handle = Some(handle);
        self
    }

    /// Associates a [`ControlPlaneHandle`] to enable Raft consensus control and state machine inspection.
    pub fn with_control_plane_handle(mut self, handle: ControlPlaneHandle) -> Self {
        self.control_plane_handle = Some(handle);
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

        // 2. Assemble application state
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
            control_plane: self.control_plane_handle.clone(),
            start_time: Instant::now(),
            static_dir: config.static_dir.clone(),
            services: Arc::new(services_list),
        };

        // 3. Build Axum Router
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
