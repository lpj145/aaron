use crate::BoxError;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

/// A thread-safe reader handle to a managed TCP connection.
#[derive(Clone, Debug)]
pub struct TcpReader {
    reader: Arc<Mutex<OwnedReadHalf>>,
    peer_addr: SocketAddr,
    is_closed: Arc<AtomicBool>,
}

impl TcpReader {
    /// Reads incoming bytes into the given buffer, returning the number of bytes read.
    pub async fn read(&self, buf: &mut [u8]) -> Result<usize, BoxError> {
        let mut guard = self.reader.lock().await;
        match guard.read(buf).await {
            Ok(0) => {
                self.is_closed.store(true, Ordering::Release);
                Ok(0)
            }
            Ok(n) => Ok(n),
            Err(e) => {
                self.is_closed.store(true, Ordering::Release);
                Err(Box::new(e) as BoxError)
            }
        }
    }

    /// Reads exactly the number of bytes required to fill `buf`.
    pub async fn read_exact(&self, buf: &mut [u8]) -> Result<(), BoxError> {
        let mut guard = self.reader.lock().await;
        match guard.read_exact(buf).await {
            Ok(_) => Ok(()),
            Err(e) => {
                self.is_closed.store(true, Ordering::Release);
                Err(Box::new(e) as BoxError)
            }
        }
    }

    /// Returns the remote peer's socket address.
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    /// Returns whether this stream has encountered EOF or disconnect.
    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::Acquire)
    }

    /// Returns a reference to the shared inner reader mutex.
    pub fn inner(&self) -> &Arc<Mutex<OwnedReadHalf>> {
        &self.reader
    }
}

/// A thread-safe writer handle to a managed TCP connection.
#[derive(Clone, Debug)]
pub struct TcpWriter {
    writer: Arc<Mutex<OwnedWriteHalf>>,
    peer_addr: SocketAddr,
    is_closed: Arc<AtomicBool>,
}

impl TcpWriter {
    /// Writes a complete byte buffer to the TCP stream.
    pub async fn write_all(&self, buf: &[u8]) -> Result<(), BoxError> {
        let mut guard = self.writer.lock().await;
        match guard.write_all(buf).await {
            Ok(_) => Ok(()),
            Err(e) => {
                self.is_closed.store(true, Ordering::Release);
                Err(Box::new(e) as BoxError)
            }
        }
    }

    /// Flushes any pending written bytes to the underlying socket.
    pub async fn flush(&self) -> Result<(), BoxError> {
        let mut guard = self.writer.lock().await;
        match guard.flush().await {
            Ok(_) => Ok(()),
            Err(e) => {
                self.is_closed.store(true, Ordering::Release);
                Err(Box::new(e) as BoxError)
            }
        }
    }

    /// Shuts down the output stream.
    pub async fn shutdown(&self) -> Result<(), BoxError> {
        let mut guard = self.writer.lock().await;
        self.is_closed.store(true, Ordering::Release);
        guard.shutdown().await.map_err(|e| Box::new(e) as BoxError)
    }

    /// Returns the remote peer's socket address.
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    /// Returns whether this stream has been closed or broken.
    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::Acquire)
    }

    /// Returns a reference to the shared inner writer mutex.
    pub fn inner(&self) -> &Arc<Mutex<OwnedWriteHalf>> {
        &self.writer
    }
}

/// A thread-safe, cloneable handle to an established full-duplex TCP connection.
///
/// Allows concurrent reading and writing without writer tasks being blocked
/// by long-running read operations.
#[derive(Clone, Debug)]
pub struct TcpConnection {
    reader: TcpReader,
    writer: TcpWriter,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    is_closed: Arc<AtomicBool>,
}

impl TcpConnection {
    /// Wraps a raw [`TcpStream`] into a managed full-duplex `TcpConnection`.
    pub fn new(stream: TcpStream) -> Result<Self, BoxError> {
        let peer_addr = stream.peer_addr()?;
        let local_addr = stream.local_addr()?;
        let (read_half, write_half) = stream.into_split();
        let is_closed = Arc::new(AtomicBool::new(false));

        Ok(Self {
            reader: TcpReader {
                reader: Arc::new(Mutex::new(read_half)),
                peer_addr,
                is_closed: is_closed.clone(),
            },
            writer: TcpWriter {
                writer: Arc::new(Mutex::new(write_half)),
                peer_addr,
                is_closed: is_closed.clone(),
            },
            peer_addr,
            local_addr,
            is_closed,
        })
    }

    /// Returns the remote peer's socket address.
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    /// Returns the local socket address.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Checks if this connection has encountered EOF, disconnect, or error.
    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::Acquire)
    }

    /// Manually marks this connection as closed.
    pub fn mark_closed(&self) {
        self.is_closed.store(true, Ordering::Release);
    }

    /// Writes a complete byte buffer to the TCP stream.
    pub async fn write_all(&self, buf: &[u8]) -> Result<(), BoxError> {
        self.writer.write_all(buf).await
    }

    /// Flushes any pending written bytes to the underlying socket.
    pub async fn flush(&self) -> Result<(), BoxError> {
        self.writer.flush().await
    }

    /// Reads incoming bytes into the given buffer, returning the number of bytes read.
    pub async fn read(&self, buf: &mut [u8]) -> Result<usize, BoxError> {
        self.reader.read(buf).await
    }

    /// Reads exactly the number of bytes required to fill `buf`.
    pub async fn read_exact(&self, buf: &mut [u8]) -> Result<(), BoxError> {
        self.reader.read_exact(buf).await
    }

    /// Splits this connection into independent reader and writer handles for full-duplex pipelines.
    pub fn split(&self) -> (TcpReader, TcpWriter) {
        (self.reader.clone(), self.writer.clone())
    }

    /// Returns a clone of the reader handle.
    pub fn reader(&self) -> TcpReader {
        self.reader.clone()
    }

    /// Returns a clone of the writer handle.
    pub fn writer(&self) -> TcpWriter {
        self.writer.clone()
    }
}
