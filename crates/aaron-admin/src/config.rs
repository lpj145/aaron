use aaron_core::{ConfigError, ConfigField, Env, ServiceConfig};
use std::net::SocketAddr;
use std::path::PathBuf;

/// Strongly-typed configuration schema for the Aaron Admin Dashboard Service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminConfig {
    /// Address (IP:Port) to bind the admin HTTP dashboard server.
    pub bind_addr: SocketAddr,
    /// Whether the admin dashboard service is enabled.
    pub enabled: bool,
    /// Optional directory path containing static frontend assets (defaults to embedded Vue SPA).
    pub static_dir: Option<PathBuf>,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:8080".parse().unwrap(),
            enabled: true,
            static_dir: None,
        }
    }
}

impl ServiceConfig for AdminConfig {
    fn schema() -> Vec<ConfigField> {
        vec![
            ConfigField::new("ADMIN_BIND_ADDR", "SocketAddr")
                .default("0.0.0.0:8080")
                .description("HTTP address and port for the Vue.js admin dashboard and REST API"),
            ConfigField::new("ADMIN_ENABLED", "bool")
                .default("true")
                .description("Controls whether the admin web interface and API are active"),
            ConfigField::new("ADMIN_STATIC_DIR", "String")
                .description("Optional path to custom static asset directory for development"),
        ]
    }

    fn from_env(env: &Env) -> Result<Self, ConfigError> {
        let bind_addr = env
            .get::<SocketAddr>("ADMIN_BIND_ADDR")
            .unwrap_or_else(|| "0.0.0.0:8080".parse().unwrap());

        let enabled = env.get::<bool>("ADMIN_ENABLED").unwrap_or(true);

        let static_dir = env
            .get_raw("ADMIN_STATIC_DIR")
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from);

        Ok(Self {
            bind_addr,
            enabled,
            static_dir,
        })
    }
}
