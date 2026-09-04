use std::fmt;

/// Event published on [`aaron_core::EventHub`] to dynamically reload the node's tracing log level filter at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeLogLevel {
    /// New log level filter directive (e.g. `"info"`, `"debug"`, `"trace"`, `"node=debug,tracing_service=trace"`).
    pub filter: String,
}

impl ChangeLogLevel {
    /// Creates a new `ChangeLogLevel` event with the specified filter directive.
    pub fn new(filter: impl Into<String>) -> Self {
        Self {
            filter: filter.into(),
        }
    }

    /// Convenience helper for setting log level to "trace".
    pub fn trace() -> Self {
        Self::new("trace")
    }

    /// Convenience helper for setting log level to "debug".
    pub fn debug() -> Self {
        Self::new("debug")
    }

    /// Convenience helper for setting log level to "info".
    pub fn info() -> Self {
        Self::new("info")
    }

    /// Convenience helper for setting log level to "warn".
    pub fn warn() -> Self {
        Self::new("warn")
    }

    /// Convenience helper for setting log level to "error".
    pub fn error() -> Self {
        Self::new("error")
    }
}

impl fmt::Display for ChangeLogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChangeLogLevel({})", self.filter)
    }
}
