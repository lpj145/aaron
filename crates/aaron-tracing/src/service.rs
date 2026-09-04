use aaron_core::{BoxError, Context, Service, ServiceConfig};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};
use tracing_subscriber::{
    EnvFilter, Registry, layer::SubscriberExt, reload, util::SubscriberInitExt,
};

use crate::config::{LogFormat, TracingConfig};
use crate::event::ChangeLogLevel;

/// Type alias for the reload handle managing dynamic EnvFilter updates.
pub type ReloadHandle = reload::Handle<EnvFilter, Registry>;

/// A supervised Tracing Service that configures dynamic subscriber filtering
/// with support for `json` and `pretty` formatting, and reacts to [`ChangeLogLevel`]
/// events published on the node's [`aaron_core::EventHub`].
#[derive(Clone)]
pub struct TracingService {
    handle: Arc<RwLock<Option<ReloadHandle>>>,
    config_override: Option<TracingConfig>,
}

impl Default for TracingService {
    fn default() -> Self {
        Self::new()
    }
}

impl TracingService {
    /// Creates a new `TracingService` resolving settings from the node's environment.
    pub fn new() -> Self {
        Self {
            handle: Arc::new(RwLock::new(None)),
            config_override: None,
        }
    }

    /// Creates a new `TracingService` with explicit configuration settings.
    pub fn with_config(config: TracingConfig) -> Self {
        Self {
            handle: Arc::new(RwLock::new(None)),
            config_override: Some(config),
        }
    }

    /// Dynamically reloads the active log level filter directive.
    pub async fn reload(&self, filter_directive: &str) -> Result<(), BoxError> {
        let trimmed = filter_directive.trim();
        if trimmed.is_empty() {
            return Err(Box::new(std::io::Error::other(
                "Filter directive cannot be empty",
            )));
        }
        let read_guard = self.handle.read().await;
        if let Some(handle) = read_guard.as_ref() {
            let new_filter = EnvFilter::try_new(trimmed).map_err(|e| Box::new(e) as BoxError)?;
            handle
                .reload(new_filter)
                .map_err(|e| Box::new(e) as BoxError)?;
            info!(target: "tracing_service", new_level = %trimmed, "Log level dynamically reloaded");
            Ok(())
        } else {
            Err(Box::new(std::io::Error::other(
                "Cannot reload log level: subscriber reload handle not initialized",
            )))
        }
    }

    /// Initializes the global tracing subscriber with a reload layer.
    pub async fn init_subscriber(&self, config: &TracingConfig) -> Result<(), BoxError> {
        let mut write_guard = self.handle.write().await;
        if write_guard.is_some() {
            return Ok(());
        }

        let filter =
            EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
        let (filter_layer, reload_handle) = reload::Layer::new(filter);

        let installed = match config.log_format {
            LogFormat::Pretty => {
                let fmt_layer = tracing_subscriber::fmt::layer().pretty();
                let subscriber = tracing_subscriber::registry()
                    .with(filter_layer)
                    .with(fmt_layer);
                subscriber.try_init().is_ok()
            }
            LogFormat::Json => {
                let fmt_layer = tracing_subscriber::fmt::layer().json();
                let subscriber = tracing_subscriber::registry()
                    .with(filter_layer)
                    .with(fmt_layer);
                subscriber.try_init().is_ok()
            }
        };

        if installed {
            *write_guard = Some(reload_handle);
        }

        Ok(())
    }
}

impl Service for TracingService {
    type Config = TracingConfig;

    fn name(&self) -> &str {
        "tracing-service"
    }

    async fn run(&self, ctx: Context) -> Result<(), BoxError> {
        // 1. Resolve configuration (from explicit override or environment)
        let config = match &self.config_override {
            Some(cfg) => cfg.clone(),
            None => TracingConfig::from_env(&ctx.env)?,
        };

        // 2. Initialize global subscriber with reloadable layer
        self.init_subscriber(&config).await?;

        info!(
            target: "tracing_service",
            initial_level = %config.log_level,
            format = %config.log_format,
            "TracingService active and listening for ChangeLogLevel events"
        );

        // 3. Subscribe to ChangeLogLevel events on EventHub
        let mut subscriber = ctx.event_hub.subscribe::<ChangeLogLevel>().await;

        // 4. Event loop: dynamically reload log level whenever an event arrives, reacting to ctx.token
        loop {
            tokio::select! {
                _ = ctx.token.cancelled() => {
                    info!(target: "tracing_service", "TracingService received cancellation signal, stopping event loop");
                    break;
                }
                event = subscriber.recv() => {
                    match event {
                        Ok(event) => {
                            info!(target: "tracing_service", requested_filter = %event.filter, "Received ChangeLogLevel event");
                            if let Err(err) = self.reload(&event.filter).await {
                                error!(target: "tracing_service", error = %err, filter = %event.filter, "Failed to reload log level");
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }

        Ok(())
    }
}
