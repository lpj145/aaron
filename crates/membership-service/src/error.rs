use node::{Error, ErrorKind};
use snafu::Snafu;
use std::net::SocketAddr;

/// Strongly-typed membership service errors.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum MembershipError {
    #[snafu(display("MembershipService is not yet running"))]
    NotRunning,

    #[snafu(display("Cluster ID mismatch: local cluster is {local}, remote is {remote}"))]
    ClusterMismatch {
        local: node::Uuid,
        remote: node::Uuid,
    },

    #[snafu(display("Invalid join response received from seed {seed}"))]
    InvalidJoinResponse { seed: SocketAddr },

    #[snafu(display("SWIM ping or probe to {target} timed out"))]
    ProbeTimeout { target: SocketAddr },

    #[snafu(display("Malformed membership message: {reason}"))]
    MalformedMessage { reason: String },

    #[snafu(display("Node core error: {source}"))]
    Node { source: node::Error },

    #[snafu(display("Membership I/O error: {source}"))]
    Io { source: std::io::Error },
}

impl From<MembershipError> for Error {
    fn from(err: MembershipError) -> Self {
        let kind = match err {
            MembershipError::NotRunning => ErrorKind::LockedForMaintenance,
            MembershipError::ClusterMismatch { .. } => ErrorKind::PermissionDenied,
            MembershipError::InvalidJoinResponse { .. } => ErrorKind::ProtocolViolation,
            MembershipError::ProbeTimeout { .. } => ErrorKind::Timeout,
            MembershipError::MalformedMessage { .. } => ErrorKind::ProtocolViolation,
            MembershipError::Node { ref source } => source.kind(),
            MembershipError::Io { ref source } => match source.kind() {
                std::io::ErrorKind::ConnectionRefused => ErrorKind::ConnectionRefused,
                std::io::ErrorKind::TimedOut => ErrorKind::Timeout,
                _ => ErrorKind::Internal,
            },
        };
        Error::new(kind, err.to_string()).with_source(err)
    }
}

impl From<std::io::Error> for MembershipError {
    fn from(source: std::io::Error) -> Self {
        MembershipError::Io { source }
    }
}

impl From<node::Error> for MembershipError {
    fn from(source: node::Error) -> Self {
        MembershipError::Node { source }
    }
}
