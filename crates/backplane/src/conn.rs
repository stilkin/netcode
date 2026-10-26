//! Reading and writing [`Frame`]s over tokio async byte streams.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{Error, Frame, MAX_FRAME_LEN};

/// Connection failure: transport or protocol.
#[derive(Debug, Error)]
pub enum ConnError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Protocol(#[from] Error),
}

/// Reads one length-prefixed frame. Oversized or truncated frames are
/// protocol errors: the caller must close the connection.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Frame, ConnError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_LEN {
        return Err(ConnError::Protocol(Error::TooLarge {
            len,
            max: MAX_FRAME_LEN,
        }));
    }
    if len < 2 {
        return Err(ConnError::Protocol(Error::Truncated {
            needed: 2,
            got: len,
        }));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Frame::decode(&buf).map_err(ConnError::Protocol)
}

/// Writes one length-prefixed frame.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &Frame,
) -> Result<(), ConnError> {
    let bytes = frame.encode_to_vec()?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}
