use std::fmt;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Default maximum allowed frame payload size: 3MB (3 * 1024 * 1024 bytes).
pub const DEFAULT_MAX_FRAME_SIZE: usize = 3 * 1024 * 1024;

/// Default maximum allowed frame payload size for consensus/snapshots: 64MB (64 * 1024 * 1024 bytes).
pub const DEFAULT_MAX_RAFT_FRAME_SIZE: usize = 64 * 1024 * 1024;

/// Errors that can occur when reading or writing length-prefixed frames.
#[derive(Debug)]
pub enum FrameError {
    /// Underlying I/O error.
    Io(io::Error),
    /// Frame size exceeds the configured maximum limit (guards against OOM / DoS).
    FrameTooLarge { size: usize, max: usize },
    /// Unexpected EOF encountered while reading frame header or payload.
    UnexpectedEof,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::FrameTooLarge { size, max } => {
                write!(
                    f,
                    "Frame size {size} bytes exceeds max allowed limit of {max} bytes"
                )
            }
            Self::UnexpectedEof => write!(f, "Unexpected EOF while reading frame"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Writes a length-prefixed binary frame to an async writer with a 3MB default size check.
///
/// Encodes a 4-byte big-endian `u32` length prefix followed by the payload bytes,
/// and flushes the writer.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    payload: &[u8],
) -> Result<(), FrameError> {
    write_frame_with_limit(writer, payload, DEFAULT_MAX_FRAME_SIZE).await
}

/// Writes a length-prefixed binary frame with an explicit maximum size limit.
pub async fn write_frame_with_limit<W: AsyncWrite + Unpin>(
    writer: &mut W,
    payload: &[u8],
    max_size: usize,
) -> Result<(), FrameError> {
    if payload.len() > max_size {
        return Err(FrameError::FrameTooLarge {
            size: payload.len(),
            max: max_size,
        });
    }

    let len_prefix = (payload.len() as u32).to_be_bytes();
    writer.write_all(&len_prefix).await?;
    writer.write_all(payload).await?;
    writer.flush().await?;

    Ok(())
}

/// Reads a length-prefixed binary frame from an async reader using the default 3MB limit.
///
/// Returns `Ok(Some(bytes))` on success, or `Ok(None)` if a clean EOF is reached before
/// any bytes of a new frame header are read.
pub async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<Vec<u8>>, FrameError> {
    read_frame_with_limit(reader, DEFAULT_MAX_FRAME_SIZE).await
}

/// Reads a length-prefixed binary frame with an explicit maximum size limit.
pub async fn read_frame_with_limit<R: AsyncRead + Unpin>(
    reader: &mut R,
    max_size: usize,
) -> Result<Option<Vec<u8>>, FrameError> {
    let mut header = [0u8; 4];

    // Attempt to read the 4-byte length prefix
    let mut read_bytes = 0;
    while read_bytes < 4 {
        match reader.read(&mut header[read_bytes..]).await {
            Ok(0) => {
                if read_bytes == 0 {
                    // Clean EOF at start of frame
                    return Ok(None);
                } else {
                    // Incomplete header
                    return Err(FrameError::UnexpectedEof);
                }
            }
            Ok(n) => {
                read_bytes += n;
            }
            Err(err) => {
                return Err(FrameError::Io(err));
            }
        }
    }

    let frame_len = u32::from_be_bytes(header) as usize;

    if frame_len > max_size {
        return Err(FrameError::FrameTooLarge {
            size: frame_len,
            max: max_size,
        });
    }

    let mut buf = vec![0u8; frame_len];
    reader.read_exact(&mut buf).await.map_err(|err| {
        if err.kind() == io::ErrorKind::UnexpectedEof {
            FrameError::UnexpectedEof
        } else {
            FrameError::Io(err)
        }
    })?;

    Ok(Some(buf))
}
