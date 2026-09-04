use aaron_core::{ConfigError, ConfigField, Env, ServiceConfig};
use std::fmt;
use std::str::FromStr;

/// Supported log formatting modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Structured JSON formatting (default).
    #[default]
    Json,
    /// Human-readable, colored pretty formatting.
    Pretty,
}

impl FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "pretty" => Ok(Self::Pretty),
            other => Err(format!(
                "unrecognized LOG_FORMAT '{other}'; expected 'json' or 'pretty'"
            )),
        }
    }
}

impl fmt::Display for LogFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json => write!(f, "json"),
            Self::Pretty => write!(f, "pretty"),
        }
    }
}

/// Strongly-typed configuration for [`crate::TracingService`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracingConfig {
    /// Initial log level filter directive (e.g. `"info"`, `"debug"`, `"trace"`). Defaults to `"info"`.
    pub log_level: String,
    /// Log output formatting mode (`Json` or `Pretty`). Defaults to `Json`.
    pub log_format: LogFormat,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            log_format: LogFormat::Json,
        }
    }
}

impl TracingConfig {
    /// Creates a new `TracingConfig` with default settings (`LOG_LEVEL=info`, `LOG_FORMAT=json`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the initial log level filter directive.
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = level.into();
        self
    }

    /// Sets the log formatting mode.
    pub fn log_format(mut self, format: LogFormat) -> Self {
        self.log_format = format;
        self
    }

    /// Convenience method to configure JSON format.
    pub fn json(mut self) -> Self {
        self.log_format = LogFormat::Json;
        self
    }

    /// Convenience method to configure Pretty format.
    pub fn pretty(mut self) -> Self {
        self.log_format = LogFormat::Pretty;
        self
    }
}

impl ServiceConfig for TracingConfig {
    fn schema() -> Vec<ConfigField> {
        vec![
            ConfigField::new("LOG_LEVEL", "String")
                .default("info")
                .description("Initial tracing log level filter directive (e.g. info, debug, trace, crate=debug)"),
            ConfigField::new("LOG_FORMAT", "String")
                .default("json")
                .description("Log output format: 'json' (default) or 'pretty'"),
        ]
    }

    fn from_env(env: &Env) -> Result<Self, ConfigError> {
        let log_level = env
            .get::<String>("LOG_LEVEL")
            .unwrap_or_else(|| "info".to_string());

        let format_str = env
            .get::<String>("LOG_FORMAT")
            .unwrap_or_else(|| "json".to_string());

        let log_format =
            LogFormat::from_str(&format_str).map_err(|_| ConfigError::InvalidValue {
                service: "tracing-service".to_string(),
                var_name: "LOG_FORMAT".to_string(),
                expected_type: "LogFormat ('json' or 'pretty')".to_string(),
                raw_value: format_str,
            })?;

        Ok(Self {
            log_level,
            log_format,
        })
    }
}
