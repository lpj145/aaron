use crate::error::{Error, ErrorKind};
use snafu::Snafu;

/// Strongly-typed event hub errors for Aaron Node.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum EventHubError {
    #[snafu(display("Event subscription channel has been disconnected"))]
    Disconnected,
}

impl From<EventHubError> for Error {
    fn from(err: EventHubError) -> Self {
        Error::new(ErrorKind::ConnectionClosed, err.to_string()).with_source(err)
    }
}
