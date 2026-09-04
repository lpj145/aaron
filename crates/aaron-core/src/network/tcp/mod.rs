pub mod connection;
pub mod pool;

use crate::BoxError;
pub use connection::{TcpConnection, TcpReader, TcpWriter};
pub use pool::TcpPool;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

/// Manager for TCP listening and pooled outbound connections.
#[derive(Clone, Default, Debug)]
pub struct TcpManager {
    pool: TcpPool,
}

impl TcpManager {
    /// Creates a new `TcpManager`.
    pub fn new() -> Self {
        Self {
            pool: TcpPool::new(),
        }
    }

    /// Binds a [`TcpListener`] to the specified address.
    pub async fn listen(&self, addr: impl ToSocketAddrs) -> Result<TcpListener, BoxError> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| Box::new(e) as BoxError)?;
        Ok(listener)
    }

    /// Connects to a remote peer with automatic connection pooling.
    ///
    /// If an active connection to `addr` already exists in the pool, it is returned.
    /// Otherwise, a new TCP connection is established, added to the pool, and returned.
    pub async fn connect(&self, addr: impl ToSocketAddrs) -> Result<TcpConnection, BoxError> {
        let socket_addr = tokio::net::lookup_host(addr).await?.next().ok_or_else(|| {
            Box::new(std::io::Error::other(
                "failed to resolve target socket address",
            )) as BoxError
        })?;

        // 1. Check if we already have an active connection in the pool
        if let Some(existing) = self.pool.get(&socket_addr).await {
            return Ok(existing);
        }

        // 2. Establish new TCP connection
        let stream = TcpStream::connect(socket_addr)
            .await
            .map_err(|e| Box::new(e) as BoxError)?;
        let conn = TcpConnection::new(stream)?;

        // 3. Register in pool atomically
        let final_conn = self.pool.get_or_insert(socket_addr, conn).await;

        Ok(final_conn)
    }

    /// Removes a connection from the pool (e.g. after a connection failure or close).
    pub async fn disconnect(&self, addr: &SocketAddr) -> Option<TcpConnection> {
        self.pool.remove(addr).await
    }

    /// Returns a reference to the underlying [`TcpPool`].
    pub fn pool(&self) -> &TcpPool {
        &self.pool
    }
}
