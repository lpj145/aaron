use aaron_core::{Error, ErrorKind};
use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum AdminError {
    #[snafu(display("HTTP server bind failed at {addr}: {source}"))]
    Bind {
        addr: String,
        source: std::io::Error,
    },

    #[snafu(display("Failed to serve admin HTTP server: {source}"))]
    Serve { source: std::io::Error },

    #[snafu(display("Storage operation error: {message}"))]
    Store { message: String },

    #[snafu(display("Cluster membership operation error: {message}"))]
    Membership { message: String },

    #[snafu(display("Tracing reload error: {message}"))]
    Tracing { message: String },

    #[snafu(display("Invalid request: {message}"))]
    InvalidRequest { message: String },
}

impl From<AdminError> for Error {
    fn from(err: AdminError) -> Self {
        Error::new(ErrorKind::Unexpected, err.to_string()).with_source(err)
    }
}
