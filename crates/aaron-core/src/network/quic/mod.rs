pub mod pool;
pub mod tls;

use crate::BoxError;
use crate::identity::Uuid;
pub use pool::QuicPool;
use std::net::SocketAddr;
use std::sync::Arc;
pub use tls::{
    P2pServerCertVerifier, build_p2p_client_config, build_p2p_server_config, generate_node_cert,
    generate_self_signed_cert,
};
use tokio::net::ToSocketAddrs;
use tokio::sync::OnceCell;

/// Manager for QUIC transport using Quinn and Web-of-Trust P2P TLS.
///
/// Supports high-performance multiplexed bi-directional streams and
/// automatic outbound connection pooling.
#[derive(Clone, Default, Debug)]
pub struct QuicManager {
    pool: QuicPool,
    client_endpoint_v4: Arc<OnceCell<quinn::Endpoint>>,
    client_endpoint_v6: Arc<OnceCell<quinn::Endpoint>>,
    connecting_locks: Arc<tokio::sync::Mutex<std::collections::HashMap<SocketAddr, Arc<tokio::sync::Mutex<()>>>>>,
}

impl QuicManager {
    /// Creates a new `QuicManager`.
    pub fn new() -> Self {
        Self {
            pool: QuicPool::new(),
            client_endpoint_v4: Arc::new(OnceCell::new()),
            client_endpoint_v6: Arc::new(OnceCell::new()),
            connecting_locks: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Binds a QUIC server endpoint to the specified address with an automatically generated
    /// self-signed Web-of-Trust P2P TLS certificate.
    pub async fn listen(&self, addr: impl ToSocketAddrs) -> Result<quinn::Endpoint, BoxError> {
        let (cert, key) =
            generate_self_signed_cert(vec!["localhost".to_string(), "aaron.node".to_string()])?;
        self.listen_with_cert(addr, cert, key).await
    }

    /// Binds a QUIC server endpoint with an ephemeral self-signed P2P TLS certificate
    /// bound to the specified node's unique [`Uuid`].
    pub async fn listen_for_node(
        &self,
        addr: impl ToSocketAddrs,
        node_uuid: Uuid,
    ) -> Result<quinn::Endpoint, BoxError> {
        let (cert, key) = generate_node_cert(node_uuid)?;
        self.listen_with_cert(addr, cert, key).await
    }

    /// Binds a QUIC server endpoint with explicit TLS certificate and private key.
    pub async fn listen_with_cert(
        &self,
        addr: impl ToSocketAddrs,
        cert: rustls::pki_types::CertificateDer<'static>,
        key: rustls::pki_types::PrivateKeyDer<'static>,
    ) -> Result<quinn::Endpoint, BoxError> {
        let socket_addr = tokio::net::lookup_host(addr).await?.next().ok_or_else(|| {
            Box::new(std::io::Error::other(
                "failed to resolve QUIC listen address",
            )) as BoxError
        })?;

        let server_config = build_p2p_server_config(cert, key)?;
        let endpoint = quinn::Endpoint::server(server_config, socket_addr)?;
        Ok(endpoint)
    }

    /// Connects to a remote QUIC peer with automatic connection pooling and singleflight deduplication.
    ///
    /// If an active connection to `addr` already exists in the pool, it is returned immediately.
    /// Concurrent connection requests to the same address are deduplicated so only a single
    /// QUIC handshake is performed against the target node.
    pub async fn connect(
        &self,
        addr: impl ToSocketAddrs,
        server_name: &str,
    ) -> Result<quinn::Connection, BoxError> {
        let socket_addr = tokio::net::lookup_host(addr).await?.next().ok_or_else(|| {
            Box::new(std::io::Error::other(
                "failed to resolve QUIC target address",
            )) as BoxError
        })?;

        // 1. Fast path: check if we already have an active QUIC connection in the pool
        if let Some(existing) = self.pool.get(&socket_addr).await {
            return Ok(existing);
        }

        // 2. Singleflight: acquire per-address lock to deduplicate concurrent handshakes
        let addr_lock = {
            let mut map = self.connecting_locks.lock().await;
            map.entry(socket_addr)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };

        let _guard = addr_lock.lock().await;

        // 3. Re-check if another concurrent task completed the handshake while we waited
        if let Some(existing) = self.pool.get(&socket_addr).await {
            let mut map = self.connecting_locks.lock().await;
            if Arc::strong_count(&addr_lock) <= 2 {
                map.remove(&socket_addr);
            }
            return Ok(existing);
        }

        // 4. Obtain or initialize client endpoint matching IPv4/IPv6 address family
        let endpoint = self
            .get_or_init_client_endpoint(socket_addr.is_ipv6())
            .await?;

        // 5. Initiate QUIC handshake (singleflight execution)
        let connecting = endpoint.connect(socket_addr, server_name)?;
        let connection = connecting.await?;

        // 6. Register in pool atomically
        let final_conn = self.pool.get_or_insert(socket_addr, connection).await;

        {
            let mut map = self.connecting_locks.lock().await;
            if Arc::strong_count(&addr_lock) <= 2 {
                map.remove(&socket_addr);
            }
        }

        Ok(final_conn)
    }

    /// Connects to a remote peer node by its [`Uuid`] with automatic connection pooling.
    pub async fn connect_node(
        &self,
        addr: impl ToSocketAddrs,
        target_node_uuid: Uuid,
    ) -> Result<quinn::Connection, BoxError> {
        let server_name = format!("{target_node_uuid}");
        self.connect(addr, &server_name).await
    }

    /// Helper to get or lazily initialize the shared client endpoint for IPv4 or IPv6.
    async fn get_or_init_client_endpoint(
        &self,
        is_ipv6: bool,
    ) -> Result<&quinn::Endpoint, BoxError> {
        let (cell, bind_addr) = if is_ipv6 {
            (
                &self.client_endpoint_v6,
                SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], 0)),
            )
        } else {
            (
                &self.client_endpoint_v4,
                SocketAddr::from(([0, 0, 0, 0], 0)),
            )
        };

        cell.get_or_try_init(|| async {
            let mut endpoint = quinn::Endpoint::client(bind_addr)?;
            let client_config = build_p2p_client_config()?;
            endpoint.set_default_client_config(client_config);
            Ok(endpoint)
        })
        .await
    }

    /// Removes a connection from the pool.
    pub async fn disconnect(&self, addr: &SocketAddr) -> Option<quinn::Connection> {
        self.pool.remove(addr).await
    }

    /// Returns a reference to the underlying [`QuicPool`].
    pub fn pool(&self) -> &QuicPool {
        &self.pool
    }
}
