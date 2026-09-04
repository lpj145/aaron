use crate::Env;

/// Definition of a configuration field / environment variable expected by a service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigField {
    /// Environment variable name (e.g. `"P2P_PORT"`).
    pub name: &'static str,
    /// Data type description (e.g. `"u16"`, `"String"`).
    pub type_name: &'static str,
    /// Whether this variable is mandatory for the service to operate.
    pub required: bool,
    /// Default fallback value if the environment variable is not set.
    pub default: Option<&'static str>,
    /// Human-readable description of what this configuration controls.
    pub description: &'static str,
}

impl ConfigField {
    /// Creates a new `ConfigField` with the given name and type name.
    pub const fn new(name: &'static str, type_name: &'static str) -> Self {
        Self {
            name,
            type_name,
            required: false,
            default: None,
            description: "",
        }
    }

    /// Marks this configuration field as required.
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Sets a default fallback value for this configuration field.
    pub const fn default(mut self, val: &'static str) -> Self {
        self.default = Some(val);
        self.required = false;
        self
    }

    /// Adds a descriptive explanation for this configuration field.
    pub const fn description(mut self, desc: &'static str) -> Self {
        self.description = desc;
        self
    }
}

use crate::error::{Error, ErrorKind};
use snafu::Snafu;

/// Errors encountered when resolving and validating service configurations.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(visibility(pub))]
pub enum ConfigError {
    /// A mandatory environment variable was missing.
    #[snafu(display(
        "[{service}] Missing required environment variable '{var_name}'{}",
        if description.is_empty() { String::new() } else { format!(" ({description})") }
    ))]
    MissingRequired {
        service: String,
        var_name: String,
        description: String,
    },
    /// An environment variable could not be parsed into the expected type.
    #[snafu(display(
        "[{service}] Invalid value '{raw_value}' for '{var_name}'; expected type {expected_type}"
    ))]
    InvalidValue {
        service: String,
        var_name: String,
        expected_type: String,
        raw_value: String,
    },
    /// Custom configuration error.
    #[snafu(display("{message}"))]
    Custom { message: String },
}

impl From<ConfigError> for Error {
    fn from(err: ConfigError) -> Self {
        Error::new(ErrorKind::ConfigInvalid, err.to_string()).with_source(err)
    }
}

/// Contract for strongly-typed service configuration structures.
pub trait ServiceConfig: Sized + Send + Sync + 'static {
    /// Declares the configuration schema fields and environment variables expected by this struct.
    fn schema() -> Vec<ConfigField>;

    /// Parses and validates this typed configuration from the node's environment.
    fn from_env(env: &Env) -> Result<Self, ConfigError>;
}

impl ServiceConfig for () {
    fn schema() -> Vec<ConfigField> {
        vec![]
    }

    fn from_env(_env: &Env) -> Result<Self, ConfigError> {
        Ok(())
    }
}
