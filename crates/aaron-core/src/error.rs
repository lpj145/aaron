use std::fmt;

/// High-level categorization of errors across all Aaron subsystems, inspired by OpenDAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Target entity (key, node, keyspace, service) was not found.
    NotFound,
    /// Target entity already exists or duplicate registration was attempted.
    AlreadyExists,
    /// Store or service is temporarily locked (e.g. during snapshot installation / catch-up).
    LockedForMaintenance,
    /// Caller does not have permission or verification failed.
    PermissionDenied,
    /// Supplied argument, parameter or filter directive is malformed or invalid.
    InvalidInput,
    /// Required configuration is missing, type-mismatched or invalid.
    ConfigInvalid,
    /// Network or operation timeout exceeded deadline.
    Timeout,
    /// Remote peer refused connection or address could not be resolved.
    ConnectionRefused,
    /// Established connection or stream was closed unexpectedly.
    ConnectionClosed,
    /// Network unreachable or no route to host.
    NetworkUnreachable,
    /// Frame, payload or batch exceeds configured limit.
    PayloadTooLarge,
    /// Message or protocol invariants violated.
    ProtocolViolation,
    /// Internal subsystem error or unexpected invariant breach.
    Internal,
    /// Unclassified or external third-party error.
    Unexpected,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "NotFound"),
            Self::AlreadyExists => write!(f, "AlreadyExists"),
            Self::LockedForMaintenance => write!(f, "LockedForMaintenance"),
            Self::PermissionDenied => write!(f, "PermissionDenied"),
            Self::InvalidInput => write!(f, "InvalidInput"),
            Self::ConfigInvalid => write!(f, "ConfigInvalid"),
            Self::Timeout => write!(f, "Timeout"),
            Self::ConnectionRefused => write!(f, "ConnectionRefused"),
            Self::ConnectionClosed => write!(f, "ConnectionClosed"),
            Self::NetworkUnreachable => write!(f, "NetworkUnreachable"),
            Self::PayloadTooLarge => write!(f, "PayloadTooLarge"),
            Self::ProtocolViolation => write!(f, "ProtocolViolation"),
            Self::Internal => write!(f, "Internal"),
            Self::Unexpected => write!(f, "Unexpected"),
        }
    }
}

/// Unified, enriched error type for Aaron Node operations.
///
/// Features OpenDAL-style metadata enrichment:
/// - [`kind`](Self::kind): Structured classification for programmatic matching.
/// - [`operation`](Self::operation): Exact subsystem operation that triggered the error.
/// - [`context`](Self::context): Key-value contextual diagnostics (paths, keys, peers, ports).
/// - [`source`](Self::source): Lower-level underlying error cause with full causal chains.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    operation: &'static str,
    context: Vec<(&'static str, String)>,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl Error {
    /// Creates a new `Error` with the specified kind and descriptive message.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            operation: "",
            context: Vec::new(),
            source: None,
        }
    }

    /// Decorates this error with the specific operation name (e.g. `"store::set"`).
    #[must_use]
    pub fn with_operation(mut self, op: &'static str) -> Self {
        self.operation = op;
        self
    }

    /// Attaches a key-value diagnostic pair to the error context.
    #[must_use]
    pub fn with_context(mut self, key: &'static str, value: impl fmt::Display) -> Self {
        self.context.push((key, value.to_string()));
        self
    }

    /// Attaches the underlying source error cause.
    #[must_use]
    pub fn with_source(
        mut self,
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Returns the high-level classification of this error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Returns the operation that failed, if designated.
    pub fn operation(&self) -> &'static str {
        self.operation
    }

    /// Returns the attached contextual key-value diagnostics.
    pub fn context(&self) -> &[(&'static str, String)] {
        &self.context
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.operation.is_empty() {
            write!(f, "{} failed: {}", self.operation, self.message)?;
        } else {
            write!(f, "{}", self.message)?;
        }

        write!(f, " (kind: {})", self.kind)?;

        if !self.context.is_empty() {
            write!(f, ", context: [")?;
            for (idx, (k, v)) in self.context.iter().enumerate() {
                if idx > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{} = {}", k, v)?;
            }
            write!(f, "]")?;
        }

        if let Some(ref source) = self.source {
            write!(f, " => source: {}", source)?;
        }

        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| &**s as &(dyn std::error::Error + 'static))
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        let kind = match err.kind() {
            std::io::ErrorKind::NotFound => ErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
            std::io::ErrorKind::ConnectionRefused => ErrorKind::ConnectionRefused,
            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe => {
                ErrorKind::ConnectionClosed
            }
            std::io::ErrorKind::TimedOut => ErrorKind::Timeout,
            std::io::ErrorKind::AlreadyExists => ErrorKind::AlreadyExists,
            _ => ErrorKind::Internal,
        };
        Error::new(kind, err.to_string()).with_source(err)
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(err: std::str::Utf8Error) -> Self {
        Error::new(ErrorKind::InvalidInput, "invalid UTF-8 byte sequence").with_source(err)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(err: std::string::FromUtf8Error) -> Self {
        Error::new(ErrorKind::InvalidInput, "invalid UTF-8 string").with_source(err)
    }
}
