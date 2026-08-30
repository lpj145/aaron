use crate::BoxError;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{ToSocketAddrs, UdpSocket};
use tokio::sync::RwLock;

/// Manager for local UDP socket binding and datagram communication.
#[derive(Clone, Default, Debug)]
pub struct UdpManager {
    sockets: Arc<RwLock<HashMap<SocketAddr, Arc<UdpSocket>>>>,
}

impl UdpManager {
    /// Creates a new `UdpManager`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds a new [`UdpSocket`] to the given address.
    pub async fn bind(&self, addr: impl ToSocketAddrs) -> Result<Arc<UdpSocket>, BoxError> {
        let socket = UdpSocket::bind(addr)
            .await
            .map_err(|e| Box::new(e) as BoxError)?;
        let local_addr = socket.local_addr()?;
        let arc_socket = Arc::new(socket);

        let mut write_guard = self.sockets.write().await;
        write_guard.insert(local_addr, Arc::clone(&arc_socket));

        Ok(arc_socket)
    }

    /// Gets an existing bound socket by its local address, or binds a new one if not present.
    pub async fn get_or_bind(&self, addr: impl ToSocketAddrs) -> Result<Arc<UdpSocket>, BoxError> {
        let socket_addr = tokio::net::lookup_host(addr).await?.next().ok_or_else(|| {
            Box::new(std::io::Error::other(
                "failed to resolve UDP socket address",
            )) as BoxError
        })?;

        {
            let read_guard = self.sockets.read().await;
            if let Some(existing) = read_guard.get(&socket_addr) {
                return Ok(Arc::clone(existing));
            }
        }

        self.bind(socket_addr).await
    }

    /// Unbinds/removes a socket from the manager.
    pub async fn unbind(&self, addr: &SocketAddr) -> Option<Arc<UdpSocket>> {
        let mut write_guard = self.sockets.write().await;
        write_guard.remove(addr)
    }

    /// Returns the number of active bound UDP sockets.
    pub async fn count(&self) -> usize {
        let read_guard = self.sockets.read().await;
        read_guard.len()
    }
}
