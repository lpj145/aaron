use super::connection::TcpConnection;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A thread-safe connection pool for active outbound TCP connections.
#[derive(Clone, Default, Debug)]
pub struct TcpPool {
    connections: Arc<RwLock<HashMap<SocketAddr, TcpConnection>>>,
}

impl TcpPool {
    /// Creates a new empty `TcpPool`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Gets an existing active connection for `addr`, removing it if closed.
    pub async fn get(&self, addr: &SocketAddr) -> Option<TcpConnection> {
        let read_guard = self.connections.read().await;
        match read_guard.get(addr) {
            Some(conn) if !conn.is_closed() => Some(conn.clone()),
            Some(_) => {
                drop(read_guard);
                // Stale connection found — acquire write lock to prune
                let mut write_guard = self.connections.write().await;
                if let Some(conn) = write_guard.get(addr)
                    && conn.is_closed()
                {
                    write_guard.remove(addr);
                }
                None
            }
            None => None, // Fast path: zero write lock on miss
        }
    }

    /// Inserts an active connection into the pool.
    pub async fn insert(&self, addr: SocketAddr, conn: TcpConnection) {
        let mut write_guard = self.connections.write().await;
        write_guard.insert(addr, conn);
    }

    /// Atomically inserts a newly connected peer if absent, or returns existing active connection.
    pub async fn get_or_insert(&self, addr: SocketAddr, new_conn: TcpConnection) -> TcpConnection {
        let mut write_guard = self.connections.write().await;
        if let Some(existing) = write_guard.get(&addr)
            && !existing.is_closed()
        {
            return existing.clone();
        }
        write_guard.insert(addr, new_conn.clone());
        new_conn
    }

    /// Removes a connection from the pool (e.g. on disconnect or error).
    pub async fn remove(&self, addr: &SocketAddr) -> Option<TcpConnection> {
        let mut write_guard = self.connections.write().await;
        write_guard.remove(addr)
    }

    /// Returns the number of active connections in the pool.
    pub async fn count(&self) -> usize {
        let read_guard = self.connections.read().await;
        read_guard.len()
    }

    /// Returns a list of all peer addresses currently in the pool.
    pub async fn peer_addrs(&self) -> Vec<SocketAddr> {
        let read_guard = self.connections.read().await;
        read_guard.keys().copied().collect()
    }

    /// Clears all connections from the pool.
    pub async fn clear(&self) {
        let mut write_guard = self.connections.write().await;
        write_guard.clear();
    }
}
