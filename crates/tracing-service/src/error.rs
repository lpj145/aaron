use node::{Error, ErrorKind};
use snafu::Snafu;

/// Strongly-typed tracing service errors.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum TracingError {
    #[snafu(display("Filter directive cannot be empty or whitespace"))]
    EmptyFilterDirective,

    #[snafu(display("Invalid filter directive '{directive}': {source}"))]
    InvalidFilterDirective {
        directive: String,
        source: tracing_subscriber::filter::ParseError,
    },

    #[snafu(display("Subscriber reload handle is not initialized"))]
    ReloadHandleNotInitialized,

    #[snafu(display("Failed to reload tracing filter: {source}"))]
    ReloadFailed {
        source: tracing_subscriber::reload::Error,
    },

    #[snafu(display("Tracing subscriber initialization failed: {source}"))]
    SubscriberInit {
        source: tracing_subscriber::util::TryInitError,
    },
}

impl From<TracingError> for Error {
    fn from(err: TracingError) -> Self {
        let kind = match err {
            TracingError::EmptyFilterDirective => ErrorKind::InvalidInput,
            TracingError::InvalidFilterDirective { .. } => ErrorKind::InvalidInput,
            TracingError::ReloadHandleNotInitialized => ErrorKind::NotFound,
            TracingError::ReloadFailed { .. } => ErrorKind::Internal,
            TracingError::SubscriberInit { .. } => ErrorKind::AlreadyExists,
        };
        Error::new(kind, err.to_string()).with_source(err)
    }
}
