pub mod codec;
pub mod error;
pub mod quic;
pub mod tcp;
pub mod udp;

pub use codec::{
    DEFAULT_MAX_FRAME_SIZE, DEFAULT_MAX_RAFT_FRAME_SIZE, FrameError, read_frame, read_frame_with_limit, write_frame,
    write_frame_with_limit,
};
pub use error::NetworkError;
pub use quic::{
    P2pServerCertVerifier, QuicManager, QuicPool, build_p2p_client_config, build_p2p_server_config,
    generate_node_cert, generate_self_signed_cert,
};
pub use tcp::{TcpConnection, TcpManager, TcpPool, TcpReader, TcpWriter};
pub use udp::UdpManager;

/// Unified Network Manager providing multi-transport networking (TCP, UDP, QUIC)
/// with automatic outbound connection pooling and inbound listener management.
///
/// # Example
///
/// ```rust
/// use node::Network;
///
/// # async fn doc_example() -> Result<(), node::BoxError> {
/// let network = Network::new();
///
/// // Inbound: bind listener
/// let listener = network.tcp.listen("127.0.0.1:0").await?;
/// let local_addr = listener.local_addr()?;
///
/// // Outbound: connect with automatic pooling
/// let conn1 = network.tcp.connect(local_addr).await?;
/// let conn2 = network.tcp.connect(local_addr).await?; // Reuses conn1 from pool!
/// assert_eq!(network.tcp.pool().count().await, 1);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Default, Debug)]
pub struct Network {
    pub tcp: TcpManager,
    pub udp: UdpManager,
    pub quic: QuicManager,
}

impl Network {
    /// Creates a new `Network` instance with default protocol managers.
    pub fn new() -> Self {
        Self::default()
    }
}
