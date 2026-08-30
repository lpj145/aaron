use crate::error::{Error, ErrorKind};
use snafu::Snafu;

/// Strongly-typed network errors for Aaron Node.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum NetworkError {
    #[snafu(display("Frame size {size} bytes exceeds configured maximum {limit} bytes"))]
    FrameTooLarge { size: usize, limit: usize },

    #[snafu(display("Connection closed unexpectedly during frame read"))]
    UnexpectedDisconnect,

    #[snafu(display("Failed to resolve socket address '{addr}'"))]
    AddressResolution { addr: String },

    #[snafu(display("QUIC connect error: {source}"))]
    QuicConnect { source: quinn::ConnectError },

    #[snafu(display("QUIC connection error: {source}"))]
    QuicConnection { source: quinn::ConnectionError },

    #[snafu(display("QUIC write error: {source}"))]
    QuicWrite { source: quinn::WriteError },

    #[snafu(display("QUIC read error: {source}"))]
    QuicRead { source: quinn::ReadExactError },

    #[snafu(display("TLS error: {source}"))]
    Tls { source: rustls::Error },

    #[snafu(display("Network I/O error: {source}"))]
    Io { source: std::io::Error },
}

impl From<NetworkError> for Error {
    fn from(err: NetworkError) -> Self {
        let kind = match err {
            NetworkError::FrameTooLarge { .. } => ErrorKind::PayloadTooLarge,
            NetworkError::UnexpectedDisconnect => ErrorKind::ConnectionClosed,
            NetworkError::AddressResolution { .. } => ErrorKind::ConnectionRefused,
            NetworkError::QuicConnect { .. } => ErrorKind::ConnectionRefused,
            NetworkError::QuicConnection { .. } => ErrorKind::ConnectionClosed,
            NetworkError::QuicWrite { .. } => ErrorKind::ConnectionClosed,
            NetworkError::QuicRead { .. } => ErrorKind::ConnectionClosed,
            NetworkError::Tls { .. } => ErrorKind::PermissionDenied,
            NetworkError::Io { ref source } => match source.kind() {
                std::io::ErrorKind::ConnectionRefused => ErrorKind::ConnectionRefused,
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe => {
                    ErrorKind::ConnectionClosed
                }
                std::io::ErrorKind::TimedOut => ErrorKind::Timeout,
                _ => ErrorKind::Internal,
            },
        };
        Error::new(kind, err.to_string()).with_source(err)
    }
}

impl From<std::io::Error> for NetworkError {
    fn from(source: std::io::Error) -> Self {
        NetworkError::Io { source }
    }
}

impl From<rustls::Error> for NetworkError {
    fn from(source: rustls::Error) -> Self {
        NetworkError::Tls { source }
    }
}
