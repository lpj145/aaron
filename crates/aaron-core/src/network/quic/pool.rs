use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A thread-safe connection pool for active outbound QUIC connections.
#[derive(Clone, Default, Debug)]
pub struct QuicPool {
    connections: Arc<RwLock<HashMap<SocketAddr, quinn::Connection>>>,
}

impl QuicPool {
    /// Creates a new empty `QuicPool`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Gets an existing active connection for `addr`, removing it if closed.
    pub async fn get(&self, addr: &SocketAddr) -> Option<quinn::Connection> {
        let read_guard = self.connections.read().await;
        match read_guard.get(addr) {
            Some(conn) if conn.close_reason().is_none() => Some(conn.clone()),
            Some(_) => {
                drop(read_guard);
                // Stale connection found — acquire write lock to prune
                let mut write_guard = self.connections.write().await;
                if let Some(conn) = write_guard.get(addr)
                    && conn.close_reason().is_some()
                {
                    write_guard.remove(addr);
                }
                None
            }
            None => None, // Fast path: zero write lock on miss
        }
    }

    /// Inserts an active connection into the pool.
    pub async fn insert(&self, addr: SocketAddr, conn: quinn::Connection) {
        let mut write_guard = self.connections.write().await;
        write_guard.insert(addr, conn);
    }

    /// Atomically inserts a newly connected peer if absent, or returns existing active connection and closes duplicate.
    pub async fn get_or_insert(
        &self,
        addr: SocketAddr,
        new_conn: quinn::Connection,
    ) -> quinn::Connection {
        let mut write_guard = self.connections.write().await;
        if let Some(existing) = write_guard.get(&addr)
            && existing.close_reason().is_none()
        {
            new_conn.close(0u32.into(), b"duplicate connection closed");
            return existing.clone();
        }
        write_guard.insert(addr, new_conn.clone());
        new_conn
    }

    /// Removes a connection from the pool.
    pub async fn remove(&self, addr: &SocketAddr) -> Option<quinn::Connection> {
        let mut write_guard = self.connections.write().await;
        write_guard.remove(addr)
    }

    /// Returns the number of active connections in the pool.
    pub async fn count(&self) -> usize {
        let read_guard = self.connections.read().await;
        read_guard.len()
    }

    /// Clears all connections from the pool.
    pub async fn clear(&self) {
        let mut write_guard = self.connections.write().await;
        write_guard.clear();
    }
}
